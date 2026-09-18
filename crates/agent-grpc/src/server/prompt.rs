//! The `PromptStore` seam as a service (see + CRUD every prompt).
//!
//! Fails **hard** (`Err` → `Status`): unlike the fail-soft enrichment seams, a
//! prompt read/write that fails should surface to the operator, not silently
//! degrade. Every untrusted `id` is validated inside the store (segment check +
//! `confine`), and a rejection maps to `InvalidArgument` via `status_from_error`.

use std::sync::Arc;

use agent_core::{ActivePersonalityCell, ConfigStore, PromptContext, PromptStore, TaskMode};
use agent_proto::{pb, status_from_error};
use tonic::transport::server::Router;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::Instrument;

use super::span;

pub struct PromptSvc {
    inner: Arc<dyn PromptStore>,
    /// The live head-base cell shared with the running loop's `Settings`, when this
    /// service is co-located with an agent (`--serve-prompt` / `--serve-sessions`).
    /// `None` ⇒ the bare `prompt_router` (tests/loadtest): `SetActivePersonality` is
    /// `FAILED_PRECONDITION` (docs/design/prompts/10-portal-selector.md).
    active: Option<Arc<dyn ActivePersonalityCell>>,
    /// The config store, for the optional `persist` write-back of `[agent] personality`
    /// as the new-run default. `None` ⇒ persist is a logged no-op; the live switch still
    /// applies.
    config: Option<Arc<dyn ConfigStore>>,
}

