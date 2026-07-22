//! The `ReferenceResolver` seam as a service — `@`-mention expansion behind gRPC.

use std::sync::Arc;

use agent_core::ReferenceResolver;
use agent_proto::pb;
use tonic::transport::server::Router;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::Instrument;

use super::span;

pub struct ReferenceServiceSvc {
    inner: Arc<dyn ReferenceResolver>,
}

impl ReferenceServiceSvc {
    pub fn new(inner: Arc<dyn ReferenceResolver>) -> Self {
        Self { inner }
    }
    pub fn into_server(self) -> pb::reference_service_server::ReferenceServiceServer<Self> {
        pb::reference_service_server::ReferenceServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl pb::reference_service_server::ReferenceService for ReferenceServiceSvc {
    async fn resolve(
        &self,
        request: Request<pb::RefResolveRequest>,
    ) -> Result<Response<pb::RefResolution>, Status> {
        let sp = span("reference.resolve", request.metadata());
        let inner = self.inner.clone();
        async move {
            let req = request.into_inner();
            // `resolve` cannot fail by construction — a bad reference becomes a
            // warning — so there is no error path to map.
            let budget = usize::try_from(req.budget_tokens).unwrap_or(usize::MAX);
            let res = inner.resolve(&req.prompt, budget).await;
            Ok(Response::new(res.into()))
        }
        .instrument(sp)
        .await
    }
}

pub fn reference_router(inner: Arc<dyn ReferenceResolver>) -> Router {
    Server::builder().add_service(ReferenceServiceSvc::new(inner).into_server())
}
