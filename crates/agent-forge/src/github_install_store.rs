//! `StoreInstalls` — the [`GithubInstallRegistry`] over a shared
//! [`agent_config_store::Backend`] (forge-auth F4).
//!
//! One card per GitHub App installation (id = the decimal `installation_id`).
//! Mirrors [`StoreForges`](crate::StoreForges): it persists the prost
//! `pb::GithubInstall` blob directly, and is single-tenant `local` until the
//! per-tenant plane calls [`StoreInstalls::with_tenant`]. Every id and tenant is
//! `safe_segment`/`valid_installation_id`-gated before it becomes a storage key, and
//! a stored blob is re-validated on load (fail-closed on an out-of-band tamper).

use std::sync::Arc;

use agent_config_store::{Backend, Write, DEFAULT_MAX_CARDS_PER_TENANT};
use agent_core::{safe_segment, Error, GithubInstall, GithubInstallRegistry, Result};
use agent_proto::pb;
use async_trait::async_trait;
use prost::Message;

/// The collection holding one card per installation (id = the installation id).
const INSTALLS: &str = "github_installs";
/// The default single-tenant scope (see [`StoreInstalls::with_tenant`]).
pub const DEFAULT_TENANT: &str = "local";

/// A [`GithubInstallRegistry`] persisted on a shared [`Backend`]. Cheap to clone (an
/// `Arc` handle plus the tenant key).
pub struct StoreInstalls {
    backend: Arc<dyn Backend>,
    tenant: String,
}

impl StoreInstalls {
    /// An install store over `backend` under the default single-tenant scope.
    pub fn new(backend: Arc<dyn Backend>) -> Self {
        Self {
            backend,
            tenant: DEFAULT_TENANT.to_string(),
        }
    }

    /// An install store scoped to an explicit tenant (the per-tenant plane calls
    /// this per verified identity). The tenant is `safe_segment`-gated — a hostile
    /// tenant is rejected at construction, never persisted or turned into a key.
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
    /// decoded or fails structural validation is a fail-closed error (an out-of-band
    /// tamper), never a partial card.
    fn decode(blob: &[u8]) -> Result<GithubInstall> {
        let wire = pb::GithubInstall::decode(blob)
            .map_err(|e| Error::Config(format!("stored github install decode: {e}")))?;
        let card = GithubInstall::from(wire);
        card.validate()?;
        Ok(card)
    }
}

#[async_trait]
impl GithubInstallRegistry for StoreInstalls {
    async fn list(&self) -> Result<Vec<GithubInstall>> {
        self.backend
            .list(INSTALLS, &self.tenant)
            .await?
            .iter()
            .map(|blob| Self::decode(blob))
            .collect()
    }

    async fn get(&self, installation_id: &str) -> Result<GithubInstall> {
        if !safe_segment(installation_id) {
            return Err(Error::Config(format!(
                "invalid installation id `{installation_id}`"
            )));
        }
        match self
            .backend
            .get(INSTALLS, &self.tenant, installation_id)
            .await?
        {
            Some(blob) => Self::decode(&blob),
            // The `not found` prefix is the seam contract (the wire maps it to NotFound).
            None => Err(Error::Config(format!(
                "not found: github install `{installation_id}`"
            ))),
        }
    }

    async fn put(&self, mut install: GithubInstall) -> Result<GithubInstall> {
        // Fail-closed structural validation before any write (the id becomes a
        // storage key, each repo a path segment downstream).
        install.sanitize();
        install.validate()?;
        let id = install.installation_id.clone();
        let is_new = self
            .backend
            .get(INSTALLS, &self.tenant, &id)
            .await?
            .is_none();
        if is_new
            && self.backend.count(INSTALLS, &self.tenant).await? >= DEFAULT_MAX_CARDS_PER_TENANT
        {
            return Err(Error::Config(format!(
                "github installs is full ({DEFAULT_MAX_CARDS_PER_TENANT} for tenant `{}`)",
                self.tenant
            )));
        }
        let blob = pb::GithubInstall::from(install.clone()).encode_to_vec();
        self.backend
            .apply(&[
                Write::EnsureTenant {
                    tenant: self.tenant.clone(),
                },
                Write::Put {
                    collection: INSTALLS,
                    tenant: self.tenant.clone(),
                    id,
                    blob,
                },
            ])
            .await?;
        Ok(install)
    }

