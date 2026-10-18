//! `StoreInstalls` — the [`GithubInstallRegistry`] over a shared
//! [`agent_config_store::Backend`] (forge-auth F4).
//!
//! One card per GitHub App installation (id = the decimal `installation_id`).
//! Mirrors [`StoreForges`](crate::StoreForges): it persists the prost
//! `pb::GithubInstall` blob directly, and is single-tenant `local` until the
//! per-tenant plane calls [`StoreInstalls::with_tenant`]. Every id and tenant is
//! `safe_segment`/`valid_installation_id`-gated before it becomes a storage key, and
//! a stored blob is re-validated on load (fail-closed on an out-of-band tamper).

use std::collections::{BTreeMap, BTreeSet};
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

/// The collection of **ownership-verified** installation bindings (forge-auth F5c2), one
/// row per installation a tenant proved it administers. Deliberately a *separate*
/// collection from [`INSTALLS`]: the CRUD seam (`GithubInstallRegistry::put`) writes only
/// `INSTALLS`, whose rows are tenant-self-asserted, so no open CRUD call can ever reach
/// this one. Only the onboarding `CompleteInstall` path writes here, after the proof.
pub const INSTALLS_VERIFIED: &str = "github_installs_verified";
/// Longest verified principal `subject` we persist (an OIDC `sub` is opaque but short).
pub const MAX_SUBJECT_LEN: usize = 256;

/// One installation a tenant **proved** it administers (forge-auth F5c2): the record the
/// mint gate's verified snapshot is rebuilt from. The tenant is the store's tenant
/// dimension, not a field. `account_login`/`account_kind` are GitHub's own App-JWT record
/// of the account the installation lives on; `subject` is the verified principal that
/// completed the onboarding (audit).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedInstall {
    pub installation_id: String,
    pub account_login: String,
    /// `"User"` or `"Organization"` — the only account kinds an ownership proof accepts.
    pub account_kind: String,
    pub subject: String,
    /// When ownership was proven, Unix seconds.
    pub verified_at: u64,
}

impl VerifiedInstall {
    /// Fail-closed structural validation, run before any write and after any load (a row
    /// that fails it is an out-of-band tamper). A rejection names the field, never the value.
    pub fn validate(&self) -> Result<()> {
        if !agent_core::valid_installation_id(&self.installation_id) {
            return Err(Error::Config(
                "verified install: installation_id must be decimal digits (<= 20)".into(),
            ));
        }
        if !agent_core::valid_github_login(&self.account_login) {
            return Err(Error::Config(
                "verified install: account_login is not a GitHub login".into(),
            ));
        }
        if !matches!(self.account_kind.as_str(), "User" | "Organization") {
            return Err(Error::Config(
                "verified install: account_kind must be User or Organization".into(),
            ));
        }
        if self.subject.is_empty()
            || self.subject.len() > MAX_SUBJECT_LEN
            || self.subject.chars().any(char::is_control)
        {
            return Err(Error::Config(
                "verified install: subject must be 1..=256 printable chars".into(),
            ));
        }
        Ok(())
    }

    fn encode(&self) -> Vec<u8> {
        serde_json::json!({
            "installation_id": self.installation_id,
            "account_login": self.account_login,
            "account_kind": self.account_kind,
            "subject": self.subject,
            "verified_at": self.verified_at,
        })
        .to_string()
        .into_bytes()
    }

    /// Decode + validate a stored row; any missing/mistyped field or failed validation is
    /// a fail-closed error, never a partial record.
    fn decode(blob: &[u8]) -> Result<Self> {
        let bad = || Error::Config("stored verified install: malformed".into());
        let v: serde_json::Value = serde_json::from_slice(blob).map_err(|_| bad())?;
        let s = |k: &str| {
            v.get(k)
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
                .ok_or_else(bad)
        };
        let row = Self {
            installation_id: s("installation_id")?,
            account_login: s("account_login")?,
            account_kind: s("account_kind")?,
            subject: s("subject")?,
            verified_at: v
                .get("verified_at")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(bad)?,
        };
        row.validate()?;
        Ok(row)
    }
}

/// The per-tenant verified-install store over [`INSTALLS_VERIFIED`] (forge-auth F5c2).
/// **Not** a [`GithubInstallRegistry`]: no CRUD seam is served over it, so a tenant cannot
/// write a verified row except through the onboarding proof.
pub struct VerifiedInstalls {
    backend: Arc<dyn Backend>,
    tenant: String,
}

