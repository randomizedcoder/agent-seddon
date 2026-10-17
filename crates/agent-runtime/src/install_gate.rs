//! Keeping a sync-readable snapshot of the per-tenant install store — an **F5 building
//! block** the mint IDOR gate does **not** consult yet.
//!
//! The gate (`secrets::ghapp_installation_allowed`) is **synchronous** and shared with
//! callers that never mint, but the install store ([`GithubInstallRegistry`]) is **async**
//! and per-tenant, so the store cannot be queried on the gate path. The intended bridge is
//! the sync-readable snapshot `secrets::GHAPP_STORE_BINDINGS`, which this decorator keeps
//! fresh with single-id edits as installs are onboarded or revoked over the CRUD seam.
//!
//! **Security (why the gate does not read this snapshot yet):** a store row is
//! **tenant-self-asserted** — any tenant with `write:forge_registry` can `put` a row naming
//! *any* `installation_id` (shape-validated, never ownership-validated). Treating that as
//! mint authority is a cross-tenant IDOR (a tenant self-grants minting for an installation it
//! does not own). So this decorator, and the snapshot it feeds, are **wired but unread**: the
//! gate consults only the operator-vouched `[github_app]` bootstrap allowlist. forge-auth
//! **F5** makes the snapshot authoritative only once a row's ownership is proven (the GitHub
//! App installation callback cross-checked via the App JWT against the tenant's OAuth-linked
//! account); it must bind into the snapshot from *that verified path*, not from a raw `put`.
//! Until then, do not union `GHAPP_STORE_BINDINGS` into `ghapp_allowed`.
//!
//! [`GithubInstallRegistry`]: agent_core::GithubInstallRegistry

use std::sync::Arc;

use agent_core::{current_tenant, GithubInstall, GithubInstallRegistry, Result};
use async_trait::async_trait;

/// Sink for the single-id edits the install CRUD path pushes into the mint gate's
/// sync-readable store snapshot. The production sink forwards to
/// `secrets::ghapp_store_bind`/`ghapp_store_unbind`; tests record the edits instead of
/// touching the process-global snapshot.
pub trait GhappStoreSink: Send + Sync {
    fn bind(&self, tenant: &str, id: &str);
    fn unbind(&self, tenant: &str, id: &str);
}

/// The production sink: forwards each edit to the process-global gate snapshot.
struct SecretsSink;

impl GhappStoreSink for SecretsSink {
    fn bind(&self, tenant: &str, id: &str) {
        crate::secrets::ghapp_store_bind(tenant, id);
    }
    fn unbind(&self, tenant: &str, id: &str) {
        crate::secrets::ghapp_store_unbind(tenant, id);
    }
}

/// A [`GithubInstallRegistry`] decorator that keeps the install-store snapshot fresh (an F5
/// building block — the mint gate does not read the snapshot yet; see the module docs): a
/// `put` that persists **binds** the row for the caller's tenant, a `delete` **unbinds** it.
/// The tenant is the ambient verified identity ([`current_tenant`]) — the same key the
/// wrapped `PerTenant` store routed the write on — so a caller only ever edits its own
/// tenant's snapshot entry.
///
/// **This is not an ownership check.** A `put` reflects only that the tenant *wrote* a row,
/// not that it *owns* the installation, so binding on a raw `put` must never by itself
/// authorize minting. F5 replaces the bind source with the ownership-verified install
/// callback before the gate is wired to read the snapshot (see the module docs' security
/// note). Until then this decorator is unmounted and its edits feed a snapshot nothing reads.
///
/// Reads (`list`/`get`) pass straight through. A snapshot edit happens **only after** the
/// inner write succeeds, so a failed or rejected write leaves the snapshot untouched and the
/// snapshot stays a subset of the store (never an id the store does not hold).
pub struct RefreshingInstalls {
    inner: Arc<dyn GithubInstallRegistry>,
    sink: Arc<dyn GhappStoreSink>,
}

impl RefreshingInstalls {
    /// Wrap `inner`, forwarding snapshot edits to the process-global gate (the production
    /// wiring the served registry uses).
    pub fn new(inner: Arc<dyn GithubInstallRegistry>) -> Self {
        Self {
            inner,
            sink: Arc::new(SecretsSink),
        }
    }

    #[cfg(test)]
    fn with_sink(inner: Arc<dyn GithubInstallRegistry>, sink: Arc<dyn GhappStoreSink>) -> Self {
        Self { inner, sink }
    }
}

#[async_trait]
impl GithubInstallRegistry for RefreshingInstalls {
    async fn list(&self) -> Result<Vec<GithubInstall>> {
        self.inner.list().await
    }

    async fn get(&self, installation_id: &str) -> Result<GithubInstall> {
        self.inner.get(installation_id).await
    }

    async fn put(&self, install: GithubInstall) -> Result<GithubInstall> {
        let stored = self.inner.put(install).await?;
        // The stored id is the store's validated id — never a hostile value in the snapshot.
        self.sink.bind(&current_tenant(), &stored.installation_id);
        Ok(stored)
    }

