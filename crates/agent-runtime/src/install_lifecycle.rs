//! GitHub App installation lifecycle from webhooks (forge-auth F6): the
//! [`GithubInstallLifecycle`] over the verified-install store and the `ghapp:` mint gate.
//!
//! An uninstall or suspension **narrows** the gate: the verified snapshot loses the
//! binding (`secrets::ghapp_store_unbind`) and the minter forgets its cached token, so the
//! next use fails closed here rather than riding a still-cached token. An uninstall also
//! releases the one-tenant-per-installation claim and deletes the verified + display rows
//! (so the installation can later be onboarded again, by whoever proves they administer
//! it). An unsuspend restores a binding only when the tenant **still holds** the verified
//! claim; nothing here can bind an installation that was never proven (F5c2 stays the
//! only way in). A repo-selection change only edits the display row.
//!
//! The owning tenant always comes from the verified claim ([`Self::owner_of`]), never
//! from the payload.
//!
//! **Limits (documented):** the gate is per process — another replica keeps its binding
//! until restart, where GitHub's own refusal to mint for a removed installation fails it
//! closed; a suspension is not persisted, so a restart during one re-binds from the
//! verified rows (GitHub still refuses the mint while suspended).

use std::sync::Arc;

use agent_config_store::Backend;
use agent_core::{
    valid_installation_id, GithubInstall, GithubInstallLifecycle, GithubInstallRegistry, Result,
    MAX_SELECTED_REPOS,
};
use agent_forge::{verified_owner, StoreInstalls, VerifiedInstalls};
use async_trait::async_trait;

/// A gate edit: `(tenant, installation_id)`.
type GateFn = Arc<dyn Fn(&str, &str) + Send + Sync>;
/// Forget an installation's cached token.
type ForgetFn = Arc<dyn Fn(&str) + Send + Sync>;

/// See the [module docs](self).
pub struct InstallLifecycle {
    backend: Arc<dyn Backend>,
    bind: GateFn,
    unbind: GateFn,
    forget: ForgetFn,
}

impl InstallLifecycle {
    /// The lifecycle over the install store's `backend`, editing the process mint gate.
    pub fn new(backend: Arc<dyn Backend>) -> Self {
        Self::with_gate(
            backend,
            Arc::new(crate::secrets::ghapp_store_bind),
            Arc::new(crate::secrets::ghapp_store_unbind),
            Arc::new(|id: &str| {
                if let Some(m) = crate::secrets::minter() {
                    m.forget(id);
                }
            }),
        )
    }

    fn with_gate(
        backend: Arc<dyn Backend>,
        bind: GateFn,
        unbind: GateFn,
        forget: ForgetFn,
    ) -> Self {
        Self {
            backend,
            bind,
            unbind,
            forget,
        }
    }

    fn checked(installation_id: &str) -> Result<()> {
        if valid_installation_id(installation_id) {
            Ok(())
        } else {
            Err(agent_core::Error::Config("invalid installation id".into()))
        }
    }
}

/// `current` with `added` appended (each once, in order) and `removed` dropped, capped at
/// [`MAX_SELECTED_REPOS`] — the display row's repo list after a webhook's change.
fn merge_repos(current: &[String], added: &[String], removed: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for r in current.iter().chain(added) {
        if !removed.contains(r) && !out.contains(r) {
            out.push(r.clone());
        }
    }
    out.truncate(MAX_SELECTED_REPOS);
    out
}

#[async_trait]
impl GithubInstallLifecycle for InstallLifecycle {
    async fn owner_of(&self, installation_id: &str) -> Result<Option<String>> {
        verified_owner(&self.backend, installation_id).await
    }

    async fn revoke(&self, tenant: &str, installation_id: &str) -> Result<()> {
        Self::checked(installation_id)?;
        // Narrow the gate first, so a failure further on never leaves it open.
        (self.unbind)(tenant, installation_id);
        (self.forget)(installation_id);
        VerifiedInstalls::with_tenant(self.backend.clone(), tenant)?
            .revoke(installation_id)
            .await?;
        StoreInstalls::with_tenant(self.backend.clone(), tenant)?
            .delete(installation_id)
            .await?;
        Ok(())
    }

