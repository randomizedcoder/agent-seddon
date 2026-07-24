//! The `TaskClassifier` seam as a service (general task-mode detection).

use std::sync::Arc;

use agent_core::TaskClassifier;
use agent_proto::pb;
use tonic::transport::server::Router;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::Instrument;

use super::span;

pub struct ModeSvc {
    inner: Arc<dyn TaskClassifier>,
}

impl ModeSvc {
    pub fn new(inner: Arc<dyn TaskClassifier>) -> Self {
        Self { inner }
    }
    pub fn into_server(self) -> pb::mode_service_server::ModeServiceServer<Self> {
        pb::mode_service_server::ModeServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl pb::mode_service_server::ModeService for ModeSvc {
    async fn classify(
        &self,
        request: Request<pb::ClassifyRequest>,
    ) -> Result<Response<pb::ModeVerdict>, Status> {
        let sp = span("mode.classify", request.metadata());
        let inner = self.inner.clone();
        async move {
            let req = request.into_inner();
            // Untrusted history: drop any malformed message rather than failing.
            let history: Vec<agent_core::Message> = req
                .history
                .into_iter()
                .filter_map(|m| m.try_into().ok())
                .collect();
            let verdict = inner
                .classify(&agent_core::ClassifyCtx {
                    prompt: &req.prompt,
                    history: &history,
                })
                .await;
            Ok(Response::new(verdict.into()))
        }
        .instrument(sp)
        .await
    }
}

pub fn mode_router(inner: Arc<dyn TaskClassifier>) -> Router {
    Server::builder().add_service(ModeSvc::new(inner).into_server())
}
