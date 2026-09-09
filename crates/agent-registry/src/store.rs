//! `StoreRegistry` — the [`ProviderRegistry`] backed by the shared transactional
//! config store (`agent-config-store`, config design C41 / increment A3).
//!
//! This is the **convergence** backend: instead of a bespoke file/SQLite store,
//! the registry persists onto any [`agent_config_store::Backend`] (memory, file,
//! sqlite, or — the capability A3 unlocks — **postgres**). The legacy
//! [`MemoryRegistry`](crate::MemoryRegistry) / [`FileRegistry`](crate::FileRegistry)
//! / `SqliteRegistry` backends are **untouched** (decision: keep `rusqlite` for
//! `file`/`sqlite`, add postgres as the new `sqlx` tier), so their behaviour — and
//! their tests — are unchanged; this backend adds the shared-store path beside
//! them.
//!
//! **Behaviour-identical by construction.** Every mutation routes through the same
//! shared [`crate::ops`], reads decode the same `pb::Upstream`/`pb::RoutePolicy`
//! wire blobs the SQLite tier uses (one schema, so encodings can't drift), and
//! `route`/`health` run the same [`crate::decide`]/[`crate::static_health`]. The
//! store is a serialized snapshot → op → rewrite cycle, exactly like the SQLite
//! backend's `mutate` (a ≤`MAX_REGISTRY_UPSTREAMS`-row rewrite inside one atomic
//! batch), so the three tiers stay interchangeable.
//!
//! **Untrusted input, fail closed.** Cards decode-then-`validate` on read (an
//! out-of-band-tampered row fails closed at the seam); ids reach the backend only
//! after `safe_segment` (via the shared `ops`/`check_id`), and the backend itself
//! binds them as parameters. `api_key_ref` is stored verbatim as a reference.

use std::sync::Arc;

use agent_config_store::{Backend, Write};
use agent_core::{
    safe_segment, Error, ModelRouterConfig, ProviderRegistry, Result, RouteDecision, RouteHint,
    RoutePolicySpec, Upstream, UpstreamHealth,
};
use agent_proto::pb;
use async_trait::async_trait;
use prost::Message;

use crate::{check_id, decide, not_found, ops, static_health};

/// The collection holding one card per [`Upstream`] (id = the upstream id).
const UPSTREAMS: &str = "upstreams";
/// The collection holding the single routing-policy card.
const POLICY: &str = "route_policy";
/// The fixed id of the one policy card in its collection.
const POLICY_ID: &str = "default";
/// The default single-tenant scope. Per-tenant scoping (a verified tenant per
/// call) arrives with the per-tenant plane (config C35 / increment C2); until
/// then the registry is one un-namespaced control plane under this key.
pub const DEFAULT_TENANT: &str = "local";

/// A [`ProviderRegistry`] persisted on a shared [`Backend`]. Cheap to clone (an
/// `Arc` handle plus the tenant key).
pub struct StoreRegistry {
    backend: Arc<dyn Backend>,
    tenant: String,
}

impl StoreRegistry {
    /// A registry over `backend` under the default single-tenant scope.
    pub fn new(backend: Arc<dyn Backend>) -> Self {
        Self {
            backend,
            tenant: DEFAULT_TENANT.to_string(),
        }
    }

    /// A registry scoped to an explicit tenant (config C2 will call this per
    /// verified identity). The tenant is `safe_segment`-gated — a hostile tenant
    /// is rejected at construction, never persisted or turned into a key.
    pub fn with_tenant(backend: Arc<dyn Backend>, tenant: &str) -> Result<Self> {
        if !safe_segment(tenant) {
            return Err(Error::Registry(format!("invalid tenant `{tenant}`")));
        }
        Ok(Self {
            backend,
            tenant: tenant.to_string(),
        })
    }

    /// Snapshot the whole config from the store. Decoding clamps hostile numbers
    /// and `validate` fails closed on an out-of-band-tampered row; list order is
    /// the backend's insertion order (`pos`), so routing ties break exactly like
    /// the file/memory/sqlite backends.
    async fn load(&self) -> Result<ModelRouterConfig> {
        let upstreams: Vec<Upstream> = self
            .backend
            .list(UPSTREAMS, &self.tenant)
            .await?
            .into_iter()
            .map(|blob| {
                pb::Upstream::decode(blob.as_slice())
                    .map(Upstream::from)
                    .map_err(|e| Error::Registry(format!("stored card decode: {e}")))
            })
            .collect::<Result<_>>()?;
        let policy: RoutePolicySpec =
            match self.backend.get(POLICY, &self.tenant, POLICY_ID).await? {
                Some(blob) => pb::RoutePolicy::decode(blob.as_slice())
                    .map(RoutePolicySpec::from)
                    .map_err(|e| Error::Registry(format!("stored policy decode: {e}")))?,
                // No policy card ⇒ the default policy (a default policy encodes to an
                // empty blob, which the store rejects — so it is stored as *absence*).
                None => RoutePolicySpec::default(),
            };
        let cfg = ModelRouterConfig { upstreams, policy };
        cfg.validate()?;
        Ok(cfg)
    }

