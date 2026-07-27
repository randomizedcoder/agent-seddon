//! The `PromptStore` seam as a service (see + CRUD every prompt).
//!
//! Fails **hard** (`Err` → `Status`): unlike the fail-soft enrichment seams, a
//! prompt read/write that fails should surface to the operator, not silently
//! degrade. Every untrusted `id` is validated inside the store (segment check +
//! `confine`), and a rejection maps to `InvalidArgument` via `status_from_error`.

use std::sync::Arc;

use agent_core::{PromptContext, PromptStore, TaskMode};
use agent_proto::{pb, status_from_error};
use tonic::transport::server::Router;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::Instrument;

use super::span;

pub struct PromptSvc {
    inner: Arc<dyn PromptStore>,
}

impl PromptSvc {
    pub fn new(inner: Arc<dyn PromptStore>) -> Self {
        Self { inner }
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
