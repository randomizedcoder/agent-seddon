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
        let sp = span("transport_registry.list", request.metadata());
        let inner = self.inner.clone();
        async move {
            let cards = inner.list().await.map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::TransportList {
                transports: cards.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp)
        .await
    }

    async fn get(
        &self,
        request: Request<pb::TransportRef>,
    ) -> Result<Response<pb::TransportCard>, Status> {
        let sp = span("transport_registry.get", request.metadata());
        let inner = self.inner.clone();
        async move {
            let card = inner
                .get(&request.into_inner().id)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(card.into()))
        }
        .instrument(sp)
        .await
    }

    async fn put(
        &self,
        request: Request<pb::TransportCard>,
    ) -> Result<Response<pb::TransportCard>, Status> {
        super::authz::require(
            agent_core::Action::Write,
            agent_core::ResourceType::TransportRegistry,
        )?;
        let sp = span("transport_registry.put", request.metadata());
        let inner = self.inner.clone();
        async move {
            // Wire → core is the fail-closed boundary (unknown/absent channel purpose);
            // the store validates again (id/kind/token_refs) before it persists.
            let card = agent_core::TransportCard::try_from(request.into_inner())
                .map_err(tonic::Status::from)?;
            let stored = inner.put(card).await.map_err(|e| status_from_error(&e))?;
            Ok(Response::new(stored.into()))
        }
        .instrument(sp)
        .await
    }

    async fn delete(
        &self,
        request: Request<pb::TransportRef>,
    ) -> Result<Response<pb::TransportDeleteReply>, Status> {
        super::authz::require(
            agent_core::Action::Delete,
            agent_core::ResourceType::TransportRegistry,
        )?;
        let sp = span("transport_registry.delete", request.metadata());
        let inner = self.inner.clone();
        async move {
            let deleted = inner
                .delete(&request.into_inner().id)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::TransportDeleteReply { deleted }))
        }
        .instrument(sp)
        .await
    }
}

pub fn transport_registry_router(inner: Arc<dyn TransportRegistry>) -> Router {
    Server::builder().add_service(TransportRegistrySvc::new(inner).into_server())
}
