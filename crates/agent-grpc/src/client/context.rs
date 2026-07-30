//! The `ContextStrategy` seam over the wire.

use agent_core::{
    CompactAction, ContextInput, ContextStrategy, Message, Result, TaskMode, TokenBudget,
    WorkingSet,
};
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
        let resp = unary!(self, assemble, pbreq)
            .map_err(|s| agent_core::Error::Provider(s.to_string()))?;
        resp.into_inner()
            .messages
            .into_iter()
            .map(std::convert::TryInto::try_into)
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e: agent_proto::ConvertError| agent_core::Error::Provider(e.to_string()))
    }

    async fn compact(
        &self,
        working: &mut WorkingSet,
        budget: &TokenBudget,
        switch: Option<(TaskMode, TaskMode)>,
    ) -> Result<CompactAction> {
        // The armed switch is a caller-owned parameter — send it as the request's
        // `from_mode`/`to_mode` (adaptive-cognition 02).
        let (from_mode, to_mode) = match switch {
            Some((f, t)) => (pb::TaskMode::from(f) as i32, pb::TaskMode::from(t) as i32),
            None => (
                pb::TaskMode::Unspecified as i32,
                pb::TaskMode::Unspecified as i32,
            ),
        };
        let req = pb::CompactRequest {
            working: Some(std::mem::take(working).into()),
            budget: Some(budget.clone().into()),
            from_mode,
            to_mode,
        };
        let resp = unary!(self, compact, req)
            .map_err(|s| agent_core::Error::Provider(s.to_string()))?
            .into_inner();
        let action = resp
            .stats
            .as_ref()
            .map(|s| action_from_str(&s.action))
            .unwrap_or(CompactAction::Budget);
        let compacted = resp
            .working
            .ok_or_else(|| agent_core::Error::Provider("compact: missing working set".into()))?;
        *working = compacted
            .try_into()
            .map_err(|e: agent_proto::ConvertError| agent_core::Error::Provider(e.to_string()))?;
        Ok(action)
    }
}

/// Decode the server's `CompactStats.action` string. An unknown value (an old or
/// hostile server) degrades to `Budget` — never a panic.
fn action_from_str(s: &str) -> CompactAction {
    match s {
        "switch" => CompactAction::Switch,
        "fallback-generic" => CompactAction::FallbackGeneric,
        "fallback-drop" => CompactAction::FallbackDrop,
        _ => CompactAction::Budget,
    }
}
