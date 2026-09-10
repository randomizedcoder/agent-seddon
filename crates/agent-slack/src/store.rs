//! `StoreTransports` — the [`TransportRegistry`] over a shared
//! [`agent_config_store::Backend`] (config design C37, increment D2).
//!
//! One card per transport (id = the transport name). Mirrors `StoreForges`: it
//! persists the prost `pb::TransportCard` blob directly (the orphan rule forbids a
//! `Card` impl on the foreign proto type). Single-tenant `local` until the per-tenant
//! plane (config C35 / increment C2) calls [`StoreTransports::with_tenant`].

use std::sync::Arc;

use agent_config_store::{Backend, Write, DEFAULT_MAX_CARDS_PER_TENANT};
use agent_core::{safe_segment, Error, Result, TransportCard, TransportRegistry};
use agent_proto::pb;
use async_trait::async_trait;
use prost::Message;

/// The collection holding one card per transport (id = the transport name).
const TRANSPORTS: &str = "transports";
/// The default single-tenant scope (see [`StoreTransports::with_tenant`]).
pub const DEFAULT_TENANT: &str = "local";

/// A [`TransportRegistry`] persisted on a shared [`Backend`]. Cheap to clone (an
/// `Arc` handle plus the tenant key).
pub struct StoreTransports {
    backend: Arc<dyn Backend>,
    tenant: String,
}

impl StoreTransports {
    /// A transport store over `backend` under the default single-tenant scope.
    pub fn new(backend: Arc<dyn Backend>) -> Self {
        Self {
            backend,
            tenant: DEFAULT_TENANT.to_string(),
        }
    }

    /// A transport store scoped to an explicit tenant (config C2 will call this per
    /// verified identity). The tenant is `safe_segment`-gated — a hostile tenant is
    /// rejected at construction, never persisted or turned into a key.
    pub fn with_tenant(backend: Arc<dyn Backend>, tenant: &str) -> Result<Self> {
        if !safe_segment(tenant) {
            return Err(Error::Config(format!("invalid tenant `{tenant}`")));
        }
        Ok(Self {
            backend,
            tenant: tenant.to_string(),
        })
    }

    /// Decode a stored blob into a validated core card. A blob that cannot be
    /// decoded, carries an unknown channel `purpose`, or fails structural validation
    /// is a fail-closed error (an out-of-band tamper), never a partial card.
    fn decode(blob: &[u8]) -> Result<TransportCard> {
        let wire = pb::TransportCard::decode(blob)
            .map_err(|e| Error::Config(format!("stored transport decode: {e}")))?;
        let card = TransportCard::try_from(wire)
            .map_err(|e| Error::Config(format!("stored transport decode: {e}")))?;
        card.validate()?;
        Ok(card)
    }
}

#[async_trait]
impl TransportRegistry for StoreTransports {
    async fn list(&self) -> Result<Vec<TransportCard>> {
        self.backend
            .list(TRANSPORTS, &self.tenant)
            .await?
            .iter()
            .map(|blob| Self::decode(blob))
            .collect()
    }

    async fn get(&self, id: &str) -> Result<TransportCard> {
        if !safe_segment(id) {
            return Err(Error::Config(format!("invalid transport id `{id}`")));
        }
        match self.backend.get(TRANSPORTS, &self.tenant, id).await? {
            Some(blob) => Self::decode(&blob),
            // The `not found` prefix is the seam contract (the wire maps it to NotFound).
            None => Err(Error::Config(format!("not found: transport card `{id}`"))),
        }
    }