impl VerifiedInstalls {
    /// A verified store scoped to `tenant` (`safe_segment`-gated, like [`StoreInstalls`]).
    pub fn with_tenant(backend: Arc<dyn Backend>, tenant: &str) -> Result<Self> {
        if !safe_segment(tenant) {
            return Err(Error::Config("invalid tenant".into()));
        }
        Ok(Self {
            backend,
            tenant: tenant.to_string(),
        })
    }

    /// Persist a verified binding (upsert). Validated before the write; a new row is
    /// refused once the tenant holds [`DEFAULT_MAX_CARDS_PER_TENANT`].
    pub async fn put(&self, row: &VerifiedInstall) -> Result<()> {
        row.validate()?;
        let id = row.installation_id.clone();
        let is_new = self
            .backend
            .get(INSTALLS_VERIFIED, &self.tenant, &id)
            .await?
            .is_none();
        if is_new
            && self.backend.count(INSTALLS_VERIFIED, &self.tenant).await?
                >= DEFAULT_MAX_CARDS_PER_TENANT
        {
            return Err(Error::Config(format!(
                "verified installs is full ({DEFAULT_MAX_CARDS_PER_TENANT} per tenant)"
            )));
        }
        self.backend
            .apply(&[
                Write::EnsureTenant {
                    tenant: self.tenant.clone(),
                },
                Write::Put {
                    collection: INSTALLS_VERIFIED,
                    tenant: self.tenant.clone(),
                    id,
                    blob: row.encode(),
                },
            ])
            .await?;
        Ok(())
    }

    /// This tenant's verified rows (each re-validated on load).
    pub async fn list(&self) -> Result<Vec<VerifiedInstall>> {
        self.backend
            .list(INSTALLS_VERIFIED, &self.tenant)
            .await?
            .iter()
            .map(|b| VerifiedInstall::decode(b))
            .collect()
    }
}

/// Every **verified** binding, as a `tenant → {installation_id}` map across all tenants —
/// the startup seed of the mint gate's verified snapshot
/// (`secrets::install_github_app_store_bindings`). The gate is synchronous and
/// cross-tenant, but the store is async and per-tenant, so the builder calls this once.
///
/// Reads [`INSTALLS_VERIFIED`] **only** — never the tenant-self-asserted CRUD
/// [`INSTALLS`] collection (seeding from that would be the F4b cross-tenant minting IDOR).
/// Every row is decoded through the validating path; a blob that fails (an out-of-band
/// tamper) fails the whole rebuild closed, so a corrupt row can never silently widen the
/// gate. A tenant with no rows contributes no entry (a `get(tenant)` miss, never an
/// empty-set hit — the fail-closed shape the gate expects).
pub async fn verified_install_bindings(
    backend: &Arc<dyn Backend>,
) -> Result<BTreeMap<String, BTreeSet<String>>> {
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for tenant in backend.tenants(INSTALLS_VERIFIED).await? {
        let mut ids = BTreeSet::new();
        for blob in backend.list(INSTALLS_VERIFIED, &tenant).await? {
            ids.insert(VerifiedInstall::decode(&blob)?.installation_id);
        }
        if !ids.is_empty() {
            out.insert(tenant, ids);
        }
    }
    Ok(out)
}

