//! The `ContextStrategy` seam as a service.

use std::sync::Arc;

use agent_core::ContextStrategy;
use agent_proto::{pb, status_from_error};
use tonic::transport::server::Router;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::Instrument;

use super::{missing, span};

pub struct ContextSvc {
    inner: Arc<dyn ContextStrategy>,
}

impl ContextSvc {
    pub fn new(inner: Arc<dyn ContextStrategy>) -> Self {
        Self { inner }
    }
    pub fn into_server(self) -> pb::context_service_server::ContextServiceServer<Self> {
        pb::context_service_server::ContextServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl pb::context_service_server::ContextService for ContextSvc {
    async fn assemble(
        &self,
        request: Request<pb::ContextInput>,
    ) -> Result<Response<pb::AssembleResponse>, Status> {
        let sp = span("context.assemble", request.metadata());
        let inner = self.inner.clone();
        async move {
            let input = request.into_inner().into();
            let messages = inner
                .assemble(input)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::AssembleResponse {
                messages: messages.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp)
        .await
    }

    async fn compact(
        &self,
        request: Request<pb::CompactRequest>,
    ) -> Result<Response<pb::CompactResponse>, Status> {
        let sp = span("context.compact", request.metadata());
        let inner = self.inner.clone();
        async move {
            let req = request.into_inner();
            let mut working = req
                .working
                .ok_or_else(|| missing("CompactRequest.working"))?
                .try_into()?;
            let budget = req
                .budget
                .ok_or_else(|| missing("CompactRequest.budget"))?
                .into();
            inner
                .compact(&mut working, &budget)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::CompactResponse {
                working: Some(working.into()),
            }))
        }
        .instrument(sp)
        .await
    }
}

pub fn context_router(inner: Arc<dyn ContextStrategy>) -> Router {
    Server::builder().add_service(ContextSvc::new(inner).into_server())
}
