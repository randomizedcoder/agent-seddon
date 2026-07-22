//! The `LlmProvider` seam over the wire.

use agent_core::{
    ChunkStream, CompletionRequest, CompletionResponse, LlmProvider, ModelCapabilities, Result,
};
use agent_proto::pb;
use async_trait::async_trait;
use futures_util::StreamExt;
use tonic::transport::Channel;

use super::{call_retry, grpc_retry_policy, outbound};
use crate::transport::Endpoint;

pub struct GrpcProvider {
    client: pb::provider_client::ProviderClient<Channel>,
    caps: ModelCapabilities,
    retry: agent_retry::RetryPolicy,
}

impl GrpcProvider {
    /// Connect lazily. `caps` is config-derived so `capabilities()` (a sync trait
    /// method) needs no round-trip and the factory stays synchronous.
    pub fn connect(endpoint: &Endpoint, caps: ModelCapabilities) -> Result<Self> {
        let channel = endpoint
            .connect_lazy()
            .map_err(|e| agent_core::Error::Provider(e.to_string()))?;
        Ok(Self {
            client: pb::provider_client::ProviderClient::new(channel),
            caps,
            retry: grpc_retry_policy(),
        })
    }
}

#[async_trait]
impl LlmProvider for GrpcProvider {
    fn capabilities(&self) -> ModelCapabilities {
        self.caps.clone()
    }

    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse> {
        let pbreq = pb::CompletionRequest::from(req);
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = pbreq.clone();
            async move { client.complete(outbound(r)).await }
        })
        .await
        .map_err(|s| agent_core::Error::Provider(s.to_string()))?;
        resp.into_inner()
            .try_into()
            .map_err(|e: agent_proto::ConvertError| agent_core::Error::Provider(e.to_string()))
    }

    async fn stream(&self, req: CompletionRequest) -> Result<ChunkStream> {
        let mut client = self.client.clone();
        let stream = client
            .stream(outbound(pb::CompletionRequest::from(req)))
            .await
            .map_err(|s| agent_core::Error::Provider(s.to_string()))?
            .into_inner();
        let mapped = stream.map(|item| match item {
            Ok(chunk) => agent_core::CompletionChunk::try_from(chunk)
                .map_err(|e| agent_core::Error::Provider(e.to_string())),
            Err(s) => Err(agent_core::Error::Provider(s.to_string())),
        });
        Ok(Box::pin(mapped))
    }
}
