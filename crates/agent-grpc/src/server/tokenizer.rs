//! The `Tokenizer` seam as a service — token counting behind gRPC.

use std::sync::Arc;

use agent_core::Tokenizer;
use agent_proto::{pb, status_from_error};
use tonic::transport::server::Router;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::Instrument;

use super::span;

pub struct TokenizerServiceSvc {
    inner: Arc<dyn Tokenizer>,
}

impl TokenizerServiceSvc {
    pub fn new(inner: Arc<dyn Tokenizer>) -> Self {
        Self { inner }
    }
    pub fn into_server(self) -> pb::tokenizer_service_server::TokenizerServiceServer<Self> {
        pb::tokenizer_service_server::TokenizerServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl pb::tokenizer_service_server::TokenizerService for TokenizerServiceSvc {
    async fn count(
        &self,
        request: Request<pb::TokCountRequest>,
    ) -> Result<Response<pb::TokCount>, Status> {
        let sp = span("tokenizer.count", request.metadata());
        let inner = self.inner.clone();
        async move {
            let req = request.into_inner();
            let tokens = inner
                .count(&req.text, &req.model)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::TokCount { tokens }))
        }
        .instrument(sp)
        .await
    }

    async fn count_messages(
        &self,
        request: Request<pb::TokCountMessagesRequest>,
    ) -> Result<Response<pb::TokCount>, Status> {
        let sp = span("tokenizer.count_messages", request.metadata());
        let inner = self.inner.clone();
        async move {
            let req = request.into_inner();
            let messages: Vec<agent_core::Message> = req
                .messages
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<_, _>>()?;
            let tokens = inner
                .count_messages(&messages, &req.model)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::TokCount { tokens }))
        }
        .instrument(sp)
        .await
    }
}

pub fn tokenizer_router(inner: Arc<dyn Tokenizer>) -> Router {
    Server::builder().add_service(TokenizerServiceSvc::new(inner).into_server())
}
