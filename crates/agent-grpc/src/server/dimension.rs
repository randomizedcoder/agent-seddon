//! The `DimensionStore` seam as a service (per-step dimensional memory).

use std::sync::Arc;

use agent_core::DimensionStore;
use agent_proto::pb;
use tonic::transport::server::Router;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::Instrument;

use super::span;

pub struct DimensionSvc {
    inner: Arc<dyn DimensionStore>,
}

impl DimensionSvc {
    pub fn new(inner: Arc<dyn DimensionStore>) -> Self {
        Self { inner }
    }
    pub fn into_server(self) -> pb::dimension_service_server::DimensionServiceServer<Self> {
        pb::dimension_service_server::DimensionServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl pb::dimension_service_server::DimensionService for DimensionSvc {
    async fn summarize(
        &self,
        request: Request<pb::SummarizeRequest>,
    ) -> Result<Response<pb::DimensionStep>, Status> {
        // Scope the caller's tenant so a per-user dimension store routes to *their*
        // histories (docs/design/multi-session/04-tenancy.md).
        let key = super::identity_key(request.metadata());
        let sp = span("memory.dimension.summarize", request.metadata());
        let inner = self.inner.clone();
        let work = async move {
            let req = request.into_inner();
            // Untrusted events: drop any malformed one rather than failing.
            let events: Vec<agent_core::MemoryEvent> = req
                .events
                .into_iter()
                .filter_map(|e| e.try_into().ok())
                .collect();
            // Fail-soft: the store never errors on a dead pool, but even a hard
            // error becomes an empty step so a caller degrades gracefully.
            let summaries = inner.summarize_step(&events).await.unwrap_or_default();
            Ok(Response::new(pb::DimensionStep {
                summaries: summaries.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }

    async fn recall(
        &self,
        request: Request<pb::DimensionRecallRequest>,
    ) -> Result<Response<pb::DimensionRecallResponse>, Status> {
        let key = super::identity_key(request.metadata());
        let sp = span("memory.dimension.recall", request.metadata());
        let inner = self.inner.clone();
        let work = async move {
            let req = request.into_inner();
            // The dimension is validated inside the store (untrusted path arg).
            let items = inner
                .recall_dimension(&req.dimension, req.limit as usize)
                .await
                .unwrap_or_default();
            Ok(Response::new(pb::DimensionRecallResponse {
                items: items.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }
}

pub fn dimension_router(inner: Arc<dyn DimensionStore>) -> Router {
    Server::builder().add_service(DimensionSvc::new(inner).into_server())
}
