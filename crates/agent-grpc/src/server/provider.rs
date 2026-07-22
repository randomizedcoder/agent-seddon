//! The `LlmProvider` seam as a service.

use std::pin::Pin;
use std::sync::Arc;

use agent_core::LlmProvider;
use agent_proto::{pb, status_from_error};
use futures_util::{Stream, StreamExt};
use tonic::transport::server::Router;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::Instrument;

use super::span;

pub struct ProviderService {
    inner: Arc<dyn LlmProvider>,
}

impl ProviderService {
    pub fn new(inner: Arc<dyn LlmProvider>) -> Self {
        Self { inner }
    }
    pub fn into_server(self) -> pb::provider_server::ProviderServer<Self> {
        pb::provider_server::ProviderServer::new(self)
    }
}

#[tonic::async_trait]
impl pb::provider_server::Provider for ProviderService {
    async fn capabilities(
        &self,
        request: Request<pb::CapabilitiesRequest>,
    ) -> Result<Response<pb::ModelCapabilities>, Status> {
        let sp = span("provider.capabilities", request.metadata());
        let inner = self.inner.clone();
        async move { Ok(Response::new(inner.capabilities().into())) }
            .instrument(sp)
            .await
    }

    async fn complete(
        &self,
        request: Request<pb::CompletionRequest>,
    ) -> Result<Response<pb::CompletionResponse>, Status> {
        let sp = span("provider.complete", request.metadata());
        let inner = self.inner.clone();
        async move {
            let req = request.into_inner().try_into()?;
            let resp = inner
                .complete(req)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(resp.into()))
        }
        .instrument(sp)
        .await
    }

    type StreamStream = Pin<Box<dyn Stream<Item = Result<pb::CompletionChunk, Status>> + Send>>;

    // `tonic::Status` is a large Err type, but the stream item type is fixed by the
    // generated trait — boxing it would defeat the point.
    #[allow(clippy::result_large_err)]
    async fn stream(
        &self,
        request: Request<pb::CompletionRequest>,
    ) -> Result<Response<Self::StreamStream>, Status> {
        let sp = span("provider.stream", request.metadata());
        let inner = self.inner.clone();
        async move {
            let req = request.into_inner().try_into()?;
            let chunks = inner.stream(req).await.map_err(|e| status_from_error(&e))?;
            let mapped = chunks.map(|item| {
                item.map(pb::CompletionChunk::from)
                    .map_err(|e| status_from_error(&e))
            });
            Ok(Response::new(Box::pin(mapped) as Self::StreamStream))
        }
        .instrument(sp)
        .await
    }
}

pub fn provider_router(inner: Arc<dyn LlmProvider>) -> Router {
    Server::builder().add_service(ProviderService::new(inner).into_server())
}