    async fn delete(&self, installation_id: &str) -> Result<bool> {
        if !safe_segment(installation_id) {
            return Err(Error::Config(format!(
                "invalid installation id `{installation_id}`"
            )));
        }
        let existed = self
            .backend
            .get(INSTALLS, &self.tenant, installation_id)
            .await?
            .is_some();
        self.backend
            .apply(&[Write::Delete {
                collection: INSTALLS,
                tenant: self.tenant.clone(),
                id: installation_id.to_string(),
            }])
            .await?;
        Ok(existed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_config_store::MemoryBackend;
    use rstest::rstest;

    fn store() -> StoreInstalls {
        StoreInstalls::new(Arc::new(MemoryBackend::new()))
    }

    fn install(id: &str, repos: &[&str]) -> GithubInstall {
        GithubInstall {
            installation_id: id.to_string(),
            selected_repos: repos.iter().map(ToString::to_string).collect(),
            installed_at: 1_700_000_000,
        }
    }

    // desc: a put→get round-trips the stored install verbatim, and list returns it.
    #[tokio::test]
    async fn positive_put_get_roundtrip() {
        let s = store();
        let c = install("12345", &["octo__hello"]);
        let stored = s.put(c.clone()).await.expect("put");
        assert_eq!(stored, c);
        assert_eq!(s.get("12345").await.expect("get"), c);
        assert_eq!(s.list().await.expect("list"), vec![c]);
    }

    // desc: put is an upsert — a second put with the same id replaces the card.
    #[tokio::test]
    async fn positive_put_upserts() {
        let s = store();
        s.put(install("777", &["a__b"])).await.expect("put");
        let c2 = install("777", &["a__b", "c__d"]);
        s.put(c2.clone()).await.expect("upsert");
        assert_eq!(
            s.get("777").await.expect("get").selected_repos,
            c2.selected_repos
        );
        assert_eq!(s.list().await.expect("list").len(), 1);
    }

    // desc: get of an unknown id is a `not found` error (maps to NotFound on the wire).
    #[tokio::test]
    async fn negative_missing_install() {
        let s = store();
        let err = s.get("404").await.expect_err("must be absent");
        assert!(err.to_string().contains("not found"), "got: {err}");
    }

    // desc: delete is idempotent — true when it existed, false when absent.
    #[tokio::test]
    async fn negative_delete_absent_is_false() {
        let s = store();
        assert!(!s.delete("999").await.expect("delete absent"));
        s.put(install("999", &[])).await.expect("put");
        assert!(s.delete("999").await.expect("delete present"));
        assert!(!s.delete("999").await.expect("delete again"));
    }

    // boundary: put rejects an install whose id is not the decimal shape (validate).
    #[tokio::test]
    async fn boundary_put_rejects_non_digit_id() {
        let s = store();
        assert!(s.put(install("12a", &["a__b"])).await.is_err());
    }

    // adversarial: a hostile id never mutates the store, on get/delete (the key path).
    #[rstest]
    #[case::traversal("../etc")]
    #[case::separator("a/b")]
    #[case::empty("")]
    #[tokio::test]
    async fn adversarial_hostile_id_rejected(#[case] id: &str) {
        let s = store();
        assert!(s.get(id).await.is_err(), "get {id:?}");
        assert!(s.delete(id).await.is_err(), "delete {id:?}");
        assert!(s.put(install(id, &["a__b"])).await.is_err(), "put {id:?}");
    }

    // adversarial: a hostile tenant is rejected at construction.
    #[test]
    fn adversarial_hostile_tenant_rejected() {
        let b: Arc<dyn Backend> = Arc::new(MemoryBackend::new());
        assert!(StoreInstalls::with_tenant(b.clone(), "../etc").is_err());
        assert!(StoreInstalls::with_tenant(b, "acme").is_ok());
    }

    // adversarial: two tenants over the same backend never see each other's installs.
    #[tokio::test]
    async fn adversarial_cross_tenant_no_leak() {
        let b: Arc<dyn Backend> = Arc::new(MemoryBackend::new());
        let acme = StoreInstalls::with_tenant(b.clone(), "acme").expect("tenant");
        let globex = StoreInstalls::with_tenant(b, "globex").expect("tenant");
        acme.put(install("12345", &["a__b"])).await.expect("put");
        assert_eq!(acme.list().await.expect("list").len(), 1);
        assert!(globex.list().await.expect("list").is_empty());
        assert!(
            globex.get("12345").await.is_err(),
            "cross-tenant get must miss"
        );
    }
}
