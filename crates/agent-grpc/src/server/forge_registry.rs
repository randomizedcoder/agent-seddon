//! The `ForgeRegistryService` seam (config design C36 / increment D1): the forge
//! control plane — CRUD over the persisted **forge cards** the fleet + in-loop
//! review paths build their git-host clients from, on top of the built-in host
//! impls. This is the *config-card registry* for forges; the git-host *capability*
//! seam is `agent.v1.Forge` (`server::forge`), the way `ProviderRegistryService`
//! (cards) relates to the `LlmProvider` seam.
//!
//! Fails **hard** (`Err` → `Status`), like the other control-plane seams: a failed
//! mutation surfaces to the operator, never silently degrades. Every id, kind,
//! `token_ref`, and `repo_encoding` is untrusted and validated inside the store and
//! the wire→core boundary (`safe_segment`, `ApiKeyRef::parse`, `RepoEncoding::parse`)
//! — a rejection maps to `InvalidArgument`, and a store's `not found` to `NotFound`,
//! via `status_from_error`. Mutations are gated by the RBAC enforcement core (C1):
//! a caller needs a role granting `(write|delete, forge_registry)`.
//!
//! Every RPC also scopes the caller's ambient identity (`identity_key` +
//! `run_scoped`, mirroring `prompt.rs`) before touching the store, so a
//! `PerTenant<dyn ForgeRegistry>`-wrapped registry (`agent-runtime/src/tenant.rs`)
//! routes a standalone `--serve-forge-registry` caller to *their* tenant's cards
//! rather than the default `local` set. Identity is attacker-controlled and fails
//! **closed** to `None` → the `local` tenant (see `server::identity_key`).

use std::sync::Arc;

use agent_core::ForgeRegistry;
use agent_proto::{pb, status_from_error};
use tonic::transport::server::Router;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::Instrument;

use super::span;

pub struct ForgeRegistrySvc {
    inner: Arc<dyn ForgeRegistry>,
}