impl PromptSvc {
    pub fn new(inner: Arc<dyn PromptStore>) -> Self {
        Self {
            inner,
            active: None,
            config: None,
        }
    }
    /// Wire the live active-personality cell (mirrors `AgentSessionSvc::with_driver`).
    pub fn with_active(mut self, active: Option<Arc<dyn ActivePersonalityCell>>) -> Self {
        self.active = active;
        self
    }
    /// Wire the config store used by `SetActivePersonality { persist }`.
    pub fn with_config(mut self, config: Option<Arc<dyn ConfigStore>>) -> Self {
        self.config = config;
        self
    }
    pub fn into_server(self) -> pb::prompt_service_server::PromptServiceServer<Self> {
        pb::prompt_service_server::PromptServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl pb::prompt_service_server::PromptService for PromptSvc {
    async fn list(
        &self,
        request: Request<pb::PromptListRequest>,
    ) -> Result<Response<pb::PromptList>, Status> {
        let sp = span("prompt.list", request.metadata());
        let inner = self.inner.clone();
        async move {
            let req = request.into_inner();
            // UNSPECIFIED / unknown tag ⇒ "every kind".
            let kind = pb::PromptKind::try_from(req.kind)
                .ok()
                .and_then(pb_kind_to_core);
            let entries = inner.list(kind).await.map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::PromptList {
                entries: entries.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp)
        .await
    }

    async fn get(
        &self,
        request: Request<pb::PromptRef>,
    ) -> Result<Response<pb::PromptEntry>, Status> {
        let sp = span("prompt.get", request.metadata());
        let inner = self.inner.clone();
        async move {
            let r = request.into_inner().try_into()?;
            let entry = inner.get(&r).await.map_err(|e| status_from_error(&e))?;
            Ok(Response::new(entry.into()))
        }
        .instrument(sp)
        .await
    }

    async fn put(
        &self,
        request: Request<pb::PromptEntry>,
    ) -> Result<Response<pb::PromptEntry>, Status> {
        super::authz::require(agent_core::Action::Write, agent_core::ResourceType::Prompt)?;
        let sp = span("prompt.put", request.metadata());
        let inner = self.inner.clone();
        async move {
            let entry = request.into_inner().try_into()?;
            let stored = inner.put(entry).await.map_err(|e| status_from_error(&e))?;
            Ok(Response::new(stored.into()))
        }
        .instrument(sp)
        .await
    }

    async fn delete(
        &self,
        request: Request<pb::PromptRef>,
    ) -> Result<Response<pb::DeleteReply>, Status> {
        super::authz::require(agent_core::Action::Delete, agent_core::ResourceType::Prompt)?;
        let sp = span("prompt.delete", request.metadata());
        let inner = self.inner.clone();
        async move {
            let r = request.into_inner().try_into()?;
            let deleted = inner.delete(&r).await.map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::DeleteReply { deleted }))
        }
        .instrument(sp)
        .await
    }

    async fn select(
        &self,
        request: Request<pb::PromptContext>,
    ) -> Result<Response<pb::PromptList>, Status> {
        let sp = span("prompt.select", request.metadata());
        let inner = self.inner.clone();
        async move {
            let ctx = PromptContext::from(request.into_inner());
            let entries = inner
                .select(&ctx)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::PromptList {
                entries: entries.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp)
        .await
    }

    async fn preview_assembled(
        &self,
        request: Request<pb::PreviewRequest>,
    ) -> Result<Response<pb::AssembledContext>, Status> {
        let sp = span("prompt.preview_assembled", request.metadata());
        let inner = self.inner.clone();
        async move {
            let req = request.into_inner();
            // Prefer the explicit tag set; fall back to a `mode:<mode>` tag from the
            // pre-04 scalar `mode` field (empty/unknown ⇒ Other) so old clients work.
            let ctx = match req.context {
                Some(c) if !c.tags.is_empty() => PromptContext::from(c),
                _ => {
                    let mode = TaskMode::parse(&req.mode).unwrap_or_default();
                    PromptContext::new().with_tag(format!("mode:{}", mode.as_str()))
                }
            };
            let messages = inner
                .preview_assembled(&ctx, &req.goal)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::AssembledContext {
                messages: messages.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp)
        .await
    }

    async fn get_active_personality(
        &self,
        request: Request<pb::GetActivePersonalityRequest>,
    ) -> Result<Response<pb::ActivePersonality>, Status> {
        let sp = span("prompt.get_active_personality", request.metadata());
        // No cell wired (the bare router) ⇒ report the default; a read is always safe.
        let id = self.active.as_ref().map(|c| c.id()).unwrap_or_default();
        async move { Ok(Response::new(pb::ActivePersonality { id })) }
            .instrument(sp)
            .await
    }

    async fn set_active_personality(
        &self,
        request: Request<pb::SetActivePersonalityRequest>,
    ) -> Result<Response<pb::ActivePersonality>, Status> {
        super::authz::require(agent_core::Action::Write, agent_core::ResourceType::Prompt)?;
        let sp = span("prompt.set_active_personality", request.metadata());
        let active = self.active.clone();
        let config = self.config.clone();
        async move {
            let req = request.into_inner();
            // Only meaningful with a running loop to re-resolve against; the bare
            // router (no cell) reports the switch unavailable rather than silently
            // succeeding with no effect.
            let cell = active.ok_or_else(|| {
                Status::failed_precondition(
                    "active personality unavailable: no running agent bound to this service",
                )
            })?;
            // Closed-set validation lives in the cell; a bad id ⇒ InvalidArgument.
            cell.set(&req.id).map_err(|e| status_from_error(&e))?;
            let id = cell.id();
            // Best-effort persist of the new default; never fails the live switch.
            if req.persist {
                match &config {
                    Some(store) => {
                        let edit = agent_core::ConfigEdit {
                            path: "agent.personality".to_string(),
                            value: Some(serde_json::Value::String(id.clone())),
                        };
                        match store.put(vec![edit]).await {
                            Ok(issues) if !issues.is_empty() => tracing::warn!(
                                count = issues.len(),
                                "persist of active personality rejected by config store"
                            ),
                            Ok(_) => {}
                            Err(e) => {
                                tracing::warn!(error = %e, "persist of active personality failed");
                            }
                        }
                    }
                    None => tracing::warn!("persist requested but no config store wired"),
                }
            }
            Ok(Response::new(pb::ActivePersonality { id }))
        }
        .instrument(sp)
        .await
    }
}

/// Map a wire `PromptKind` to the core enum, or `None` for the "all kinds" sentinel.
fn pb_kind_to_core(k: pb::PromptKind) -> Option<agent_core::PromptKind> {
    match k {
        pb::PromptKind::Unspecified => None,
        pb::PromptKind::System => Some(agent_core::PromptKind::System),
        pb::PromptKind::Prepend => Some(agent_core::PromptKind::Prepend),
        pb::PromptKind::Append => Some(agent_core::PromptKind::Append),
        pb::PromptKind::ModeLens => Some(agent_core::PromptKind::ModeLens),
        pb::PromptKind::SystemFragment => Some(agent_core::PromptKind::SystemFragment),
    }
}

pub fn prompt_router(inner: Arc<dyn PromptStore>) -> Router {
    Server::builder().add_service(PromptSvc::new(inner).into_server())
}
