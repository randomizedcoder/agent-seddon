//! The `Policy` seam as a service — the tool-approval gate.

use std::sync::Arc;

use agent_core::Policy;
use agent_proto::pb;
use tonic::transport::server::Router;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::Instrument;

use super::span;

pub struct PolicySvc {
    inner: Arc<dyn Policy>,
}

impl PolicySvc {
    pub fn new(inner: Arc<dyn Policy>) -> Self {
        Self { inner }
    }
    pub fn into_server(self) -> pb::policy_server::PolicyServer<Self> {
        pb::policy_server::PolicyServer::new(self)
    }
}

#[tonic::async_trait]
impl pb::policy_server::Policy for PolicySvc {
    async fn authorize(
        &self,
        request: Request<pb::ToolCall>,
    ) -> Result<Response<pb::Decision>, Status> {
        let sp = span("policy.authorize", request.metadata());
        let inner = self.inner.clone();
        async move {
            let call = request.into_inner().try_into()?;
            let decision = inner.authorize(&call).await;
            Ok(Response::new(decision.into()))
        }
        .instrument(sp)
        .await
    }
}

pub fn policy_router(inner: Arc<dyn Policy>) -> Router {
    Server::builder().add_service(PolicySvc::new(inner).into_server())
}
