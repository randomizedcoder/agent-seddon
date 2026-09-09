//! Per-tenant routing for the shared-store control-plane seams (config design
//! C35, increment C2, docs/design/config/03-per-tenant-config.md).
//!
//! The A3* convergence put `ProviderRegistry`, `FleetRegistry`, and `PromptStore`
//! on one transactional store keyed by `(collection, tenant, id)`, but the builder
//! wires a single `local`-tenant view shared across every caller. [`PerTenant<S>`]
//! is the routing layer that makes those seams *actually* multi-tenant: it resolves
//! the caller's **verified tenant** on each call and delegates to a per-tenant view
//! of the store, built lazily and cached — the control-plane twin of
//! [`PerUserMemory`](agent_memory) for the file stores.
//!
//! **The routing key is the verified identity, not a request field.** B1's auth
//! layer overwrites the identity with the token's verified tenant
//! (`server/auth.rs`), so `current_identity().user` *is* the verified tenant under
//! `oidc`, and is the trusted `local` fallback otherwise. A caller cannot name
//! another tenant — there is no tenant argument on the seam methods, and the store
//! view is chosen entirely from the ambient identity.
//!
//! **Fail closed to the default tenant.** A missing identity, or one whose segment
//! is not [`safe_segment`]-valid (only reachable under `mode = "none"`, where the
//! header is trusted-as-sent), routes to `local` — never to `..`/separators or
//! another tenant's view. `local` maps to the store's own un-namespaced base, so
//! `per_tenant = false` and the single-tenant CLI stay byte-identical.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use agent_core::{current_identity, safe_segment, UserId};

/// The current turn's verified tenant segment, or `local` when no identity is
/// scoped or the scoped segment is not path-safe (fail-closed to the default
/// tenant — never another tenant's view, never an escape).
fn current_tenant() -> String {
    match current_identity() {
        Some(k) if safe_segment(k.user.as_str()) => k.user.as_str().to_string(),
        _ => UserId::LOCAL.to_string(),
    }
}

/// The per-tenant view builder: maps a (safe) tenant string to that tenant's store
/// view. Boxed so [`PerTenant`] is object-safe over any seam trait.
type TenantBuilder<S> = dyn Fn(&str) -> Arc<S> + Send + Sync;

/// Routes each seam call to the caller's per-tenant view of a shared store, built
/// lazily by `build` and cached per tenant. Generic over the seam trait object
/// (`S = dyn ProviderRegistry`, etc.); cheap per-tenant views (an `Arc` handle plus
/// a tenant `String`) make the lazy cache nearly free.
pub struct PerTenant<S: ?Sized> {
    build: Arc<TenantBuilder<S>>,
    cache: Mutex<HashMap<String, Arc<S>>>,
}

