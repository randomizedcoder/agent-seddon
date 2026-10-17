//! The `GithubInstallRegistryService` seam (forge-auth F4): the GitHub-App install
//! control plane — CRUD over the persisted **install cards** `{ installation_id,
//! selected_repos[], installed_at }` the portal onboarding flow (F5) writes and the
//! webhook receiver (F6) reads. Mirrors `ForgeRegistryService` (the way
//! `ProviderRegistryService` cards relate to the `LlmProvider` capability seam).
//!
//! Fails **hard** (`Err` → `Status`), like the other control-plane seams: a failed
//! mutation surfaces to the operator, never silently degrades. `installation_id` and
//! every `selected_repos` slug are untrusted and validated in the store
//! (`valid_installation_id`, `safe_segment`) — a rejection maps to `InvalidArgument`
//! and a store's `not found` to `NotFound`, via `status_from_error`. Mutations are
//! gated by the RBAC enforcement core (C1): a caller needs a role granting
//! `(write|delete, forge_registry)` (the forge-domain resource — installs are forge
//! state, so they share the forge-registry grant rather than minting a new one).
//!
//! Every RPC scopes the caller's ambient identity (`identity_key` + `run_scoped`)
//! before touching the store, so a `PerTenant<dyn GithubInstallRegistry>`-wrapped
//! registry routes a caller to *their* tenant's installs rather than the default
//! `local` set. Identity is attacker-controlled and fails **closed** to `None` → the
//! `local` tenant (see `server::identity_key`).

use std::sync::Arc;

use agent_core::GithubInstallRegistry;
use agent_proto::{pb, status_from_error};
use tonic::transport::server::Router;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::Instrument;

use super::span;

pub struct GithubInstallRegistrySvc {
    inner: Arc<dyn GithubInstallRegistry>,
}

