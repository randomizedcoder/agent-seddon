//! The `LlmPool` seam over the wire.
//!
//! **Failure semantic: soft.** Both methods fail soft — `health` returns an
//! empty report and `complete_all` an empty batch when the remote is
//! unreachable, mirroring the local pool: a dead pool is "nothing answered",
//! which the caller (a vote, a summary batch) already handles.

use agent_core::{
    CompletionRequest, CompletionResponse, Error, HealthReport, LlmPool, PoolMemberResult,
    PoolTier, Result,
};
use agent_proto::pb;
use async_trait::async_trait;
use tonic::transport::Channel;

use super::{call_retry, grpc_retry_policy, outbound};
use crate::transport::Endpoint;

/// An `LlmPool` that calls a remote `LlmPoolService`.
pub struct GrpcLlmPool {
    client: pb::llm_pool_service_client::LlmPoolServiceClient<Channel>,
    retry: agent_retry::RetryPolicy,
    name: String,
}

impl GrpcLlmPool {
    pub fn connect(endpoint: &Endpoint) -> Result<Self> {
        let channel = endpoint
            .connect_lazy()
            .map_err(|e| Error::Config(e.to_string()))?;
        Ok(Self {
            client: pb::llm_pool_service_client::LlmPoolServiceClient::new(channel),
            retry: grpc_retry_policy(),
            name: "grpc".to_string(),
        })
    }
}

#[async_trait]
impl LlmPool for GrpcLlmPool {
    fn name(&self) -> &str {
        &self.name
    }

    async fn health(&self) -> HealthReport {
        let mut client = self.client.clone();
        match client.health(outbound(pb::PoolHealthRequest {})).await {
            Ok(resp) => resp.into_inner().into(),
            Err(_) => HealthReport::default(),
        }
    }

    async fn complete_all(
        &self,
        req: CompletionRequest,
        tier: PoolTier,
        fanout: usize,
    ) -> Vec<PoolMemberResult> {
        let pb_req = pb::PoolCompleteRequest {
            req: Some(req.into()),
            tier: pb::PoolTier::from(tier) as i32,
            fanout: fanout.min(u32::MAX as usize) as u32,
        };
        // Idempotent read-shaped call; a retry is safe. Fail-soft to an empty
        // batch on a dead remote.
        match call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = pb_req.clone();
            async move { client.complete(outbound(r)).await }
        })
        .await
        {
            Ok(resp) => resp
                .into_inner()
                .results
                .into_iter()
                .map(Into::into)
                .collect(),
            Err(_) => Vec::new(),
        }
    }

    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse> {
        self.complete_all(req, PoolTier::Light, 1)
            .await
            .into_iter()
            .find_map(|r| r.response)
            .ok_or_else(|| Error::Provider("remote pool returned no answer".into()))
    }
}
