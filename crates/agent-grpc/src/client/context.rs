//! The `ContextStrategy` seam over the wire.

use agent_core::{ContextInput, ContextStrategy, Message, Result, TokenBudget, WorkingSet};
use agent_proto::pb;
use async_trait::async_trait;
use tonic::transport::Channel;

use super::{call_retry, grpc_retry_policy, outbound};
use crate::transport::Endpoint;

pub struct GrpcContext {
    client: pb::context_service_client::ContextServiceClient<Channel>,
    retry: agent_retry::RetryPolicy,
}

impl GrpcContext {
    pub fn connect(endpoint: &Endpoint) -> Result<Self> {
        let channel = endpoint
            .connect_lazy()
            .map_err(|e| agent_core::Error::Provider(e.to_string()))?;
        Ok(Self {
            client: pb::context_service_client::ContextServiceClient::new(channel),
            retry: grpc_retry_policy(),
        })
    }
}

#[async_trait]
impl ContextStrategy for GrpcContext {
    async fn assemble(&self, input: ContextInput) -> Result<Vec<Message>> {
        let pbreq = pb::ContextInput::from(input);
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = pbreq.clone();
            async move { client.assemble(outbound(r)).await }
        })
        .await
        .map_err(|s| agent_core::Error::Provider(s.to_string()))?;
        resp.into_inner()
            .messages
            .into_iter()
            .map(|m| m.try_into())
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e: agent_proto::ConvertError| agent_core::Error::Provider(e.to_string()))
    }

    async fn compact(&self, working: &mut WorkingSet, budget: &TokenBudget) -> Result<()> {
        let req = pb::CompactRequest {
            working: Some(std::mem::take(working).into()),
            budget: Some(budget.clone().into()),
        };
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.compact(outbound(r)).await }
        })
        .await
        .map_err(|s| agent_core::Error::Provider(s.to_string()))?;
        let compacted = resp
            .into_inner()
            .working
            .ok_or_else(|| agent_core::Error::Provider("compact: missing working set".into()))?;
        *working = compacted
            .try_into()
            .map_err(|e: agent_proto::ConvertError| agent_core::Error::Provider(e.to_string()))?;
        Ok(())
    }
}