/// The tenant `installation_id` is already verified-bound to, if any (one tenant per
/// installation). A scan of [`verified_install_bindings`]; fails closed on a tamper.
/// Single-process: two replicas completing the same install concurrently are not
/// serialized against each other (documented limit; F6 webhooks reconcile).
pub async fn verified_owner(
    backend: &Arc<dyn Backend>,
    installation_id: &str,
) -> Result<Option<String>> {
    Ok(verified_install_bindings(backend)
        .await?
        .into_iter()
        .find_map(|(t, ids)| ids.contains(installation_id).then_some(t)))
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

    fn verified(id: &str) -> VerifiedInstall {
        VerifiedInstall {
            installation_id: id.to_string(),
            account_login: "acme".into(),
            account_kind: "Organization".into(),
            subject: "user-1".into(),
            verified_at: 1_700_000_000,
        }
    }

    fn ids(v: &[&str]) -> BTreeSet<String> {
        v.iter().map(ToString::to_string).collect()
    }

    // `VerifiedInstall::validate` guards every persisted (and re-loaded) verified row.
    #[rstest]
    #[case::positive_org(verified("42"), true)]
    #[case::positive_user(VerifiedInstall { account_kind: "User".into(), ..verified("42") }, true)]
    #[case::boundary_subject_at_cap(VerifiedInstall { subject: "s".repeat(MAX_SUBJECT_LEN), ..verified("42") }, true)]
    #[case::boundary_subject_over_cap(VerifiedInstall { subject: "s".repeat(MAX_SUBJECT_LEN + 1), ..verified("42") }, false)]
    #[case::negative_empty_subject(VerifiedInstall { subject: String::new(), ..verified("42") }, false)]
    #[case::negative_non_digit_id(verified("4a"), false)]
    #[case::corner_enterprise_kind_never_stored(VerifiedInstall { account_kind: "Enterprise".into(), ..verified("42") }, false)]
    #[case::adversarial_traversal_login(VerifiedInstall { account_login: "../x".into(), ..verified("42") }, false)]
    #[case::adversarial_crlf_subject(VerifiedInstall { subject: "a\r\nb".into(), ..verified("42") }, false)]
    #[case::adversarial_traversal_id(verified("../42"), false)]
    fn verified_validate_matrix(#[case] row: VerifiedInstall, #[case] ok: bool) {
        assert_eq!(row.validate().is_ok(), ok);
    }

    // desc: verified_install_bindings rebuilds tenant→ids across tenants (the startup seed
    // of the mint gate's verified snapshot), and verified_owner finds the binding tenant.
    #[tokio::test]
    async fn positive_verified_bindings_rebuild_across_tenants() {
        let b: Arc<dyn Backend> = Arc::new(MemoryBackend::new());
        let acme = VerifiedInstalls::with_tenant(b.clone(), "acme").expect("tenant");
        let beta = VerifiedInstalls::with_tenant(b.clone(), "beta").expect("tenant");
        acme.put(&verified("123")).await.expect("put");
        acme.put(&verified("456")).await.expect("put");
        beta.put(&verified("789")).await.expect("put");
        let map = verified_install_bindings(&b).await.expect("rebuild");
        assert_eq!(map.get("acme"), Some(&ids(&["123", "456"])));
        assert_eq!(map.get("beta"), Some(&ids(&["789"])));
        assert_eq!(acme.list().await.expect("list").len(), 2);
        assert_eq!(
            verified_owner(&b, "789").await.expect("scan").as_deref(),
            Some("beta")
        );
        assert_eq!(verified_owner(&b, "999").await.expect("scan"), None);
    }

    // adversarial: the boot seed reads ONLY the verified collection — a tenant-self-
    // asserted CRUD row (the F4b IDOR shape) never seeds the mint gate.
    #[tokio::test]
    async fn adversarial_crud_rows_never_seed_verified_bindings() {
        let b: Arc<dyn Backend> = Arc::new(MemoryBackend::new());
        StoreInstalls::with_tenant(b.clone(), "mallory")
            .expect("tenant")
            .put(install("123", &["a__b"]))
            .await
            .expect("crud put");
        assert!(verified_install_bindings(&b)
            .await
            .expect("rebuild")
            .is_empty());
        assert_eq!(verified_owner(&b, "123").await.expect("scan"), None);
    }

    // adversarial: a tampered verified blob (undecodable, or decodable but invalid) fails
    // the whole rebuild closed, so a corrupt row can never silently widen the gate.
    #[rstest]
    #[case::adversarial_not_json(b"not json".to_vec())]
    #[case::adversarial_missing_field(br#"{"installation_id":"123"}"#.to_vec())]
    #[case::adversarial_hostile_login(
        br#"{"installation_id":"123","account_login":"../x","account_kind":"User","subject":"s","verified_at":1}"#.to_vec()
    )]
    #[tokio::test]
    async fn adversarial_verified_rebuild_fails_closed_on_tamper(#[case] blob: Vec<u8>) {
        let b: Arc<dyn Backend> = Arc::new(MemoryBackend::new());
        b.apply(&[
            Write::EnsureTenant {
                tenant: "acme".into(),
            },
            Write::Put {
                collection: INSTALLS_VERIFIED,
                tenant: "acme".into(),
                id: "123".into(),
                blob,
            },
        ])
        .await
        .expect("seed tamper");
        assert!(verified_install_bindings(&b).await.is_err());
        assert!(verified_owner(&b, "123").await.is_err());
    }

    // negative: an invalid verified row is never written.
    #[tokio::test]
    async fn negative_invalid_verified_row_not_written() {
        let b: Arc<dyn Backend> = Arc::new(MemoryBackend::new());
        let acme = VerifiedInstalls::with_tenant(b.clone(), "acme").expect("tenant");
        assert!(acme.put(&verified("12a")).await.is_err());
        assert!(acme.list().await.expect("list").is_empty());
        assert!(VerifiedInstalls::with_tenant(b, "../etc").is_err());
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
