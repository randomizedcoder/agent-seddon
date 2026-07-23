//! The `LlmPool` seam as a service — health-checked, tiered, fan-out pool.

use std::sync::Arc;

use agent_core::LlmPool;
use agent_proto::{convert::pool_tier_from_i32, pb};
use tonic::transport::server::Router;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::Instrument;

use super::{missing, span};

pub struct LlmPoolServiceSvc {
    inner: Arc<dyn LlmPool>,
}

impl LlmPoolServiceSvc {
    pub fn new(inner: Arc<dyn LlmPool>) -> Self {
        Self { inner }
    }
    pub fn into_server(self) -> pb::llm_pool_service_server::LlmPoolServiceServer<Self> {
        pb::llm_pool_service_server::LlmPoolServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl pb::llm_pool_service_server::LlmPoolService for LlmPoolServiceSvc {
    async fn health(
        &self,
        request: Request<pb::PoolHealthRequest>,
    ) -> Result<Response<pb::PoolHealthReport>, Status> {
        let sp = span("llm_pool.health", request.metadata());
        let inner = self.inner.clone();
        async move { Ok(Response::new(inner.health().await.into())) }
            .instrument(sp)
            .await
    }

    async fn complete(
        &self,
        request: Request<pb::PoolCompleteRequest>,
    ) -> Result<Response<pb::PoolCompleteResponse>, Status> {
        let sp = span("llm_pool.complete", request.metadata());
        let inner = self.inner.clone();
        async move {
            let req = request.into_inner();
            let core_req: agent_core::CompletionRequest =
                req.req.ok_or_else(|| missing("req"))?.try_into()?;
            let tier = pool_tier_from_i32(req.tier);
            // The fan-out is clamped by the pool; a hostile `fanout` cannot make
            // it spawn unboundedly.
            let results = inner
                .complete_all(core_req, tier, req.fanout as usize)
                .await;
            Ok(Response::new(pb::PoolCompleteResponse {
                results: results.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp)
        .await
    }
}

pub fn llm_pool_router(inner: Arc<dyn LlmPool>) -> Router {
    Server::builder().add_service(LlmPoolServiceSvc::new(inner).into_server())
}
