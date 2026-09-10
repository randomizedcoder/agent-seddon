//! `StoreForges` — the [`ForgeRegistry`] over a shared
//! [`agent_config_store::Backend`] (config design C36, increment D1).
//!
//! One card per forge (id = the forge name). Mirrors `StoreRoles`/`StoreRegistry`:
//! it persists the prost `pb::ForgeCard` blob directly (the orphan rule forbids a
//! `Card` impl on the foreign proto type). Single-tenant `local` until the
//! per-tenant plane (config C35 / increment C2) calls [`StoreForges::with_tenant`].

use std::sync::Arc;

use agent_config_store::{Backend, Write, DEFAULT_MAX_CARDS_PER_TENANT};
use agent_core::{safe_segment, Error, ForgeCard, ForgeRegistry, Result};
use agent_proto::pb;
use async_trait::async_trait;
use prost::Message;

/// The collection holding one card per forge (id = the forge name).
const FORGES: &str = "forges";
/// The default single-tenant scope (see [`StoreForges::with_tenant`]).
pub const DEFAULT_TENANT: &str = "local";

/// A [`ForgeRegistry`] persisted on a shared [`Backend`]. Cheap to clone (an `Arc`
/// handle plus the tenant key).
pub struct StoreForges {
    backend: Arc<dyn Backend>,
    tenant: String,
}

impl StoreForges {
    /// A forge store over `backend` under the default single-tenant scope.
    pub fn new(backend: Arc<dyn Backend>) -> Self {
        Self {
            backend,
            tenant: DEFAULT_TENANT.to_string(),
        }
    }

    /// A forge store scoped to an explicit tenant (config C2 will call this per
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
    /// decoded, carries an unknown `repo_encoding`, or fails structural validation
    /// is a fail-closed error (an out-of-band tamper), never a partial card.
    fn decode(blob: &[u8]) -> Result<ForgeCard> {
        let wire = pb::ForgeCard::decode(blob)
            .map_err(|e| Error::Config(format!("stored forge decode: {e}")))?;
        let card = ForgeCard::try_from(wire)
            .map_err(|e| Error::Config(format!("stored forge decode: {e}")))?;
        card.validate()?;
        Ok(card)
    }
}

#[async_trait]
impl ForgeRegistry for StoreForges {
    async fn list(&self) -> Result<Vec<ForgeCard>> {
        self.backend
            .list(FORGES, &self.tenant)
            .await?
            .iter()
            .map(|blob| Self::decode(blob))
            .collect()
    }

    async fn get(&self, id: &str) -> Result<ForgeCard> {
        if !safe_segment(id) {
            return Err(Error::Config(format!("invalid forge id `{id}`")));
        }
        match self.backend.get(FORGES, &self.tenant, id).await? {
            Some(blob) => Self::decode(&blob),
            // The `not found` prefix is the seam contract (the wire maps it to NotFound).
            None => Err(Error::Config(format!("not found: forge card `{id}`"))),
        }
    }