    /// One serialized snapshot → shared op → rewrite cycle, committed atomically.
    /// Rewriting the (≤`MAX_REGISTRY_UPSTREAMS`) card set inside a single
    /// [`Backend::apply`] batch mirrors the SQLite tier's `mutate`, keeping the
    /// backends byte-for-byte interchangeable without a per-row path that could
    /// drift from `ops`.
    async fn mutate<T>(&self, f: impl FnOnce(&mut ModelRouterConfig) -> Result<T>) -> Result<T> {
        use std::collections::HashSet;
        let mut cfg = self.load().await?;
        let before: Vec<String> = cfg.upstreams.iter().map(|u| u.id.clone()).collect();
        let out = f(&mut cfg)?;
        cfg.validate()?;
        let after: HashSet<&str> = cfg.upstreams.iter().map(|u| u.id.as_str()).collect();

        let mut writes: Vec<Write> = vec![Write::EnsureTenant {
            tenant: self.tenant.clone(),
        }];
        // Remove cards that disappeared from the snapshot …
        for id in &before {
            if !after.contains(id.as_str()) {
                writes.push(Write::Delete {
                    collection: UPSTREAMS,
                    tenant: self.tenant.clone(),
                    id: id.clone(),
                });
            }
        }
        // … then upsert every current card (upsert keeps `pos` for existing ids,
        // appends new ones — preserving order like `ops::put`'s push).
        for u in &cfg.upstreams {
            writes.push(Write::Put {
                collection: UPSTREAMS,
                tenant: self.tenant.clone(),
                id: u.id.clone(),
                blob: pb::Upstream::from(u.clone()).encode_to_vec(),
            });
        }
        // The policy card: a default policy encodes to empty bytes (which the
        // store rejects as an empty blob), so a default policy is persisted as the
        // *absence* of the card — `load` maps a missing card back to the default.
        let policy_blob = pb::RoutePolicy::from(cfg.policy.clone()).encode_to_vec();
        if policy_blob.is_empty() {
            writes.push(Write::Delete {
                collection: POLICY,
                tenant: self.tenant.clone(),
                id: POLICY_ID.to_string(),
            });
        } else {
            writes.push(Write::Put {
                collection: POLICY,
                tenant: self.tenant.clone(),
                id: POLICY_ID.to_string(),
                blob: policy_blob,
            });
        }
        self.backend.apply(&writes).await?;
        Ok(out)
    }
}