    async fn suspend(&self, tenant: &str, installation_id: &str) -> Result<()> {
        Self::checked(installation_id)?;
        (self.unbind)(tenant, installation_id);
        (self.forget)(installation_id);
        Ok(())
    }

    async fn unsuspend(&self, tenant: &str, installation_id: &str) -> Result<()> {
        Self::checked(installation_id)?;
        if VerifiedInstalls::with_tenant(self.backend.clone(), tenant)?
            .holds(installation_id)
            .await?
        {
            (self.bind)(tenant, installation_id);
        }
        Ok(())
    }

    async fn sync_repos(
        &self,
        tenant: &str,
        installation_id: &str,
        added: &[String],
        removed: &[String],
    ) -> Result<()> {
        Self::checked(installation_id)?;
        let store = StoreInstalls::with_tenant(self.backend.clone(), tenant)?;
        // No display row ⇒ nothing to keep in sync (onboarding writes it).
        let Ok(row) = store.get(installation_id).await else {
            return Ok(());
        };
        let selected_repos = merge_repos(&row.selected_repos, added, removed);
        if selected_repos.len() == MAX_SELECTED_REPOS {
            tracing::warn!(
                installation_id,
                "github install repo list at its cap; extra repos not recorded"
            );
        }
        store
            .put(GithubInstall {
                selected_repos,
                ..row
            })
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_config_store::MemoryBackend;
    use agent_forge::VerifiedInstall;
    use rstest::rstest;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Gate {
        log: Mutex<Vec<String>>,
    }

    fn rig() -> (Arc<dyn Backend>, Arc<Gate>, InstallLifecycle) {
        let b: Arc<dyn Backend> = Arc::new(MemoryBackend::new());
        let g = Arc::new(Gate::default());
        let (g1, g2, g3) = (g.clone(), g.clone(), g.clone());
        let l = InstallLifecycle::with_gate(
            b.clone(),
            Arc::new(move |t: &str, id: &str| {
                g1.log.lock().unwrap().push(format!("bind {t} {id}"));
            }),
            Arc::new(move |t: &str, id: &str| {
                g2.log.lock().unwrap().push(format!("unbind {t} {id}"));
            }),
            Arc::new(move |id: &str| g3.log.lock().unwrap().push(format!("forget {id}"))),
        );
        (b, g, l)
    }

    fn verified(id: &str) -> VerifiedInstall {
        VerifiedInstall {
            installation_id: id.into(),
            account_login: "acme".into(),
            account_kind: "Organization".into(),
            subject: "user-1".into(),
            verified_at: 1_700_000_000,
        }
    }

    async fn onboard(b: &Arc<dyn Backend>, tenant: &str, id: &str, repos: &[&str]) {
        let v = VerifiedInstalls::with_tenant(b.clone(), tenant).unwrap();
        assert!(v.claim(&verified(id)).await.unwrap());
        StoreInstalls::with_tenant(b.clone(), tenant)
            .unwrap()
            .put(GithubInstall {
                installation_id: id.into(),
                selected_repos: repos.iter().map(std::string::ToString::to_string).collect(),
                installed_at: 1,
            })
            .await
            .unwrap();
    }

    fn log(g: &Gate) -> Vec<String> {
        g.log.lock().unwrap().clone()
    }

    #[tokio::test]
    async fn positive_revoke_clears_gate_claim_and_rows() {
        let (b, g, l) = rig();
        onboard(&b, "acme", "42", &["acme__web"]).await;
        assert_eq!(l.owner_of("42").await.unwrap().as_deref(), Some("acme"));
        l.revoke("acme", "42").await.unwrap();
        assert_eq!(log(&g), ["unbind acme 42", "forget 42"]);
        assert_eq!(l.owner_of("42").await.unwrap(), None, "claim released");
        let store = StoreInstalls::with_tenant(b.clone(), "acme").unwrap();
        assert!(store.get("42").await.is_err(), "display row gone");
        assert!(VerifiedInstalls::with_tenant(b, "acme")
            .unwrap()
            .list()
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn adversarial_revoke_for_wrong_tenant_keeps_owners_records() {
        let (b, _g, l) = rig();
        onboard(&b, "acme", "42", &[]).await;
        // Only reachable if a caller passed a tenant other than the claim's: the records
        // of the real owner must survive (its verified claim cannot be released by beta).
        l.revoke("beta", "42").await.unwrap();
        assert_eq!(l.owner_of("42").await.unwrap().as_deref(), Some("acme"));
        assert!(VerifiedInstalls::with_tenant(b, "acme")
            .unwrap()
            .holds("42")
            .await
            .unwrap());
    }

    #[rstest]
    #[case::positive_still_verified_rebinds("acme", true)]
    #[case::adversarial_other_tenant_never_binds("beta", false)]
    #[tokio::test]
    async fn unsuspend_rebinds_only_a_held_claim(#[case] tenant: &str, #[case] binds: bool) {
        let (b, g, l) = rig();
        onboard(&b, "acme", "42", &[]).await;
        l.suspend(tenant, "42").await.unwrap();
        l.unsuspend(tenant, "42").await.unwrap();
        let mut want = vec![format!("unbind {tenant} 42"), "forget 42".to_string()];
        if binds {
            want.push(format!("bind {tenant} 42"));
        }
        assert_eq!(log(&g), want);
    }

    #[tokio::test]
    async fn negative_unsuspend_after_revoke_never_binds() {
        let (b, g, l) = rig();
        onboard(&b, "acme", "42", &[]).await;
        l.revoke("acme", "42").await.unwrap();
        l.unsuspend("acme", "42").await.unwrap();
        assert!(!log(&g).iter().any(|e| e.starts_with("bind")));
    }

    #[rstest]
    #[case::adversarial_traversal_id("../42")]
    #[case::negative_empty_id("")]
    #[tokio::test]
    async fn malformed_installation_id_refused(#[case] id: &str) {
        let (_b, g, l) = rig();
        assert!(l.revoke("acme", id).await.is_err());
        assert!(l.suspend("acme", id).await.is_err());
        assert!(l.unsuspend("acme", id).await.is_err());
        assert!(l.sync_repos("acme", id, &[], &[]).await.is_err());
        assert!(log(&g).is_empty(), "the gate is never touched");
    }

    #[rstest]
    #[case::positive_add(&["a"], &["b"], &[], &["a", "b"])]
    #[case::positive_remove(&["a", "b"], &[], &["a"], &["b"])]
    #[case::corner_add_existing_once(&["a"], &["a", "a"], &[], &["a"])]
    #[case::corner_add_and_remove_same(&["a"], &["b"], &["b"], &["a"])]
    #[case::negative_remove_absent(&["a"], &[], &["z"], &["a"])]
    fn merge_repos_cases(
        #[case] current: &[&str],
        #[case] added: &[&str],
        #[case] removed: &[&str],
        #[case] want: &[&str],
    ) {
        let v = |x: &[&str]| {
            x.iter()
                .map(std::string::ToString::to_string)
                .collect::<Vec<_>>()
        };
        assert_eq!(merge_repos(&v(current), &v(added), &v(removed)), v(want));
    }

    #[test]
    fn boundary_merge_repos_capped() {
        let added: Vec<String> = (0..MAX_SELECTED_REPOS + 10)
            .map(|i| format!("o__r{i}"))
            .collect();
        assert_eq!(merge_repos(&[], &added, &[]).len(), MAX_SELECTED_REPOS);
    }

    #[tokio::test]
    async fn positive_sync_repos_updates_display_row_only() {
        let (b, g, l) = rig();
        onboard(&b, "acme", "42", &["acme__web"]).await;
        l.sync_repos("acme", "42", &["acme__api".into()], &["acme__web".into()])
            .await
            .unwrap();
        let row = StoreInstalls::with_tenant(b, "acme")
            .unwrap()
            .get("42")
            .await
            .unwrap();
        assert_eq!(row.selected_repos, ["acme__api"]);
        assert!(log(&g).is_empty(), "repo sync never touches the gate");
    }

    #[tokio::test]
    async fn corner_sync_repos_without_display_row_is_noop() {
        let (b, _g, l) = rig();
        l.sync_repos("acme", "42", &["acme__api".into()], &[])
            .await
            .unwrap();
        assert!(StoreInstalls::with_tenant(b, "acme")
            .unwrap()
            .get("42")
            .await
            .is_err());
    }
}
