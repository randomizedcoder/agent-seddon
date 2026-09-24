//! The `ProviderRegistry` seam as a service (model-router 03): the model
//! router's fleet + policy control plane — CRUD over upstream cards, the
//! routing policy, `Route` introspection, and live `Health`.
//!
//! Fails **hard** (`Err` → `Status`), like the prompt seam: a control-plane
//! mutation that fails should surface to the operator, not silently degrade.
//! Every id/card/policy is untrusted and validated inside the store
//! (`safe_segment`, number clamps, size/count caps); a rejection maps to
//! `InvalidArgument` — and a store's `not found` to `NotFound` — via
//! `status_from_error`. Cards carry `api_key_ref` *references*; the server
//! never resolves one (there is no key to leak).

use std::sync::Arc;

use agent_core::ProviderRegistry;
use agent_proto::{pb, status_from_error};
use tonic::transport::server::Router;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::Instrument;

use super::span;

pub struct ProviderRegistrySvc {
    inner: Arc<dyn ProviderRegistry>,
}

impl ProviderRegistrySvc {
    pub fn new(inner: Arc<dyn ProviderRegistry>) -> Self {
        Self { inner }
    }
    pub fn into_server(
        self,
    ) -> pb::provider_registry_service_server::ProviderRegistryServiceServer<Self> {
        pb::provider_registry_service_server::ProviderRegistryServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl pb::provider_registry_service_server::ProviderRegistryService for ProviderRegistrySvc {
    async fn list(
        &self,
        request: Request<pb::UpstreamListRequest>,
    ) -> Result<Response<pb::UpstreamList>, Status> {
        let key = super::identity_key(request.metadata());
        let sp = span("registry.list", request.metadata());
        let inner = self.inner.clone();
        let work = async move {
            let cards = inner.list().await.map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::UpstreamList {
                upstreams: cards.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }

    async fn get(
        &self,
        request: Request<pb::UpstreamRef>,
    ) -> Result<Response<pb::Upstream>, Status> {
        let key = super::identity_key(request.metadata());
        let sp = span("registry.get", request.metadata());
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

    async fn put(&self, request: Request<pb::Upstream>) -> Result<Response<pb::Upstream>, Status> {
        super::authz::require(
            agent_core::Action::Write,
            agent_core::ResourceType::Registry,
        )?;
        let key = super::identity_key(request.metadata());
        let sp = span("registry.put", request.metadata());
        let inner = self.inner.clone();
        let work = async move {
            // Wire → core clamps numbers; the store validates fail-closed.
            let card = agent_core::Upstream::from(request.into_inner());
            let stored = inner.put(card).await.map_err(|e| status_from_error(&e))?;
            Ok(Response::new(stored.into()))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }

    async fn delete(
        &self,
        request: Request<pb::UpstreamRef>,
    ) -> Result<Response<pb::UpstreamDeleteReply>, Status> {
        super::authz::require(
            agent_core::Action::Delete,
            agent_core::ResourceType::Registry,
        )?;
        let key = super::identity_key(request.metadata());
        let sp = span("registry.delete", request.metadata());
        let inner = self.inner.clone();
        let work = async move {
            let deleted = inner
                .delete(&request.into_inner().id)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::UpstreamDeleteReply { deleted }))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }

    async fn enable(
        &self,
        request: Request<pb::UpstreamEnableRequest>,
    ) -> Result<Response<pb::Upstream>, Status> {
        super::authz::require(
            agent_core::Action::Write,
            agent_core::ResourceType::Registry,
        )?;
        let key = super::identity_key(request.metadata());
        let sp = span("registry.enable", request.metadata());
        let inner = self.inner.clone();
        let work = async move {
            let req = request.into_inner();
            let card = inner
                .enable(&req.id, req.enabled)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(card.into()))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }

    async fn get_policy(
        &self,
        request: Request<pb::RoutePolicyRef>,
    ) -> Result<Response<pb::RoutePolicy>, Status> {
        let key = super::identity_key(request.metadata());
        let sp = span("registry.get_policy", request.metadata());
        let inner = self.inner.clone();
        let work = async move {
            let p = inner
                .get_policy()
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(p.into()))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }

    async fn put_policy(
        &self,
        request: Request<pb::RoutePolicy>,
    ) -> Result<Response<pb::RoutePolicy>, Status> {
        super::authz::require(
            agent_core::Action::Write,
            agent_core::ResourceType::Registry,
        )?;
        let key = super::identity_key(request.metadata());
        let sp = span("registry.put_policy", request.metadata());
        let inner = self.inner.clone();
        let work = async move {
            let spec = agent_core::RoutePolicySpec::from(request.into_inner());
            let stored = inner
                .put_policy(spec)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(stored.into()))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }

    async fn route(
        &self,
        request: Request<pb::RouteRequest>,
    ) -> Result<Response<pb::RouteDecision>, Status> {
        let key = super::identity_key(request.metadata());
        let sp = span("registry.route", request.metadata());
        let inner = self.inner.clone();
        let work = async move {
            // Absent hint = an all-defaults hint; wire → core sanitizes.
            let hint = agent_core::RouteHint::from(request.into_inner().hint.unwrap_or_default());
            let d = inner
                .route(&hint)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(d.into()))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }

    async fn health(
        &self,
        request: Request<pb::UpstreamHealthRequest>,
    ) -> Result<Response<pb::UpstreamHealthList>, Status> {
        let key = super::identity_key(request.metadata());
        let sp = span("registry.health", request.metadata());
        let inner = self.inner.clone();
        let work = async move {
            let entries = inner.health().await.map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::UpstreamHealthList {
                entries: entries.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }
}

pub fn provider_registry_router(inner: Arc<dyn ProviderRegistry>) -> Router {
    Server::builder().add_service(ProviderRegistrySvc::new(inner).into_server())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_proto::identity::{SESSION_ID_KEY, USER_ID_KEY};
    use pb::provider_registry_service_server::ProviderRegistryService as _;
    use rstest::rstest;
    use std::sync::Mutex;
    use tonic::metadata::MetadataValue;

    // Records the ambient tenant (`current_identity().user`) seen at each store call —
    // proves the served handler scopes the caller's wire identity into the store, so a
    // `PerTenant`-wrapped backend (C30) routes to the caller's partition (C31 Principle 3).
    #[derive(Default)]
    struct TenantProbeRegistry {
        seen: Arc<Mutex<Vec<Option<String>>>>,
    }
    impl TenantProbeRegistry {
        fn record(&self) {
            self.seen
                .lock()
                .unwrap()
                .push(agent_core::current_identity().map(|k| k.user.as_str().to_string()));
        }
    }
    #[tonic::async_trait]
    impl ProviderRegistry for TenantProbeRegistry {
        async fn list(&self) -> agent_core::Result<Vec<agent_core::Upstream>> {
            self.record();
            Ok(vec![])
        }
        async fn put(
            &self,
            card: agent_core::Upstream,
        ) -> agent_core::Result<agent_core::Upstream> {
            self.record();
            Ok(card)
        }
        async fn get(&self, _id: &str) -> agent_core::Result<agent_core::Upstream> {
            unimplemented!()
        }
        async fn delete(&self, _id: &str) -> agent_core::Result<bool> {
            unimplemented!()
        }
        async fn enable(
            &self,
            _id: &str,
            _enabled: bool,
        ) -> agent_core::Result<agent_core::Upstream> {
            unimplemented!()
        }
        async fn get_policy(&self) -> agent_core::Result<agent_core::RoutePolicySpec> {
            unimplemented!()
        }
        async fn put_policy(
            &self,
            _policy: agent_core::RoutePolicySpec,
        ) -> agent_core::Result<agent_core::RoutePolicySpec> {
            unimplemented!()
        }
        async fn route(
            &self,
            _hint: &agent_core::RouteHint,
        ) -> agent_core::Result<agent_core::RouteDecision> {
            unimplemented!()
        }
        async fn health(&self) -> agent_core::Result<Vec<agent_core::UpstreamHealth>> {
            unimplemented!()
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

    // Every caller-identity class → the tenant the store runs under. A present, path-safe
    // (user, session) scopes to that tenant; anything partial or hostile fails closed to
    // the default tenant (`None`) — never another tenant, never the hostile string.
    #[rstest]
    #[case::positive_tenant_header_scopes_to_tenant(Some("acme".into()), Some("s1".into()), Some("acme".into()))]
    #[case::negative_no_identity_runs_as_local(None, None, None)]
    #[case::negative_second_tenant_is_isolated(Some("globex".into()), Some("s1".into()), Some("globex".into()))]
    #[case::boundary_max_len_tenant_segment(Some("a".repeat(128)), Some("s1".into()), Some("a".repeat(128)))]
    #[case::corner_user_without_session_runs_as_local(Some("acme".into()), None, None)]
    #[case::adversarial_traversal_header_fails_to_local(Some("../../heads/main".into()), Some("s1".into()), None)]
    #[case::adversarial_empty_header_fails_to_local(Some(String::new()), Some("s1".into()), None)]
    #[tokio::test]
    async fn registry_list_scopes_caller_tenant(
        #[case] user: Option<String>,
        #[case] session: Option<String>,
        #[case] expected: Option<String>,
    ) {
        let store = Arc::new(TenantProbeRegistry::default());
        let svc = ProviderRegistrySvc::new(store.clone());
        let req = req_with(
            pb::UpstreamListRequest::default(),
            user.as_deref(),
            session.as_deref(),
        );
        svc.list(req).await.unwrap();
        assert_eq!(store.seen.lock().unwrap().as_slice(), &[expected]);
    }

    // The write path composes with the RBAC gate: with no verified principal `authz::require`
    // is a pass-through, and the handler still scopes the caller into the store.
    #[tokio::test]
    async fn registry_put_scopes_under_authz_passthrough() {
        let store = Arc::new(TenantProbeRegistry::default());
        let svc = ProviderRegistrySvc::new(store.clone());
        let req = req_with(pb::Upstream::default(), Some("acme"), Some("s1"));
        svc.put(req).await.unwrap();
        assert_eq!(
            store.seen.lock().unwrap().as_slice(),
            &[Some("acme".to_string())]
        );
    }
}