impl<S: ?Sized> PerTenant<S> {
    /// Wrap a per-tenant builder. `build(tenant)` must return that tenant's view of
    /// the store; it is called at most once per tenant (results are cached). The
    /// builder receives only `safe_segment`-valid tenant strings (`local` for the
    /// default), so a fallible `with_tenant` cannot fail on the segment — a builder
    /// that still wants to be defensive should fall back to the base view rather than
    /// panic.
    pub fn new(build: impl Fn(&str) -> Arc<S> + Send + Sync + 'static) -> Self {
        Self {
            build: Arc::new(build),
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// The store view for the current turn's tenant. The cache lock is released
    /// before the returned `Arc` is used, so distinct tenants never contend on the
    /// seam call itself (only on the brief build/lookup).
    pub fn route(&self) -> Arc<S> {
        let tenant = current_tenant();
        let mut cache = self.cache.lock().expect("per-tenant cache poisoned");
        if let Some(s) = cache.get(&tenant) {
            return s.clone();
        }
        let s = (self.build)(&tenant);
        cache.insert(tenant, s.clone());
        s
    }
}

#[async_trait::async_trait]
impl agent_core::ProviderRegistry for PerTenant<dyn agent_core::ProviderRegistry> {
    async fn list(&self) -> agent_core::Result<Vec<agent_core::Upstream>> {
        self.route().list().await
    }
    async fn get(&self, id: &str) -> agent_core::Result<agent_core::Upstream> {
        self.route().get(id).await
    }
    async fn put(&self, card: agent_core::Upstream) -> agent_core::Result<agent_core::Upstream> {
        self.route().put(card).await
    }
    async fn delete(&self, id: &str) -> agent_core::Result<bool> {
        self.route().delete(id).await
    }
    async fn enable(&self, id: &str, enabled: bool) -> agent_core::Result<agent_core::Upstream> {
        self.route().enable(id, enabled).await
    }
    async fn get_policy(&self) -> agent_core::Result<agent_core::RoutePolicySpec> {
        self.route().get_policy().await
    }
    async fn put_policy(
        &self,
        policy: agent_core::RoutePolicySpec,
    ) -> agent_core::Result<agent_core::RoutePolicySpec> {
        self.route().put_policy(policy).await
    }
    async fn route(
        &self,
        hint: &agent_core::RouteHint,
    ) -> agent_core::Result<agent_core::RouteDecision> {
        PerTenant::route(self).route(hint).await
    }
    async fn health(&self) -> agent_core::Result<Vec<agent_core::UpstreamHealth>> {
        self.route().health().await
    }
}

#[async_trait::async_trait]
impl agent_core::FleetRegistry for PerTenant<dyn agent_core::FleetRegistry> {
    async fn list(&self) -> agent_core::Result<Vec<agent_core::FleetSession>> {
        self.route().list().await
    }
    async fn get(&self, id: &str) -> agent_core::Result<agent_core::FleetSession> {
        self.route().get(id).await
    }
    async fn put(
        &self,
        session: agent_core::FleetSession,
    ) -> agent_core::Result<agent_core::FleetSession> {
        self.route().put(session).await
    }
    async fn delete(&self, id: &str) -> agent_core::Result<bool> {
        self.route().delete(id).await
    }
    async fn set_enabled(
        &self,
        id: &str,
        enabled: bool,
    ) -> agent_core::Result<agent_core::FleetSession> {
        self.route().set_enabled(id, enabled).await
    }
}

#[async_trait::async_trait]
impl agent_core::PromptStore for PerTenant<dyn agent_core::PromptStore> {
    async fn list(
        &self,
        kind: Option<agent_core::PromptKind>,
    ) -> agent_core::Result<Vec<agent_core::PromptEntry>> {
        self.route().list(kind).await
    }
    async fn get(&self, r: &agent_core::PromptRef) -> agent_core::Result<agent_core::PromptEntry> {
        self.route().get(r).await
    }
    async fn put(
        &self,
        entry: agent_core::PromptEntry,
    ) -> agent_core::Result<agent_core::PromptEntry> {
        self.route().put(entry).await
    }
    async fn delete(&self, r: &agent_core::PromptRef) -> agent_core::Result<bool> {
        self.route().delete(r).await
    }
    async fn select(
        &self,
        ctx: &agent_core::PromptContext,
    ) -> agent_core::Result<Vec<agent_core::PromptEntry>> {
        self.route().select(ctx).await
    }
    async fn preview_assembled(
        &self,
        ctx: &agent_core::PromptContext,
        goal: &str,
    ) -> agent_core::Result<Vec<agent_core::Message>> {
        self.route().preview_assembled(ctx, goal).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{scope, ProviderRegistry, Result, RoutePolicySpec, SessionKey, Upstream};
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A minimal `ProviderRegistry` that records, on each `list`, the tenant it was
    /// built for — so a test can see which per-tenant view a call was routed to.
    struct FakeReg {
        tenant: String,
        calls: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait::async_trait]
    impl ProviderRegistry for FakeReg {
        async fn list(&self) -> Result<Vec<Upstream>> {
            self.calls.lock().unwrap().push(self.tenant.clone());
            Ok(vec![])
        }
        async fn get(&self, _id: &str) -> Result<Upstream> {
            Ok(Upstream::default())
        }
        async fn put(&self, card: Upstream) -> Result<Upstream> {
            Ok(card)
        }
        async fn delete(&self, _id: &str) -> Result<bool> {
            Ok(false)
        }
        async fn enable(&self, _id: &str, _enabled: bool) -> Result<Upstream> {
            Ok(Upstream::default())
        }
        async fn get_policy(&self) -> Result<RoutePolicySpec> {
            Ok(RoutePolicySpec::default())
        }
        async fn put_policy(&self, policy: RoutePolicySpec) -> Result<RoutePolicySpec> {
            Ok(policy)
        }
        async fn route(&self, _hint: &agent_core::RouteHint) -> Result<agent_core::RouteDecision> {
            Ok(agent_core::RouteDecision::default())
        }
        async fn health(&self) -> Result<Vec<agent_core::UpstreamHealth>> {
            Ok(vec![])
        }
    }

    /// A `PerTenant` over `FakeReg`, the shared call log, and a build counter.
    type Harness = (
        PerTenant<dyn ProviderRegistry>,
        Arc<Mutex<Vec<String>>>,
        Arc<AtomicUsize>,
    );

    fn per_tenant() -> Harness {
        let calls: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let builds = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let b = builds.clone();
        let pt = PerTenant::new(move |tenant: &str| {
            b.fetch_add(1, Ordering::SeqCst);
            Arc::new(FakeReg {
                tenant: tenant.to_string(),
                calls: c.clone(),
            }) as Arc<dyn ProviderRegistry>
        });
        (pt, calls, builds)
    }

    async fn list_as(pt: &PerTenant<dyn ProviderRegistry>, user: &str, session: &str) {
        let key = SessionKey::parse(user, session).unwrap();
        scope(key, async { pt.list().await.unwrap() }).await;
    }

    // desc: two verified tenants route to distinct store views → expect each call
    // recorded under its own tenant, no bleed.
    #[tokio::test]
    async fn positive_two_tenants_route_to_distinct_views() {
        let (pt, calls, _) = per_tenant();
        list_as(&pt, "acme", "s1").await;
        list_as(&pt, "globex", "s1").await;
        let log = calls.lock().unwrap().clone();
        assert_eq!(log, vec!["acme".to_string(), "globex".to_string()]);
    }

    // desc: a tenant's view is built once and reused across calls → expect one build
    // for repeated calls by the same tenant, two calls recorded.
    #[tokio::test]
    async fn positive_cached_view_reused_across_calls() {
        let (pt, calls, builds) = per_tenant();
        list_as(&pt, "acme", "s1").await;
        list_as(&pt, "acme", "s2").await; // same tenant, different session
        assert_eq!(builds.load(Ordering::SeqCst), 1, "one build per tenant");
        assert_eq!(calls.lock().unwrap().len(), 2, "both calls routed");
    }

    // boundary: the default `local` tenant routes to the un-namespaced base view →
    // expect the build to receive exactly `local`.
    #[tokio::test]
    async fn boundary_local_tenant_uses_base_view() {
        let (pt, calls, _) = per_tenant();
        scope(SessionKey::local("s1"), async { pt.list().await.unwrap() }).await;
        assert_eq!(calls.lock().unwrap().clone(), vec!["local".to_string()]);
    }

    // corner: no ambient identity scoped ⇒ route to the default tenant → expect
    // `local`.
    #[tokio::test]
    async fn corner_no_identity_defaults_to_local() {
        let (pt, calls, _) = per_tenant();
        pt.list().await.unwrap();
        assert_eq!(calls.lock().unwrap().clone(), vec!["local".to_string()]);
    }

    // adversarial: a hostile identity segment (traversal/separator) never becomes a
    // tenant key — it fails closed to `local`, never `..`/another tenant's view.
    #[rstest::rstest]
    #[case::traversal("../../etc")]
    #[case::separator("a/b")]
    #[case::dotdot("..")]
    #[tokio::test]
    async fn adversarial_hostile_identity_falls_back_to_local(#[case] bad: &str) {
        let (pt, calls, _) = per_tenant();
        // `SessionKey::parse` itself rejects most hostile users; drive the router's
        // own fallback directly for the ones that would slip a raw header through.
        let routed = {
            // Build under a manually-forced identity via the same task-local the auth
            // layer uses; if parse rejects it, we still assert the no-identity path.
            match SessionKey::parse(bad, "s1") {
                Ok(key) => {
                    scope(key, async { pt.list().await.unwrap() }).await;
                    calls.lock().unwrap().clone()
                }
                Err(_) => {
                    // A rejected key never scopes an identity ⇒ default tenant.
                    pt.list().await.unwrap();
                    calls.lock().unwrap().clone()
                }
            }
        };
        for t in &routed {
            assert_eq!(
                t, "local",
                "hostile id {bad:?} must route to local, got {t}"
            );
            assert!(!t.contains(bad));
        }
    }

    // End-to-end over the REAL converged stores on one shared MemoryBackend: proves
    // PerTenant + the store's `(collection, tenant, id)` keying actually isolate
    // tenants, and exercises the new `StorePrompt::with_tenant` (C38). Feature-gated
    // (the store deps are off by default); run by `nix/checks/per-tenant.nix`.
    #[cfg(all(feature = "registry-store", feature = "prompt-store"))]
    mod real_store {
        use crate::tenant::PerTenant;
        use agent_config_store::{Backend, MemoryBackend};
        use agent_core::{
            scope, PoolTier, PromptEntry, PromptKind, PromptRef, PromptStore, ProviderRegistry,
            SessionKey, Upstream,
        };
        use std::sync::Arc;

        fn upstream(id: &str) -> Upstream {
            Upstream {
                id: id.into(),
                kind: "openai-compat".into(),
                enabled: true,
                base_url: format!("http://127.0.0.1:1/{id}"),
                model: "m".into(),
                api_key_ref: "env:TEST_KEY".into(),
                context_window: 128_000,
                supports_tools: true,
                tier: Some(PoolTier::Medium),
                ..Default::default()
            }
        }

        // desc: a card written under tenant `acme` is invisible to `globex` → expect
        // acme sees exactly its card, globex sees none.
        #[tokio::test]
        async fn positive_two_tenants_isolated_stores() {
            let backend: Arc<dyn Backend> = Arc::new(MemoryBackend::new());
            let b = backend.clone();
            let reg = PerTenant::new(move |t| {
                agent_registry::StoreRegistry::with_tenant(b.clone(), t)
                    .map(|s| Arc::new(s) as Arc<dyn ProviderRegistry>)
                    .unwrap_or_else(|_| Arc::new(agent_registry::StoreRegistry::new(b.clone())))
            });
            scope(SessionKey::parse("acme", "s1").unwrap(), async {
                reg.put(upstream("kimi")).await.unwrap();
            })
            .await;
            let globex = scope(SessionKey::parse("globex", "s1").unwrap(), async {
                reg.list().await.unwrap()
            })
            .await;
            let acme = scope(SessionKey::parse("acme", "s1").unwrap(), async {
                reg.list().await.unwrap()
            })
            .await;
            assert!(globex.is_empty(), "globex must not see acme's card");
            assert_eq!(acme.len(), 1);
            assert_eq!(acme[0].id, "kimi");
        }

        // desc: StorePrompt::with_tenant isolates prompt overrides per tenant → expect
        // acme's System override present, globex's System still the builtin default.
        #[tokio::test]
        async fn positive_two_tenants_isolated_prompt_dbs() {
            let backend: Arc<dyn Backend> = Arc::new(MemoryBackend::new());
            let b = backend.clone();
            let prompts = PerTenant::new(move |t| {
                agent_prompt::StorePrompt::with_tenant(b.clone(), "DEFAULT-SYS", t)
                    .map(|s| Arc::new(s) as Arc<dyn PromptStore>)
                    .unwrap_or_else(|_| {
                        Arc::new(agent_prompt::StorePrompt::new(b.clone(), "DEFAULT-SYS"))
                    })
            });
            scope(SessionKey::parse("acme", "s1").unwrap(), async {
                prompts
                    .put(PromptEntry {
                        kind: PromptKind::System,
                        id: String::new(),
                        content: "ACME-SYS".into(),
                        builtin: false,
                        read_only: false,
                        order: 0,
                        tags: vec![],
                    })
                    .await
                    .unwrap();
            })
            .await;
            let r = PromptRef {
                kind: PromptKind::System,
                id: String::new(),
            };
            let acme_sys = scope(SessionKey::parse("acme", "s1").unwrap(), async {
                prompts.get(&r).await.unwrap()
            })
            .await;
            let globex_sys = scope(SessionKey::parse("globex", "s1").unwrap(), async {
                prompts.get(&r).await.unwrap()
            })
            .await;
            assert_eq!(acme_sys.content, "ACME-SYS");
            assert_eq!(
                globex_sys.content, "DEFAULT-SYS",
                "globex sees the default, not acme's override"
            );
            assert!(globex_sys.builtin, "globex's system is still the default");
        }
    }
}

// Per-tenant isolation over a REAL Postgres server — the `(collection, tenant, id)`
// keying that makes `PerTenant` isolate, proven end to end over the tier `nix flake
// check` cannot host. `#[ignore]`-gated and run single-threaded by the
// `pg-integration` harness (sets `AGENT_CONFIG_STORE_TEST_DSN`); dedicated tenants
// keep the run isolated.
#[cfg(all(test, feature = "registry-postgres"))]
mod pg_tenant_tests {
    use super::PerTenant;
    use agent_config_store::{Backend, PgBackend};
    use agent_core::{scope, ProviderRegistry, SessionKey, Upstream};
    use std::sync::Arc;

    fn upstream(id: &str) -> Upstream {
        Upstream {
            id: id.into(),
            kind: "openai-compat".into(),
            enabled: true,
            base_url: format!("http://127.0.0.1:1/{id}"),
            model: "m".into(),
            api_key_ref: "env:TEST_KEY".into(),
            context_window: 128_000,
            supports_tools: true,
            tier: Some(agent_core::PoolTier::Medium),
            ..Default::default()
        }
    }

    // desc (postgres, live): a card `Put` under one verified tenant is invisible to
    // another's `list`, routed entirely by identity through `PerTenant` over a real
    // server — the multi-tenant isolation proof for the wired postgres arm.
    #[tokio::test]
    #[ignore = "needs a running postgres (nix run .#postgres-up) — run via `nix run .#integration`"]
    async fn positive_pg_two_tenants_isolated() {
        let dsn = std::env::var("AGENT_CONFIG_STORE_TEST_DSN")
            .expect("AGENT_CONFIG_STORE_TEST_DSN must be set by the pg-integration harness");
        let backend: Arc<dyn Backend> = Arc::new(
            PgBackend::connect(&dsn, 4, true)
                .await
                .expect("connect postgres + ensure schema"),
        );
        // Dedicated tenants for this run; clean slate (idempotent across re-runs).
        const A: &str = "c2_tenant_it_a";
        const B: &str = "c2_tenant_it_b";
        for t in [A, B] {
            let s = agent_registry::StoreRegistry::with_tenant(backend.clone(), t).expect("tenant");
            for u in s.list().await.expect("list") {
                s.delete(&u.id).await.expect("cleanup");
            }
        }
        let b = backend.clone();
        let reg = PerTenant::new(move |t| {
            agent_registry::StoreRegistry::with_tenant(b.clone(), t)
                .map(|s| Arc::new(s) as Arc<dyn ProviderRegistry>)
                .unwrap_or_else(|_| Arc::new(agent_registry::StoreRegistry::new(b.clone())))
        });
        scope(SessionKey::parse(A, "s1").unwrap(), async {
            reg.put(upstream("kimi")).await.unwrap();
        })
        .await;
        let seen_b = scope(SessionKey::parse(B, "s1").unwrap(), async {
            reg.list().await.unwrap()
        })
        .await;
        let seen_a = scope(SessionKey::parse(A, "s1").unwrap(), async {
            reg.list().await.unwrap()
        })
        .await;
        assert!(seen_b.is_empty(), "tenant B must not see tenant A's card");
        assert_eq!(seen_a.len(), 1);
        assert_eq!(seen_a[0].id, "kimi");
        // Cleanup.
        scope(SessionKey::parse(A, "s1").unwrap(), async {
            reg.delete("kimi").await.unwrap();
        })
        .await;
    }
}
