//! The `TransportRegistryService` seam (config design C37 / increment D2): the
//! message-transport control plane — CRUD over the persisted **transport cards** the
//! fleet's trigger watch + progress feed build their messaging clients from, on top
//! of the built-in transport impls. This is the *config-card registry* for
//! transports; the messaging *capability* seam is `agent_core::MessageTransport`, the
//! way `ForgeRegistryService` (cards) relates to the `Forge` seam.
//!
//! Fails **hard** (`Err` → `Status`), like the other control-plane seams: a failed
//! mutation surfaces to the operator, never silently degrades. Every id, kind,
//! `*_token_ref`, `endpoint`, and channel `purpose` is untrusted and validated inside
//! the store and the wire→core boundary (`safe_segment`, `ApiKeyRef::parse`,
//! `ChannelPurpose::parse`) — a rejection maps to `InvalidArgument`, and a store's
//! `not found` to `NotFound`, via `status_from_error`. Mutations are gated by the RBAC
//! enforcement core (C1): a caller needs a role granting `(write|delete,
//! transport_registry)`.
//!
//! Every RPC also scopes the caller's ambient identity (`identity_key` +
//! `run_scoped`, mirroring `prompt.rs`) before touching the store, so a
//! `PerTenant<dyn TransportRegistry>`-wrapped registry (`agent-runtime/src/tenant.rs`)
//! routes a standalone `--serve-transport-registry` caller to *their* tenant's
//! cards rather than the default `local` set. Identity is attacker-controlled and
//! fails **closed** to `None` → the `local` tenant (see `server::identity_key`).

use std::sync::Arc;

use agent_core::TransportRegistry;
use agent_proto::{pb, status_from_error};
use tonic::transport::server::Router;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::Instrument;

use super::span;

pub struct TransportRegistrySvc {
    inner: Arc<dyn TransportRegistry>,
}

impl TransportRegistrySvc {
    pub fn new(inner: Arc<dyn TransportRegistry>) -> Self {
        Self { inner }
    }
    pub fn into_server(
        self,
    ) -> pb::transport_registry_service_server::TransportRegistryServiceServer<Self> {
        pb::transport_registry_service_server::TransportRegistryServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl pb::transport_registry_service_server::TransportRegistryService for TransportRegistrySvc {
    async fn list(
        &self,
        request: Request<pb::TransportListRequest>,
    ) -> Result<Response<pb::TransportList>, Status> {
        let key = super::identity_key(request.metadata());
        let sp = span("transport_registry.list", request.metadata());
        let inner = self.inner.clone();
        let work = async move {
            let cards = inner.list().await.map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::TransportList {
                transports: cards.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }

    async fn get(
        &self,
        request: Request<pb::TransportRef>,
    ) -> Result<Response<pb::TransportCard>, Status> {
        let key = super::identity_key(request.metadata());
        let sp = span("transport_registry.get", request.metadata());
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
        request: Request<pb::TransportCard>,
    ) -> Result<Response<pb::TransportCard>, Status> {
        super::authz::require(
            agent_core::Action::Write,
            agent_core::ResourceType::TransportRegistry,
        )?;
        let key = super::identity_key(request.metadata());
        let sp = span("transport_registry.put", request.metadata());
        let inner = self.inner.clone();
        let work = async move {
            // Wire → core is the fail-closed boundary (unknown/absent channel purpose);
            // the store validates again (id/kind/token_refs) before it persists.
            let card = agent_core::TransportCard::try_from(request.into_inner())
                .map_err(tonic::Status::from)?;
            let stored = inner.put(card).await.map_err(|e| status_from_error(&e))?;
            Ok(Response::new(stored.into()))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }

    async fn delete(
        &self,
        request: Request<pb::TransportRef>,
    ) -> Result<Response<pb::TransportDeleteReply>, Status> {
        super::authz::require(
            agent_core::Action::Delete,
            agent_core::ResourceType::TransportRegistry,
        )?;
        let key = super::identity_key(request.metadata());
        let sp = span("transport_registry.delete", request.metadata());
        let inner = self.inner.clone();
        let work = async move {
            let deleted = inner
                .delete(&request.into_inner().id)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::TransportDeleteReply { deleted }))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }
}

pub fn transport_registry_router(inner: Arc<dyn TransportRegistry>) -> Router {
    Server::builder().add_service(TransportRegistrySvc::new(inner).into_server())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::TransportCard;
    use agent_proto::identity::{SESSION_ID_KEY, USER_ID_KEY};
    use pb::transport_registry_service_server::TransportRegistryService as _;
    use rstest::rstest;
    use std::sync::Mutex;
    use tonic::metadata::MetadataValue;

    // Records the ambient tenant (`current_identity().user`) at each store call — proves the
    // served handler scopes the caller into a `PerTenant`-wrapped registry (C31 Principle 3).
    #[derive(Default)]
    struct TenantProbeTransport {
        seen: Arc<Mutex<Vec<Option<String>>>>,
    }
    impl TenantProbeTransport {
        fn record(&self) {
            self.seen
                .lock()
                .unwrap()
                .push(agent_core::current_identity().map(|k| k.user.as_str().to_string()));
        }
    }
    #[tonic::async_trait]
    impl TransportRegistry for TenantProbeTransport {
        async fn list(&self) -> agent_core::Result<Vec<TransportCard>> {
            self.record();
            Ok(vec![])
        }
        async fn get(&self, id: &str) -> agent_core::Result<TransportCard> {
            self.record();
            Err(agent_core::Error::Fleet(format!(
                "no transport card {id:?}"
            )))
        }
        async fn put(&self, card: TransportCard) -> agent_core::Result<TransportCard> {
            self.record();
            Ok(card)
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
    async fn transport_list_scopes_caller_tenant(
        #[case] user: Option<String>,
        #[case] session: Option<String>,
        #[case] expected: Option<String>,
    ) {
        let store = Arc::new(TenantProbeTransport::default());
        let svc = TransportRegistrySvc::new(store.clone());
        let req = req_with(
            pb::TransportListRequest::default(),
            user.as_deref(),
            session.as_deref(),
        );
        svc.list(req).await.unwrap();
        assert_eq!(store.seen.lock().unwrap().as_slice(), &[expected]);
    }

    // Two tenants over the wire hit disjoint scopes across different RPCs (list + delete).
    #[tokio::test]
    async fn positive_two_tenants_are_isolated_across_rpcs() {
        let store = Arc::new(TenantProbeTransport::default());
        let svc = TransportRegistrySvc::new(store.clone());
        svc.list(req_with(
            pb::TransportListRequest::default(),
            Some("acme"),
            Some("s1"),
        ))
        .await
        .unwrap();
        svc.delete(req_with(
            pb::TransportRef { id: "slack".into() },
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
