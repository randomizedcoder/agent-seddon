//! The `ContextStrategy` seam as a service.

use std::sync::Arc;

use agent_core::ContextStrategy;
use agent_proto::{pb, status_from_error};
use tonic::transport::server::Router;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::Instrument;

use super::{missing, span};

pub struct ContextSvc {
    inner: Arc<dyn ContextStrategy>,
}

impl ContextSvc {
    pub fn new(inner: Arc<dyn ContextStrategy>) -> Self {
        Self { inner }
    }
    pub fn into_server(self) -> pb::context_service_server::ContextServiceServer<Self> {
        pb::context_service_server::ContextServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl pb::context_service_server::ContextService for ContextSvc {
    async fn assemble(
        &self,
        request: Request<pb::ContextInput>,
    ) -> Result<Response<pb::AssembleResponse>, Status> {
        let sp = span("context.assemble", request.metadata());
        let inner = self.inner.clone();
        async move {
            let input = request.into_inner().into();
            let messages = inner
                .assemble(input)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::AssembleResponse {
                messages: messages.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp)
        .await
    }

    async fn compact(
        &self,
        request: Request<pb::CompactRequest>,
    ) -> Result<Response<pb::CompactResponse>, Status> {
        let sp = span("context.compact", request.metadata());
        let inner = self.inner.clone();
        async move {
            let req = request.into_inner();
            // Mode-aware compaction (adaptive-cognition 02): a switch is requested
            // when `to_mode` is set (non-UNSPECIFIED) and differs from `from_mode`.
            // Untrusted enum ints decode to the default (UNSPECIFIED→Other), so a
            // hostile value can at worst request an ordinary or same-mode compaction.
            let from: agent_core::TaskMode = req.from_mode().into();
            let to_pb = req.to_mode();
            let to: agent_core::TaskMode = to_pb.into();
            let mut working: agent_core::WorkingSet = req
                .working
                .ok_or_else(|| missing("CompactRequest.working"))?
                .try_into()?;
            let budget: agent_core::TokenBudget = req
                .budget
                .ok_or_else(|| missing("CompactRequest.budget"))?
                .into();
            let switch = if to_pb != pb::TaskMode::Unspecified && from != to {
                Some((from, to))
            } else {
                None
            };
            let before = rough_tokens(&working.messages);
            // A compaction never errors on a dead summarizer — it falls back
            // internally — so a real error here is a transport/decode fault.
            let action = inner
                .compact(&mut working, &budget, switch)
                .await
                .map_err(|e| status_from_error(&e))?;
            let after = rough_tokens(&working.messages);
            let stats = pb::CompactStats {
                kept_tokens: after,
                shed_tokens: before.saturating_sub(after),
                action: action_str(action).to_string(),
            };
            Ok(Response::new(pb::CompactResponse {
                working: Some(working.into()),
                stats: Some(stats),
            }))
        }
        .instrument(sp)
        .await
    }
}

/// Stable wire label for a `CompactAction` (matches the proto doc + metrics).
fn action_str(a: agent_core::CompactAction) -> &'static str {
    match a {
        agent_core::CompactAction::Budget => "budget",
        agent_core::CompactAction::Switch => "switch",
        agent_core::CompactAction::FallbackGeneric => "fallback-generic",
        agent_core::CompactAction::FallbackDrop => "fallback-drop",
    }
}

/// Rough token estimate (~4 chars/token + per-message tax), mirroring
/// `agent_context::estimate_tokens` — kept local so the server needs nothing from
/// the context crate. Only used to fill `CompactStats`.
fn rough_tokens(messages: &[agent_core::Message]) -> u32 {
    let mut tokens = 0u32;
    for m in messages {
        let mut chars = 0usize;
        for b in &m.content {
            if let agent_core::ContentBlock::Text { text } = b {
                chars += text.len();
            }
        }
        for tc in &m.tool_calls {
            chars += tc.name.len() + tc.arguments.to_string().len();
        }
        chars += 8;
        tokens = tokens.saturating_add((chars / 4) as u32);
    }
    tokens
}

pub fn context_router(inner: Arc<dyn ContextStrategy>) -> Router {
    Server::builder().add_service(ContextSvc::new(inner).into_server())
}
