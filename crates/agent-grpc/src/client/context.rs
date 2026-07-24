//! The `ContextStrategy` seam over the wire.

use std::sync::Mutex;

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
    /// A switch armed by `on_mode_switch`, sent on the next `compact` as the
    /// request's `from_mode`/`to_mode` (adaptive-cognition 02). Interior because
    /// the seam methods take `&self`. Never held across an `.await`.
    pending: Mutex<Option<(TaskMode, TaskMode)>>,
    /// Last compaction's action, decoded from the server's `CompactStats`.
    last_action: Mutex<CompactAction>,
}

impl GrpcContext {
    pub fn connect(endpoint: &Endpoint) -> Result<Self> {
        let channel = endpoint
            .connect_lazy()
            .map_err(|e| agent_core::Error::Provider(e.to_string()))?;
        Ok(Self {
            client: pb::context_service_client::ContextServiceClient::new(channel),
            retry: grpc_retry_policy(),
            pending: Mutex::new(None),
            last_action: Mutex::new(CompactAction::Budget),
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
        // Take the armed switch (if any) without holding the lock across the await.
        let (from_mode, to_mode) = match self.pending.lock().expect("pending poisoned").take() {
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
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.compact(outbound(r)).await }
        })
        .await
        .map_err(|s| agent_core::Error::Provider(s.to_string()))?
        .into_inner();
        if let Some(stats) = &resp.stats {
            *self.last_action.lock().expect("last_action poisoned") =
                action_from_str(&stats.action);
        }
        let compacted = resp
            .working
            .ok_or_else(|| agent_core::Error::Provider("compact: missing working set".into()))?;
        *working = compacted
            .try_into()
            .map_err(|e: agent_proto::ConvertError| agent_core::Error::Provider(e.to_string()))?;
        Ok(())
    }

    fn on_mode_switch(&self, from: TaskMode, to: TaskMode) {
        *self.pending.lock().expect("pending poisoned") = Some((from, to));
    }

    fn last_compact_action(&self) -> CompactAction {
        *self.last_action.lock().expect("last_action poisoned")
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
