//! The `Policy` seam over the wire.
//!
//! Note the failure semantic: a persistent transport failure denies rather than
//! allowing, so an unreachable policy service cannot silently open the gate.

use agent_core::{Decision, Policy, Result};
use agent_proto::pb;
use async_trait::async_trait;
use tonic::transport::Channel;

use super::{call_retry, grpc_retry_policy, outbound};
use crate::transport::Endpoint;

pub struct GrpcPolicy {
    client: pb::policy_client::PolicyClient<Channel>,
    retry: agent_retry::RetryPolicy,
}

impl GrpcPolicy {
    pub fn connect(endpoint: &Endpoint) -> Result<Self> {
        let channel = endpoint
            .connect_lazy()
            .map_err(|e| agent_core::Error::Config(e.to_string()))?;
        Ok(Self {
            client: pb::policy_client::PolicyClient::new(channel),
            retry: grpc_retry_policy(),
        })
    }
}

#[async_trait]
impl Policy for GrpcPolicy {
    async fn authorize(&self, call: &agent_core::ToolCall) -> Decision {
        let pbreq = pb::ToolCall::from(call.clone());
        // Retry a transient policy-service blip before falling back; a persistent
        // failure fails safe (deny) rather than silently allowing.
        match call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = pbreq.clone();
            async move { client.authorize(outbound(r)).await }
        })
        .await
        {
            Ok(resp) => resp.into_inner().into(),
            Err(s) => Decision::Deny(format!("policy rpc failed: {s}")),
        }
    }
}