    async fn delete(&self, installation_id: &str) -> Result<bool> {
        let existed = self.inner.delete(installation_id).await?;
        // Unbind unconditionally once the (validating) delete returns Ok: idempotent, and it
        // closes the gate on the id even if the row was already gone — keeping the snapshot a
        // subset of the store (fail closed). A rejected/errored delete returns above, so a
        // hostile id never reaches here.
        self.sink.unbind(&current_tenant(), installation_id);
        Ok(existed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_config_store::{Backend, MemoryBackend};
    use agent_core::{scope, SessionKey};
    use std::sync::Mutex;

    /// Records every snapshot edit as `("bind"|"unbind", tenant, id)`.
    #[derive(Default)]
    struct RecordingSink {
        edits: Mutex<Vec<(&'static str, String, String)>>,
    }
    impl GhappStoreSink for RecordingSink {
        fn bind(&self, tenant: &str, id: &str) {
            self.edits
                .lock()
                .unwrap()
                .push(("bind", tenant.to_string(), id.to_string()));
        }
        fn unbind(&self, tenant: &str, id: &str) {
            self.edits
                .lock()
                .unwrap()
                .push(("unbind", tenant.to_string(), id.to_string()));
        }
    }

    fn install(id: &str) -> GithubInstall {
        GithubInstall {
            installation_id: id.into(),
            selected_repos: vec!["octo__hello".into()],
            installed_at: 1_700_000_000,
        }
    }

    // A per-tenant store (so `current_tenant()` routes the write) wrapped in the decorator.
    fn wrapped(sink: Arc<RecordingSink>) -> RefreshingInstalls {
        let backend: Arc<dyn Backend> = Arc::new(MemoryBackend::new());
        let inner = Arc::new(crate::tenant::PerTenant::new(move |t| {
            agent_forge::StoreInstalls::with_tenant(backend.clone(), t)
                .map(|s| Arc::new(s) as Arc<dyn GithubInstallRegistry>)
                .unwrap_or_else(|_| Arc::new(agent_forge::StoreInstalls::new(backend.clone())))
        })) as Arc<dyn GithubInstallRegistry>;
        RefreshingInstalls::with_sink(inner, sink)
    }

    // desc: a persisted put binds, and a delete unbinds, the caller's tenant + the row id.
    #[tokio::test]
    async fn positive_put_binds_delete_unbinds() {
        let sink = Arc::new(RecordingSink::default());
        let reg = wrapped(sink.clone());
        scope(SessionKey::parse("acme", "s1").unwrap(), async {
            reg.put(install("12345")).await.unwrap();
            reg.delete("12345").await.unwrap();
        })
        .await;
        assert_eq!(
            sink.edits.lock().unwrap().as_slice(),
            &[
                ("bind", "acme".to_string(), "12345".to_string()),
                ("unbind", "acme".to_string(), "12345".to_string()),
            ]
        );
    }

    // desc: reads never edit the snapshot (list/get are pass-through).
    #[tokio::test]
    async fn positive_reads_do_not_edit_snapshot() {
        let sink = Arc::new(RecordingSink::default());
        let reg = wrapped(sink.clone());
        scope(SessionKey::parse("acme", "s1").unwrap(), async {
            reg.list().await.unwrap();
            let _ = reg.get("12345").await; // miss is fine
        })
        .await;
        assert!(sink.edits.lock().unwrap().is_empty());
    }

    // negative: a rejected put (invalid installation id) never binds — the snapshot stays a
    // subset of the store (nothing was persisted, so nothing is authorized).
    #[tokio::test]
    async fn negative_rejected_put_does_not_bind() {
        let sink = Arc::new(RecordingSink::default());
        let reg = wrapped(sink.clone());
        scope(SessionKey::parse("acme", "s1").unwrap(), async {
            assert!(reg.put(install("12a")).await.is_err());
        })
        .await;
        assert!(sink.edits.lock().unwrap().is_empty());
    }

    // adversarial: two tenants' edits carry each one's own tenant key — a caller can never
    // bind an id under another tenant's snapshot entry (the tenant-scoping F5's verified gate
    // read will rely on; the snapshot is not an authority on its own — see the module docs).
    #[tokio::test]
    async fn adversarial_edits_are_scoped_to_caller_tenant() {
        let sink = Arc::new(RecordingSink::default());
        let reg = wrapped(sink.clone());
        scope(SessionKey::parse("acme", "s1").unwrap(), async {
            reg.put(install("111")).await.unwrap();
        })
        .await;
        scope(SessionKey::parse("globex", "s1").unwrap(), async {
            reg.put(install("222")).await.unwrap();
        })
        .await;
        assert_eq!(
            sink.edits.lock().unwrap().as_slice(),
            &[
                ("bind", "acme".to_string(), "111".to_string()),
                ("bind", "globex".to_string(), "222".to_string()),
            ]
        );
    }
}
