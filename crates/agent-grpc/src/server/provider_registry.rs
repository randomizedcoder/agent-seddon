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
        let sp = span("registry.list", request.metadata());
        let inner = self.inner.clone();
        async move {
            let cards = inner.list().await.map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::UpstreamList {
                upstreams: cards.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp)
        .await
    }

    async fn get(
        &self,
        request: Request<pb::UpstreamRef>,
    ) -> Result<Response<pb::Upstream>, Status> {
        let sp = span("registry.get", request.metadata());
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

    async fn put(&self, request: Request<pb::Upstream>) -> Result<Response<pb::Upstream>, Status> {
        let sp = span("registry.put", request.metadata());
        let inner = self.inner.clone();
        async move {
            // Wire → core clamps numbers; the store validates fail-closed.
            let card = agent_core::Upstream::from(request.into_inner());
            let stored = inner.put(card).await.map_err(|e| status_from_error(&e))?;
            Ok(Response::new(stored.into()))
        }
        .instrument(sp)
        .await
    }

    async fn delete(
        &self,
        request: Request<pb::UpstreamRef>,
    ) -> Result<Response<pb::UpstreamDeleteReply>, Status> {
        let sp = span("registry.delete", request.metadata());
        let inner = self.inner.clone();
        async move {
            let deleted = inner
                .delete(&request.into_inner().id)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::UpstreamDeleteReply { deleted }))
        }
        .instrument(sp)
        .await
    }

    async fn enable(
        &self,
        request: Request<pb::UpstreamEnableRequest>,
    ) -> Result<Response<pb::Upstream>, Status> {
        let sp = span("registry.enable", request.metadata());
        let inner = self.inner.clone();
        async move {
            let req = request.into_inner();
            let card = inner
                .enable(&req.id, req.enabled)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(card.into()))
        }
        .instrument(sp)
        .await
    }

    async fn get_policy(
        &self,
        request: Request<pb::RoutePolicyRef>,
    ) -> Result<Response<pb::RoutePolicy>, Status> {
        let sp = span("registry.get_policy", request.metadata());
        let inner = self.inner.clone();
        async move {
            let p = inner
                .get_policy()
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(p.into()))
        }
        .instrument(sp)
        .await
    }

    async fn put_policy(
        &self,
        request: Request<pb::RoutePolicy>,
    ) -> Result<Response<pb::RoutePolicy>, Status> {
        let sp = span("registry.put_policy", request.metadata());
        let inner = self.inner.clone();
        async move {
            let spec = agent_core::RoutePolicySpec::from(request.into_inner());
            let stored = inner
                .put_policy(spec)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(stored.into()))
        }
        .instrument(sp)
        .await
    }

    async fn route(
        &self,
        request: Request<pb::RouteRequest>,
    ) -> Result<Response<pb::RouteDecision>, Status> {
        let sp = span("registry.route", request.metadata());
        let inner = self.inner.clone();
        async move {
            // Absent hint = an all-defaults hint; wire → core sanitizes.
            let hint = agent_core::RouteHint::from(request.into_inner().hint.unwrap_or_default());
            let d = inner
                .route(&hint)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(d.into()))
        }
        .instrument(sp)
        .await
    }

    async fn health(
        &self,
        request: Request<pb::UpstreamHealthRequest>,
    ) -> Result<Response<pb::UpstreamHealthList>, Status> {
        let sp = span("registry.health", request.metadata());
        let inner = self.inner.clone();
        async move {
            let entries = inner.health().await.map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::UpstreamHealthList {
                entries: entries.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp)
        .await
    }
}

pub fn provider_registry_router(inner: Arc<dyn ProviderRegistry>) -> Router {
    Server::builder().add_service(ProviderRegistrySvc::new(inner).into_server())
}