    async fn put(&self, mut card: ForgeCard) -> Result<ForgeCard> {
        // Clamp hostile numbers, then fail-closed structural validation before any
        // write (the id may become a storage key).
        card.sanitize();
        card.validate()?;
        let id = card.id.clone();
        let is_new = self.backend.get(FORGES, &self.tenant, &id).await?.is_none();
        if is_new && self.backend.count(FORGES, &self.tenant).await? >= DEFAULT_MAX_CARDS_PER_TENANT
        {
            return Err(Error::Config(format!(
                "forges is full ({DEFAULT_MAX_CARDS_PER_TENANT} cards for tenant `{}`)",
                self.tenant
            )));
        }
        let blob = pb::ForgeCard::from(card.clone()).encode_to_vec();
        self.backend
            .apply(&[
                Write::EnsureTenant {
                    tenant: self.tenant.clone(),
                },
                Write::Put {
                    collection: FORGES,
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
            return Err(Error::Config(format!("invalid forge id `{id}`")));
        }
        let existed = self.backend.get(FORGES, &self.tenant, id).await?.is_some();
        self.backend
            .apply(&[Write::Delete {
                collection: FORGES,
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
    use agent_core::RepoEncoding;
    use rstest::rstest;

    fn store() -> StoreForges {
        StoreForges::new(Arc::new(MemoryBackend::new()))
    }

    fn card(id: &str, kind: &str, enc: RepoEncoding) -> ForgeCard {
        ForgeCard {
            id: id.to_string(),
            kind: kind.to_string(),
            enabled: true,
            base_url: String::new(),
            token_ref: "env:TOK".to_string(),
            repo_encoding: enc,
            timeout_secs: 30,
            max_retries: 3,
        }
    }

    // desc: a put→get round-trips the stored card verbatim.
    #[tokio::test]
    async fn positive_put_get_roundtrip() {
        let s = store();
        let c = card("gh", "github", RepoEncoding::OwnerName);
        let stored = s.put(c.clone()).await.expect("put");
        assert_eq!(stored, c);
        assert_eq!(s.get("gh").await.expect("get"), c);
        assert_eq!(s.list().await.expect("list"), vec![c]);
    }

    // desc: put is an upsert — a second put with the same id replaces the card.
    #[tokio::test]
    async fn positive_put_upserts() {
        let s = store();
        s.put(card("gl", "gitlab", RepoEncoding::Path))
            .await
            .expect("put");
        let mut c2 = card("gl", "gitlab", RepoEncoding::Path);
        c2.base_url = "https://gitlab.example.com/api/v4".into();
        s.put(c2.clone()).await.expect("upsert");
        assert_eq!(s.get("gl").await.expect("get").base_url, c2.base_url);
        assert_eq!(s.list().await.expect("list").len(), 1);
    }

    // desc: get of an unknown id is a `not found` error (maps to NotFound on the wire).
    #[tokio::test]
    async fn negative_missing_forge() {
        let s = store();
        let err = s.get("ghost").await.expect_err("must be absent");
        assert!(err.to_string().contains("not found"), "got: {err}");
    }

    // desc: delete is idempotent — true when it existed, false when absent.
    #[tokio::test]
    async fn negative_delete_absent_is_false() {
        let s = store();
        assert!(!s.delete("ghost").await.expect("delete absent"));
        s.put(card("gh", "github", RepoEncoding::OwnerName))
            .await
            .expect("put");
        assert!(s.delete("gh").await.expect("delete present"));
        assert!(!s.delete("gh").await.expect("delete again"));
    }

    // boundary: put clamps a hostile timeout before persisting.
    #[tokio::test]
    async fn boundary_put_clamps_timeout() {
        let s = store();
        let mut c = card("gh", "github", RepoEncoding::OwnerName);
        c.timeout_secs = u32::MAX;
        let stored = s.put(c).await.expect("put");
        assert_eq!(stored.timeout_secs, 300);
    }

    // adversarial: a hostile id never mutates the store.
    #[rstest]
    #[case::traversal("../etc")]
    #[case::separator("a/b")]
    #[case::empty("")]
    #[tokio::test]
    async fn adversarial_hostile_id_rejected(#[case] id: &str) {
        let s = store();
        assert!(s
            .put(card(id, "github", RepoEncoding::OwnerName))
            .await
            .is_err());
    }

    // adversarial: a hostile tenant is rejected at construction.
    #[test]
    fn adversarial_hostile_tenant_rejected() {
        let b: Arc<dyn Backend> = Arc::new(MemoryBackend::new());
        assert!(StoreForges::with_tenant(b.clone(), "../etc").is_err());
        assert!(StoreForges::with_tenant(b, "acme").is_ok());
    }

    // adversarial: an unknown repo_encoding STRING never survives to the store — it
    // is rejected at the wire→core boundary.
    #[test]
    fn adversarial_unknown_repo_encoding_rejected() {
        let wire = pb::ForgeCard {
            id: "bad".into(),
            kind: "github".into(),
            enabled: true,
            base_url: String::new(),
            token_ref: "env:X".into(),
            repo_encoding: "subgroup".into(),
            timeout_secs: 30,
            max_retries: 3,
        };
        assert!(agent_core::ForgeCard::try_from(wire).is_err());
    }

    // adversarial: a decoded blob with a missing (empty) repo_encoding is rejected.
    #[test]
    fn adversarial_missing_repo_encoding_rejected() {
        let wire = pb::ForgeCard {
            id: "bad".into(),
            kind: "github".into(),
            enabled: true,
            base_url: String::new(),
            token_ref: "env:X".into(),
            repo_encoding: String::new(),
            timeout_secs: 30,
            max_retries: 3,
        };
        assert!(agent_core::ForgeCard::try_from(wire).is_err());
    }
}

// The Postgres arm exercised against a REAL server — the tier `nix flake check`
// cannot host (no DB in the sandbox). `#[ignore]`-gated and run single-threaded by
// the `pg-integration` harness, which sets `AGENT_CONFIG_STORE_TEST_DSN`. A
// dedicated tenant keeps the run isolated without a global TRUNCATE.
#[cfg(all(test, feature = "forge-store-postgres"))]
mod pg_tests {
    use super::*;
    use agent_config_store::PgBackend;
    use agent_core::RepoEncoding;

    const IT_TENANT: &str = "d1_forge_it";

    async fn pg_forges() -> StoreForges {
        let dsn = std::env::var("AGENT_CONFIG_STORE_TEST_DSN")
            .expect("AGENT_CONFIG_STORE_TEST_DSN must be set by the pg-integration harness");
        let backend = PgBackend::connect(&dsn, 4, true)
            .await
            .expect("connect postgres + ensure schema");
        let forges = StoreForges::with_tenant(Arc::new(backend), IT_TENANT).expect("tenant");
        for c in forges.list().await.expect("list") {
            forges.delete(&c.id).await.expect("cleanup delete");
        }
        forges
    }

    // desc (postgres, live): a card round-trips over a real server.
    #[tokio::test]
    #[ignore = "needs a running postgres (nix run .#postgres-up) — run via `nix run .#integration`"]
    async fn positive_pg_roundtrip() {
        let forges = pg_forges().await;
        let c = ForgeCard {
            id: "pg_gh".into(),
            kind: "github".into(),
            enabled: true,
            base_url: String::new(),
            token_ref: "env:GH".into(),
            repo_encoding: RepoEncoding::OwnerName,
            timeout_secs: 30,
            max_retries: 3,
        };
        forges.put(c.clone()).await.expect("put");
        assert_eq!(forges.get("pg_gh").await.expect("get"), c);
        assert!(forges.delete("pg_gh").await.expect("delete"));
    }

    // adversarial (postgres, live): a hostile id never mutates the store, over the
    // real server.
    #[tokio::test]
    #[ignore = "needs a running postgres (nix run .#postgres-up) — run via `nix run .#integration`"]
    async fn adversarial_pg_hostile_id_rejected() {
        let forges = pg_forges().await;
        for id in ["../../etc/passwd", "a/b", ""] {
            assert!(forges.get(id).await.is_err(), "get {id:?}");
            assert!(forges.delete(id).await.is_err(), "delete {id:?}");
        }
    }
}