impl ForgeRegistrySvc {
    pub fn new(inner: Arc<dyn ForgeRegistry>) -> Self {
        Self { inner }
    }
    pub fn into_server(
        self,
    ) -> pb::forge_registry_service_server::ForgeRegistryServiceServer<Self> {
        pb::forge_registry_service_server::ForgeRegistryServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl pb::forge_registry_service_server::ForgeRegistryService for ForgeRegistrySvc {
    async fn list(
        &self,
        request: Request<pb::ForgeListRequest>,
    ) -> Result<Response<pb::ForgeList>, Status> {
        let key = super::identity_key(request.metadata());
        let sp = span("forge_registry.list", request.metadata());
        let inner = self.inner.clone();
        let work = async move {
            let cards = inner.list().await.map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::ForgeList {
                forges: cards.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }

    async fn get(&self, request: Request<pb::ForgeRef>) -> Result<Response<pb::ForgeCard>, Status> {
        let key = super::identity_key(request.metadata());
        let sp = span("forge_registry.get", request.metadata());
        let inner = self.inner.clone();
        let work = async move {
            let card = inner
                .get(&request.into_inner().id)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(card.into()))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }

    async fn put(
        &self,
        request: Request<pb::ForgeCard>,
    ) -> Result<Response<pb::ForgeCard>, Status> {
        super::authz::require(
            agent_core::Action::Write,
            agent_core::ResourceType::ForgeRegistry,
        )?;
        let key = super::identity_key(request.metadata());
        let sp = span("forge_registry.put", request.metadata());
        let inner = self.inner.clone();
        let work = async move {
            // Wire → core is the fail-closed boundary (unknown/absent repo_encoding);
            // the store validates again (id/kind/token_ref) before it persists.
            let card = agent_core::ForgeCard::try_from(request.into_inner())
                .map_err(tonic::Status::from)?;
            let stored = inner.put(card).await.map_err(|e| status_from_error(&e))?;
            Ok(Response::new(stored.into()))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }

    async fn delete(
        &self,
        request: Request<pb::ForgeRef>,
    ) -> Result<Response<pb::ForgeDeleteReply>, Status> {
        super::authz::require(
            agent_core::Action::Delete,
            agent_core::ResourceType::ForgeRegistry,
        )?;
        let key = super::identity_key(request.metadata());
        let sp = span("forge_registry.delete", request.metadata());
        let inner = self.inner.clone();
        let work = async move {
            let deleted = inner
                .delete(&request.into_inner().id)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::ForgeDeleteReply { deleted }))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }
}

pub fn forge_registry_router(inner: Arc<dyn ForgeRegistry>) -> Router {
    Server::builder().add_service(ForgeRegistrySvc::new(inner).into_server())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::ForgeCard;
    use agent_proto::identity::{SESSION_ID_KEY, USER_ID_KEY};
    use pb::forge_registry_service_server::ForgeRegistryService as _;
    use rstest::rstest;
    use std::sync::Mutex;
    use tonic::metadata::MetadataValue;

    // Records the ambient tenant (`current_identity().user`) at each store call — proves the
    // served handler scopes the caller into a `PerTenant`-wrapped registry (C31 Principle 3).
    #[derive(Default)]
    struct TenantProbeForge {
        seen: Arc<Mutex<Vec<Option<String>>>>,
    }
    impl TenantProbeForge {
        fn record(&self) {
            self.seen
                .lock()
                .unwrap()
                .push(agent_core::current_identity().map(|k| k.user.as_str().to_string()));
        }
    }
    #[tonic::async_trait]
    impl ForgeRegistry for TenantProbeForge {
        async fn list(&self) -> agent_core::Result<Vec<ForgeCard>> {
            self.record();
            Ok(vec![])
        }
        async fn get(&self, id: &str) -> agent_core::Result<ForgeCard> {
            self.record();
            Err(agent_core::Error::Config(format!("not found: {id}")))
        }
        async fn put(&self, c: ForgeCard) -> agent_core::Result<ForgeCard> {
            self.record();
            Ok(c)
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

    // Every caller-identity class → the tenant the store runs under. Present + path-safe
    // scopes; partial/hostile fails closed to the default tenant (`None`).
    #[rstest]
    #[case::positive_tenant_header_scopes_to_tenant(Some("acme".into()), Some("s1".into()), Some("acme".into()))]
    #[case::negative_no_identity_runs_as_local(None, None, None)]
    #[case::negative_second_tenant_is_isolated(Some("globex".into()), Some("s1".into()), Some("globex".into()))]
    #[case::boundary_max_len_tenant_segment(Some("a".repeat(128)), Some("s1".into()), Some("a".repeat(128)))]
    #[case::corner_user_without_session_runs_as_local(Some("acme".into()), None, None)]
    #[case::adversarial_traversal_header_fails_to_local(Some("../../heads/main".into()), Some("s1".into()), None)]
    #[case::adversarial_empty_header_fails_to_local(Some(String::new()), Some("s1".into()), None)]
    #[tokio::test]
    async fn forge_list_scopes_caller_tenant(
        #[case] user: Option<String>,
        #[case] session: Option<String>,
        #[case] expected: Option<String>,
    ) {
        let store = Arc::new(TenantProbeForge::default());
        let svc = ForgeRegistrySvc::new(store.clone());
        let req = req_with(
            pb::ForgeListRequest::default(),
            user.as_deref(),
            session.as_deref(),
        );
        svc.list(req).await.unwrap();
        assert_eq!(store.seen.lock().unwrap().as_slice(), &[expected]);
    }

    // Two tenants over the wire hit disjoint scopes across different RPCs (list + delete).
    #[tokio::test]
    async fn positive_two_tenants_are_isolated_across_rpcs() {
        let store = Arc::new(TenantProbeForge::default());
        let svc = ForgeRegistrySvc::new(store.clone());
        svc.list(req_with(
            pb::ForgeListRequest::default(),
            Some("acme"),
            Some("s1"),
        ))
        .await
        .unwrap();
        svc.delete(req_with(
            pb::ForgeRef { id: "gh".into() },
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