    async fn put(&self, mut card: TransportCard) -> Result<TransportCard> {
        // Clamp hostile numbers, then fail-closed structural validation before any
        // write (the id may become a storage key).
        card.sanitize();
        card.validate()?;
        let id = card.id.clone();
        let is_new = self
            .backend
            .get(TRANSPORTS, &self.tenant, &id)
            .await?
            .is_none();
        if is_new
            && self.backend.count(TRANSPORTS, &self.tenant).await? >= DEFAULT_MAX_CARDS_PER_TENANT
        {
            return Err(Error::Config(format!(
                "transports is full ({DEFAULT_MAX_CARDS_PER_TENANT} cards for tenant `{}`)",
                self.tenant
            )));
        }
        let blob = pb::TransportCard::from(card.clone()).encode_to_vec();
        self.backend
            .apply(&[
                Write::EnsureTenant {
                    tenant: self.tenant.clone(),
                },
                Write::Put {
                    collection: TRANSPORTS,
                    tenant: self.tenant.clone(),
                    id,
                    blob,
                },
            ])
            .await?;
        Ok(card)
    }

    async fn delete(&self, id: &str) -> Result<bool> {
        if !safe_segment(id) {
            return Err(Error::Config(format!("invalid transport id `{id}`")));
        }
        let existed = self
            .backend
            .get(TRANSPORTS, &self.tenant, id)
            .await?
            .is_some();
        self.backend
            .apply(&[Write::Delete {
                collection: TRANSPORTS,
                tenant: self.tenant.clone(),
                id: id.to_string(),
            }])
            .await?;
        Ok(existed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_config_store::MemoryBackend;
    use agent_core::{ChannelBinding, ChannelPurpose};
    use rstest::rstest;

    fn store() -> StoreTransports {
        StoreTransports::new(Arc::new(MemoryBackend::new()))
    }

    fn card(id: &str, kind: &str) -> TransportCard {
        TransportCard {
            id: id.to_string(),
            kind: kind.to_string(),
            enabled: true,
            endpoint: String::new(),
            app_token_ref: "env:APP".to_string(),
            bot_token_ref: "env:BOT".to_string(),
            channels: vec![ChannelBinding {
                channel: "C_TRIGGER".to_string(),
                purpose: ChannelPurpose::Trigger,
            }],
            rate_limit_per_min: 30,
        }
    }

    // desc: a put→get round-trips the stored card verbatim.
    #[tokio::test]
    async fn positive_put_get_roundtrip() {
        let s = store();
        let c = card("slk", "slack");
        let stored = s.put(c.clone()).await.expect("put");
        assert_eq!(stored, c);
        assert_eq!(s.get("slk").await.expect("get"), c);
        assert_eq!(s.list().await.expect("list"), vec![c]);
    }

    // desc: put is an upsert — a second put with the same id replaces the card.
    #[tokio::test]
    async fn positive_put_upserts() {
        let s = store();
        s.put(card("slk", "slack")).await.expect("put");
        let mut c2 = card("slk", "slack");
        c2.rate_limit_per_min = 5;
        s.put(c2.clone()).await.expect("upsert");
        assert_eq!(s.get("slk").await.expect("get").rate_limit_per_min, 5);
        assert_eq!(s.list().await.expect("list").len(), 1);
    }

    // desc: get of an unknown id is a `not found` error (maps to NotFound on the wire).
    #[tokio::test]
    async fn negative_missing_transport() {
        let s = store();
        let err = s.get("ghost").await.expect_err("must be absent");
        assert!(err.to_string().contains("not found"), "got: {err}");
    }

    // desc: delete is idempotent — true when it existed, false when absent.
    #[tokio::test]
    async fn negative_delete_absent_is_false() {
        let s = store();
        assert!(!s.delete("ghost").await.expect("delete absent"));
        s.put(card("slk", "slack")).await.expect("put");
        assert!(s.delete("slk").await.expect("delete present"));
        assert!(!s.delete("slk").await.expect("delete again"));
    }

    // boundary: put clamps a hostile rate limit before persisting.
    #[tokio::test]
    async fn boundary_put_clamps_rate_limit() {
        let s = store();
        let mut c = card("slk", "slack");
        c.rate_limit_per_min = u32::MAX;
        let stored = s.put(c).await.expect("put");
        assert_eq!(stored.rate_limit_per_min, 600);
    }

    // adversarial: a hostile id never mutates the store.
    #[rstest]
    #[case::traversal("../etc")]
    #[case::separator("a/b")]
    #[case::empty("")]
    #[tokio::test]
    async fn adversarial_hostile_id_rejected(#[case] id: &str) {
        let s = store();
        assert!(s.put(card(id, "slack")).await.is_err());
    }

    // adversarial: a hostile tenant is rejected at construction.
    #[test]
    fn adversarial_hostile_tenant_rejected() {
        let b: Arc<dyn Backend> = Arc::new(MemoryBackend::new());
        assert!(StoreTransports::with_tenant(b.clone(), "../etc").is_err());
        assert!(StoreTransports::with_tenant(b, "acme").is_ok());
    }

    // adversarial: an unknown channel purpose STRING never survives to the store — it
    // is rejected at the wire→core boundary.
    #[test]
    fn adversarial_unknown_purpose_rejected() {
        let wire = pb::TransportCard {
            id: "bad".into(),
            kind: "slack".into(),
            enabled: true,
            endpoint: String::new(),
            app_token_ref: "env:X".into(),
            bot_token_ref: "env:Y".into(),
            channels: vec![pb::ChannelBinding {
                channel: "C".into(),
                purpose: "broadcast".into(),
            }],
            rate_limit_per_min: 30,
        };
        assert!(agent_core::TransportCard::try_from(wire).is_err());
    }
}

// The Postgres arm exercised against a REAL server — the tier `nix flake check`
// cannot host (no DB in the sandbox). `#[ignore]`-gated and run single-threaded by
// the `pg-integration` harness, which sets `AGENT_CONFIG_STORE_TEST_DSN`. A dedicated
// tenant keeps the run isolated without a global TRUNCATE.
#[cfg(all(test, feature = "transport-store-postgres"))]
mod pg_tests {
    use super::*;
    use agent_config_store::PgBackend;
    use agent_core::{ChannelBinding, ChannelPurpose};

    const IT_TENANT: &str = "d2_transport_it";

    async fn pg_transports() -> StoreTransports {
        let dsn = std::env::var("AGENT_CONFIG_STORE_TEST_DSN")
            .expect("AGENT_CONFIG_STORE_TEST_DSN must be set by the pg-integration harness");
        let backend = PgBackend::connect(&dsn, 4, true)
            .await
            .expect("connect postgres + ensure schema");
        let transports =
            StoreTransports::with_tenant(Arc::new(backend), IT_TENANT).expect("tenant");
        for c in transports.list().await.expect("list") {
            transports.delete(&c.id).await.expect("cleanup delete");
        }
        transports
    }

    // desc (postgres, live): a card round-trips over a real server.
    #[tokio::test]
    #[ignore = "needs a running postgres (nix run .#postgres-up) — run via `nix run .#integration`"]
    async fn positive_pg_roundtrip() {
        let transports = pg_transports().await;
        let c = TransportCard {
            id: "pg_slk".into(),
            kind: "slack".into(),
            enabled: true,
            endpoint: String::new(),
            app_token_ref: "env:APP".into(),
            bot_token_ref: "env:BOT".into(),
            channels: vec![ChannelBinding {
                channel: "C".into(),
                purpose: ChannelPurpose::Trigger,
            }],
            rate_limit_per_min: 30,
        };
        transports.put(c.clone()).await.expect("put");
        assert_eq!(transports.get("pg_slk").await.expect("get"), c);
        assert!(transports.delete("pg_slk").await.expect("delete"));
    }

    // adversarial (postgres, live): a hostile id never mutates the store, over the
    // real server.
    #[tokio::test]
    #[ignore = "needs a running postgres (nix run .#postgres-up) — run via `nix run .#integration`"]
    async fn adversarial_pg_hostile_id_rejected() {
        let transports = pg_transports().await;
        for id in ["../../etc/passwd", "a/b", ""] {
            assert!(transports.get(id).await.is_err(), "get {id:?}");
            assert!(transports.delete(id).await.is_err(), "delete {id:?}");
        }
    }
}