impl GithubInstallRegistrySvc {
    pub fn new(inner: Arc<dyn GithubInstallRegistry>) -> Self {
        Self { inner }
    }
    pub fn into_server(
        self,
    ) -> pb::github_install_registry_service_server::GithubInstallRegistryServiceServer<Self> {
        pb::github_install_registry_service_server::GithubInstallRegistryServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl pb::github_install_registry_service_server::GithubInstallRegistryService
    for GithubInstallRegistrySvc
{
    async fn list(
        &self,
        request: Request<pb::GithubInstallListRequest>,
    ) -> Result<Response<pb::GithubInstallList>, Status> {
        let key = super::identity_key(request.metadata());
        let sp = span("github_install_registry.list", request.metadata());
        let inner = self.inner.clone();
        let work = async move {
            let installs = inner.list().await.map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::GithubInstallList {
                installs: installs.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }

    async fn get(
        &self,
        request: Request<pb::GithubInstallRef>,
    ) -> Result<Response<pb::GithubInstall>, Status> {
        let key = super::identity_key(request.metadata());
        let sp = span("github_install_registry.get", request.metadata());
        let inner = self.inner.clone();
        let work = async move {
            let install = inner
                .get(&request.into_inner().installation_id)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(install.into()))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }

    async fn put(
        &self,
        request: Request<pb::GithubInstall>,
    ) -> Result<Response<pb::GithubInstall>, Status> {
        super::authz::require(
            agent_core::Action::Write,
            agent_core::ResourceType::ForgeRegistry,
        )?;
        let key = super::identity_key(request.metadata());
        let sp = span("github_install_registry.put", request.metadata());
        let inner = self.inner.clone();
        let work = async move {
            // Wire → core is total here; the store is the fail-closed validation gate
            // (installation_id shape + each selected_repos slug) before it persists.
            let install = agent_core::GithubInstall::from(request.into_inner());
            let stored = inner
                .put(install)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(stored.into()))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }

    async fn delete(
        &self,
        request: Request<pb::GithubInstallRef>,
    ) -> Result<Response<pb::GithubInstallDeleteReply>, Status> {
        super::authz::require(
            agent_core::Action::Delete,
            agent_core::ResourceType::ForgeRegistry,
        )?;
        let key = super::identity_key(request.metadata());
        let sp = span("github_install_registry.delete", request.metadata());
        let inner = self.inner.clone();
        let work = async move {
            let deleted = inner
                .delete(&request.into_inner().installation_id)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::GithubInstallDeleteReply { deleted }))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }
}

pub fn github_install_registry_router(inner: Arc<dyn GithubInstallRegistry>) -> Router {
    Server::builder().add_service(GithubInstallRegistrySvc::new(inner).into_server())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::GithubInstall;
    use agent_proto::identity::{SESSION_ID_KEY, USER_ID_KEY};
    use pb::github_install_registry_service_server::GithubInstallRegistryService as _;
    use rstest::rstest;
    use std::sync::Mutex;
    use tonic::metadata::MetadataValue;

    // Records the ambient tenant (`current_identity().user`) at each store call —
    // proves the served handler scopes the caller into a `PerTenant`-wrapped registry.
    #[derive(Default)]
    struct TenantProbeInstalls {
        seen: Arc<Mutex<Vec<Option<String>>>>,
    }
    impl TenantProbeInstalls {
        fn record(&self) {
            self.seen
                .lock()
                .unwrap()
                .push(agent_core::current_identity().map(|k| k.user.as_str().to_string()));
        }
    }
    #[tonic::async_trait]
    impl GithubInstallRegistry for TenantProbeInstalls {
        async fn list(&self) -> agent_core::Result<Vec<GithubInstall>> {
            self.record();
            Ok(vec![])
        }
        async fn get(&self, id: &str) -> agent_core::Result<GithubInstall> {
            self.record();
            Err(agent_core::Error::Config(format!("not found: {id}")))
        }
        async fn put(&self, i: GithubInstall) -> agent_core::Result<GithubInstall> {
            self.record();
            Ok(i)
        }
        async fn delete(&self, _id: &str) -> agent_core::Result<bool> {
            self.record();
            Ok(false)
        }
    }

    fn req_with<T>(payload: T, user: Option<&str>, session: Option<&str>) -> Request<T> {
        let mut req = Request::new(payload);
        if let Some(u) = user {
            req.metadata_mut()
                .insert(USER_ID_KEY, MetadataValue::try_from(u).unwrap());
        }
        if let Some(s) = session {
            req.metadata_mut()
                .insert(SESSION_ID_KEY, MetadataValue::try_from(s).unwrap());
        }
        req
    }

    // Every caller-identity class → the tenant the store runs under. Present +
    // path-safe scopes; partial/hostile fails closed to the default tenant (`None`).
    #[rstest]
    #[case::positive_tenant_header_scopes_to_tenant(Some("acme".into()), Some("s1".into()), Some("acme".into()))]
    #[case::negative_no_identity_runs_as_local(None, None, None)]
    #[case::negative_second_tenant_is_isolated(Some("globex".into()), Some("s1".into()), Some("globex".into()))]
    #[case::boundary_max_len_tenant_segment(Some("a".repeat(128)), Some("s1".into()), Some("a".repeat(128)))]
    #[case::corner_user_without_session_runs_as_local(Some("acme".into()), None, None)]
    #[case::adversarial_traversal_header_fails_to_local(Some("../../heads/main".into()), Some("s1".into()), None)]
    #[case::adversarial_empty_header_fails_to_local(Some(String::new()), Some("s1".into()), None)]
    #[tokio::test]
    async fn list_scopes_caller_tenant(
        #[case] user: Option<String>,
        #[case] session: Option<String>,
        #[case] expected: Option<String>,
    ) {
        let store = Arc::new(TenantProbeInstalls::default());
        let svc = GithubInstallRegistrySvc::new(store.clone());
        let req = req_with(
            pb::GithubInstallListRequest::default(),
            user.as_deref(),
            session.as_deref(),
        );
        svc.list(req).await.unwrap();
        assert_eq!(store.seen.lock().unwrap().as_slice(), &[expected]);
    }

    // Two tenants over the wire hit disjoint scopes across different RPCs.
    #[tokio::test]
    async fn positive_two_tenants_are_isolated_across_rpcs() {
        let store = Arc::new(TenantProbeInstalls::default());
        let svc = GithubInstallRegistrySvc::new(store.clone());
        svc.list(req_with(
            pb::GithubInstallListRequest::default(),
            Some("acme"),
            Some("s1"),
        ))
        .await
        .unwrap();
        svc.delete(req_with(
            pb::GithubInstallRef {
                installation_id: "12345".into(),
            },
            Some("globex"),
            Some("s1"),
        ))
        .await
        .unwrap();
        assert_eq!(
            store.seen.lock().unwrap().as_slice(),
            &[Some("acme".to_string()), Some("globex".to_string())]
        );
    }
}