#[async_trait]
impl ProviderRegistry for StoreRegistry {
    async fn list(&self) -> Result<Vec<Upstream>> {
        Ok(self.load().await?.upstreams)
    }
    async fn get(&self, id: &str) -> Result<Upstream> {
        check_id(id)?;
        self.load()
            .await?
            .upstreams
            .into_iter()
            .find(|u| u.id == id)
            .ok_or_else(|| not_found(id))
    }
    async fn put(&self, card: Upstream) -> Result<Upstream> {
        self.mutate(|cfg| ops::put(cfg, card)).await
    }
    async fn delete(&self, id: &str) -> Result<bool> {
        self.mutate(|cfg| ops::delete(cfg, id)).await
    }
    async fn enable(&self, id: &str, enabled: bool) -> Result<Upstream> {
        self.mutate(|cfg| ops::enable(cfg, id, enabled)).await
    }
    async fn get_policy(&self) -> Result<RoutePolicySpec> {
        Ok(self.load().await?.policy)
    }
    async fn put_policy(&self, policy: RoutePolicySpec) -> Result<RoutePolicySpec> {
        self.mutate(|cfg| ops::put_policy(cfg, policy)).await
    }
    async fn route(&self, hint: &RouteHint) -> Result<RouteDecision> {
        Ok(decide(&self.load().await?, hint))
    }
    async fn health(&self) -> Result<Vec<UpstreamHealth>> {
        Ok(static_health(&self.load().await?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdata::{card, config};
    use crate::MemoryRegistry;
    use agent_config_store::MemoryBackend;
    use agent_core::{PoolTier, RouteRole, MAX_REGISTRY_UPSTREAMS};
    use rstest::rstest;

    fn store() -> StoreRegistry {
        StoreRegistry::new(Arc::new(MemoryBackend::new()))
    }

    async fn seeded() -> StoreRegistry {
        let reg = store();
        for u in config().upstreams {
            reg.put(u).await.expect("seed put");
        }
        reg.put_policy(config().policy).await.expect("seed policy");
        reg
    }

    fn hint(role: RouteRole) -> RouteHint {
        RouteHint {
            role: Some(role),
            ..Default::default()
        }
    }

    // desc: a card survives the encode→store→decode roundtrip byte-for-byte.
    #[tokio::test]
    async fn positive_put_get_roundtrips_through_blobs() {
        let reg = store();
        let stored = reg.put(card("kimi")).await.expect("put");
        assert_eq!(stored, card("kimi"));
        assert_eq!(reg.get("kimi").await.expect("get"), card("kimi"));
        assert_eq!(reg.list().await.expect("list").len(), 1);
    }

    // desc: the store backend and the in-memory backend decide + list identically
    // (the behaviour-preservation proof for the converged tier).
    #[tokio::test]
    async fn positive_store_and_memory_backends_agree() {
        let store = seeded().await;
        let mem = MemoryRegistry::new(config()).expect("valid");
        for role in [RouteRole::Judge, RouteRole::Main] {
            assert_eq!(
                store.route(&hint(role)).await.unwrap(),
                mem.route(&hint(role)).await.unwrap(),
                "route disagreement for {role:?}"
            );
        }
        assert_eq!(store.list().await.unwrap(), mem.list().await.unwrap());
        assert_eq!(store.health().await.unwrap(), mem.health().await.unwrap());
        assert_eq!(
            store.get_policy().await.unwrap(),
            mem.get_policy().await.unwrap()
        );
    }

    // desc: enable toggles routing/health but keeps the card, across the store.
    #[tokio::test]
    async fn positive_enable_toggles_routing_but_keeps_the_card() {
        let reg = seeded().await;
        let off = reg.enable("kimi", false).await.expect("disable");
        assert!(!off.enabled);
        assert_eq!(reg.list().await.unwrap().len(), 2);
        assert_eq!(
            reg.route(&hint(RouteRole::Judge)).await.unwrap().chosen,
            "glm"
        );
        assert!(reg.health().await.unwrap().iter().all(|h| h.id != "kimi"));
        assert!(reg.enable("kimi", true).await.expect("enable").enabled);
        assert_eq!(
            reg.route(&hint(RouteRole::Judge)).await.unwrap().chosen,
            "kimi"
        );
    }

    // desc: delete reports existed-then-absent and shrinks the fleet.
    #[tokio::test]
    async fn positive_delete_true_then_false() {
        let reg = seeded().await;
        assert!(reg.delete("glm").await.expect("first"));
        assert!(!reg.delete("glm").await.expect("second"));
        assert_eq!(reg.list().await.unwrap().len(), 1);
    }

    // desc: state persists across a re-open sharing the same backend handle.
    #[tokio::test]
    async fn positive_persists_across_reopen_of_shared_backend() {
        let backend: Arc<dyn Backend> = Arc::new(MemoryBackend::new());
        {
            let reg = StoreRegistry::new(backend.clone());
            reg.put(card("kimi")).await.expect("put");
            reg.put_policy(config().policy).await.expect("policy");
        }
        let reg = StoreRegistry::new(backend);
        assert_eq!(reg.get("kimi").await.expect("get"), card("kimi"));
        assert_eq!(reg.get_policy().await.expect("policy"), config().policy);
    }

    // desc: an unknown id is `not found` (the wire NotFound contract).
    #[tokio::test]
    async fn negative_get_unknown_is_not_found() {
        let reg = store();
        let err = reg.get("ghost").await.expect_err("unknown");
        assert!(err.to_string().contains("not found"), "{err}");
    }

    // desc: a rejected policy leaves the previous one in place.
    #[tokio::test]
    async fn negative_bad_policy_rejected_and_previous_kept() {
        let reg = seeded().await;
        let mut bad = RoutePolicySpec::default();
        bad.default_prefer.policy = "ask-a-magic-8-ball".into();
        assert!(reg.put_policy(bad).await.is_err());
        assert_eq!(reg.get_policy().await.unwrap(), config().policy);
    }

    // corner: an empty store routes to no candidate and lists nothing.
    #[tokio::test]
    async fn corner_empty_store_routes_to_no_candidate() {
        let reg = store();
        let d = reg.route(&RouteHint::default()).await.unwrap();
        assert_eq!(d.chosen, "");
        assert!(d.order.is_empty());
        assert!(reg.list().await.unwrap().is_empty());
        // A default policy is persisted as absence, so it reads back as default.
        assert_eq!(reg.get_policy().await.unwrap(), RoutePolicySpec::default());
    }

    // corner: setting then clearing the policy back to default leaves no card,
    // and the default reads back (exercises the empty-blob→absence mapping).
    #[tokio::test]
    async fn corner_policy_reset_to_default_reads_back_default() {
        let reg = seeded().await;
        assert_ne!(reg.get_policy().await.unwrap(), RoutePolicySpec::default());
        reg.put_policy(RoutePolicySpec::default())
            .await
            .expect("reset");
        assert_eq!(reg.get_policy().await.unwrap(), RoutePolicySpec::default());
        // Upstreams are untouched by a policy reset.
        assert_eq!(reg.list().await.unwrap().len(), 2);
    }

    // boundary: a full registry rejects an insert but still allows an update.
    #[tokio::test]
    async fn boundary_registry_full_rejects_insert_but_allows_update() {
        let reg = store();
        for i in 0..MAX_REGISTRY_UPSTREAMS {
            reg.put(card(&format!("u{i}"))).await.expect("fits");
        }
        assert!(reg.put(card("one-too-many")).await.is_err());
        let mut upd = card("u0");
        upd.model = "m2".into();
        assert_eq!(reg.put(upd).await.expect("update").model, "m2");
    }

    // adversarial: every hostile id is rejected at every entry point, and no
    // rejected call mutates the store.
    #[rstest]
    #[case::traversal("../../etc/passwd")]
    #[case::separator("a/b")]
    #[case::leading_dash("-rf")]
    #[case::empty("")]
    #[case::overlong(&"x".repeat(300))]
    #[tokio::test]
    async fn adversarial_hostile_ids_rejected_everywhere(#[case] id: &str) {
        let reg = seeded().await;
        assert!(reg.get(id).await.is_err(), "get {id:?}");
        assert!(reg.delete(id).await.is_err(), "delete {id:?}");
        assert!(reg.enable(id, true).await.is_err(), "enable {id:?}");
        let mut bad = card("ok");
        bad.id = id.into();
        assert!(reg.put(bad).await.is_err(), "put {id:?}");
        assert_eq!(reg.list().await.unwrap().len(), 2, "unchanged after {id:?}");
    }

    // adversarial: hostile numbers are clamped before the card is stored.
    #[tokio::test]
    async fn adversarial_put_clamps_hostile_numbers_before_storing() {
        let reg = store();
        let mut evil = card("evil");
        evil.input_cost = f32::NAN;
        evil.weight = f32::INFINITY;
        evil.context_window = u32::MAX;
        let stored = reg.put(evil).await.expect("stored, clamped");
        assert_eq!(stored.input_cost, 0.0);
        assert_eq!(stored.weight, 0.0);
        assert_eq!(stored.context_window, agent_core::MAX_ROUTE_MIN_CONTEXT);
        // Re-read decodes the same clamped values.
        assert_eq!(
            reg.get("evil").await.unwrap().context_window,
            agent_core::MAX_ROUTE_MIN_CONTEXT
        );
    }

    // adversarial: a card holding a raw secret (not a `*_ref`) is rejected, and
    // the error never echoes the secret.
    #[tokio::test]
    async fn adversarial_raw_secret_in_api_key_ref_rejected() {
        let reg = store();
        let mut bad = card("x");
        bad.api_key_ref = "sk-live-secret".into();
        let err = reg.put(bad).await.expect_err("raw key rejected");
        assert!(!err.to_string().contains("sk-live"), "no echo: {err}");
    }

    // adversarial: a blob tampered out of band to decode to a traversal id fails
    // closed on the next read (decode → validate rejects it).
    #[tokio::test]
    async fn adversarial_out_of_band_row_tamper_fails_closed() {
        let backend: Arc<dyn Backend> = Arc::new(MemoryBackend::new());
        let reg = StoreRegistry::new(backend.clone());
        reg.put(card("kimi")).await.expect("seed");
        // Overwrite the stored card with a blob decoding to a traversal id.
        let evil = pb::Upstream {
            id: "../escape".into(),
            ..Default::default()
        }
        .encode_to_vec();
        backend
            .apply(&[
                Write::EnsureTenant {
                    tenant: DEFAULT_TENANT.to_string(),
                },
                Write::Put {
                    collection: UPSTREAMS,
                    tenant: DEFAULT_TENANT.to_string(),
                    id: "kimi".to_string(),
                    blob: evil,
                },
            ])
            .await
            .expect("tamper");
        assert!(reg.list().await.is_err(), "tampered row must fail closed");
    }

    // adversarial: an upstream named `task-router` (the router itself) is refused.
    #[tokio::test]
    async fn adversarial_reserved_task_router_name_rejected() {
        let reg = store();
        let mut bad = card("task-router");
        bad.tier = Some(PoolTier::Medium);
        assert!(
            reg.put(bad).await.is_err(),
            "reserved name must be rejected"
        );
    }
}

// The Postgres arm exercised against a REAL server — the tier `nix flake check`
// cannot host (no DB in the sandbox). `#[ignore]`-gated and run single-threaded
// by the `pg-integration` harness, which sets `AGENT_CONFIG_STORE_TEST_DSN`. A
// dedicated tenant keeps the run isolated without a global TRUNCATE.
#[cfg(all(test, feature = "registry-store-postgres"))]
mod pg_tests {
    use super::*;
    use crate::testdata::{card, config};
    use crate::MemoryRegistry;
    use agent_config_store::PgBackend;
    use agent_core::RouteRole;

    const IT_TENANT: &str = "a3_registry_it";

    async fn pg_registry() -> StoreRegistry {
        let dsn = std::env::var("AGENT_CONFIG_STORE_TEST_DSN")
            .expect("AGENT_CONFIG_STORE_TEST_DSN must be set by the pg-integration harness");
        // Eager connect + ensure the shared config-store schema.
        let backend = PgBackend::connect(&dsn, 4, true)
            .await
            .expect("connect postgres + ensure schema");
        let reg = StoreRegistry::with_tenant(Arc::new(backend), IT_TENANT).expect("tenant");
        // Clean slate for this tenant (idempotent across re-runs): drop every
        // upstream and reset the policy to the default (persisted as absence).
        for u in reg.list().await.expect("list") {
            reg.delete(&u.id).await.expect("cleanup delete");
        }
        reg.put_policy(RoutePolicySpec::default())
            .await
            .expect("cleanup policy");
        reg
    }

    // desc (postgres, live): the full CRUD + routing matrix roundtrips over a real
    // server and agrees with the in-memory backend — the behaviour-preservation
    // proof for the converged postgres tier.
    #[tokio::test]
    #[ignore = "needs a running postgres (nix run .#postgres-up) — run via `nix run .#integration`"]
    async fn positive_pg_crud_and_route_agree_with_memory() {
        let reg = pg_registry().await;
        for u in config().upstreams {
            reg.put(u).await.expect("put");
        }
        reg.put_policy(config().policy).await.expect("policy");

        let mem = MemoryRegistry::new(config()).expect("valid");
        let hint = RouteHint {
            role: Some(RouteRole::Judge),
            ..Default::default()
        };
        assert_eq!(
            reg.route(&hint).await.unwrap(),
            mem.route(&hint).await.unwrap(),
            "route must match memory over postgres"
        );
        assert_eq!(reg.list().await.unwrap(), mem.list().await.unwrap());
        assert_eq!(reg.get_policy().await.unwrap(), config().policy);

        // Enable/disable + delete land durably.
        assert!(!reg.enable("kimi", false).await.unwrap().enabled);
        assert_eq!(reg.route(&hint).await.unwrap().chosen, "glm");
        assert!(reg.delete("kimi").await.unwrap());
        assert_eq!(reg.list().await.unwrap().len(), 1);
    }

    // adversarial (postgres, live): a hostile id never mutates the store, and the
    // reserved `task-router` name is refused, over the real server.
    #[tokio::test]
    #[ignore = "needs a running postgres (nix run .#postgres-up) — run via `nix run .#integration`"]
    async fn adversarial_pg_hostile_inputs_rejected() {
        let reg = pg_registry().await;
        reg.put(card("kimi")).await.expect("seed");
        for id in ["../../etc/passwd", "a/b", ""] {
            assert!(reg.get(id).await.is_err(), "get {id:?}");
            assert!(reg.delete(id).await.is_err(), "delete {id:?}");
        }
        let mut reserved = card("task-router");
        reserved.tier = Some(agent_core::PoolTier::Medium);
        assert!(reg.put(reserved).await.is_err());
        assert_eq!(reg.list().await.unwrap().len(), 1, "store unchanged");
    }
}
