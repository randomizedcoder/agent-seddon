//! The `Forge` and `TaskTracker` seams as services.
//!
//! # Security
//!
//! `Forge` is the only seam here that **writes to the outside world** — it opens
//! pull requests, comments, and submits reviews, using a platform credential.
//! Hosting it means that credential lives in one process rather than in every
//! agent: an agent can open a PR without ever holding a token that could also
//! delete a repository.
//!
//! The flip side is that `--serve-forge` performs authenticated writes on behalf
//! of whoever reaches it, and the transport is unauthenticated by design. Same
//! warning as sandbox/pty, different blast radius. The `Policy` gate stays on the
//! agent side; the server hosts the raw capability.

use std::sync::Arc;

use agent_core::{Forge, TaskTracker};
use agent_proto::{pb, status_from_error};
use tonic::transport::server::Router;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::Instrument;

use super::span;

pub struct ForgeServiceSvc {
    inner: Arc<dyn Forge>,
}

impl ForgeServiceSvc {
    pub fn new(inner: Arc<dyn Forge>) -> Self {
        Self { inner }
    }
    pub fn into_server(self) -> pb::forge_service_server::ForgeServiceServer<Self> {
        pb::forge_service_server::ForgeServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl pb::forge_service_server::ForgeService for ForgeServiceSvc {
    async fn get_pr(
        &self,
        request: Request<pb::ForgeNumber>,
    ) -> Result<Response<pb::ForgePullRequest>, Status> {
        let sp = span("forge.get_pr", request.metadata());
        let inner = self.inner.clone();
        async move {
            let pr = inner
                .get_pr(request.into_inner().number)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pr.into()))
        }
        .instrument(sp)
        .await
    }

    async fn list_prs(
        &self,
        request: Request<pb::ForgePage>,
    ) -> Result<Response<pb::ForgePrPage>, Status> {
        let sp = span("forge.list_prs", request.metadata());
        let inner = self.inner.clone();
        async move {
            let page = inner
                .list_prs(request.into_inner().page)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::ForgePrPage {
                items: page.items.into_iter().map(Into::into).collect(),
                next_page: page.next_page,
            }))
        }
        .instrument(sp)
        .await
    }

    async fn list_issues(
        &self,
        request: Request<pb::ForgePage>,
    ) -> Result<Response<pb::ForgeIssuePage>, Status> {
        let sp = span("forge.list_issues", request.metadata());
        let inner = self.inner.clone();
        async move {
            let page = inner
                .list_issues(request.into_inner().page)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::ForgeIssuePage {
                items: page.items.into_iter().map(Into::into).collect(),
                next_page: page.next_page,
            }))
        }
        .instrument(sp)
        .await
    }

    async fn import_issue(
        &self,
        request: Request<pb::ForgeNumber>,
    ) -> Result<Response<pb::ForgeIssue>, Status> {
        let sp = span("forge.import_issue", request.metadata());
        let inner = self.inner.clone();
        async move {
            let issue = inner
                .import_issue(request.into_inner().number)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(issue.into()))
        }
        .instrument(sp)
        .await
    }

    async fn create_pr(
        &self,
        request: Request<pb::ForgeCreatePrRequest>,
    ) -> Result<Response<pb::ForgePullRequest>, Status> {
        let sp = span("forge.create_pr", request.metadata());
        let inner = self.inner.clone();
        async move {
            let req: agent_core::CreatePrRequest = request.into_inner().into();
            let pr = inner
                .create_pr(&req)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pr.into()))
        }
        .instrument(sp)
        .await
    }

    async fn comment(
        &self,
        request: Request<pb::ForgeCommentRequest>,
    ) -> Result<Response<pb::ForgeComment>, Status> {
        let sp = span("forge.comment", request.metadata());
        let inner = self.inner.clone();
        async move {
            let req = request.into_inner();
            let c = inner
                .comment(req.number, &req.body)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(c.into()))
        }
        .instrument(sp)
        .await
    }

    async fn review_pr(
        &self,
        request: Request<pb::ForgeReviewRequest>,
    ) -> Result<Response<pb::ForgeComment>, Status> {
        let sp = span("forge.review_pr", request.metadata());
        let inner = self.inner.clone();
        async move {
            let req = request.into_inner();
            let c = inner
                .review_pr(
                    req.number,
                    agent_proto::convert::forge_verdict_from_i32(req.verdict),
                    &req.body,
                )
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(c.into()))
        }
        .instrument(sp)
        .await
    }
}

pub fn forge_router(inner: Arc<dyn Forge>) -> Router {
    Server::builder().add_service(ForgeServiceSvc::new(inner).into_server())
}

pub struct TaskServiceSvc {
    inner: Arc<dyn TaskTracker>,
}

impl TaskServiceSvc {
    pub fn new(inner: Arc<dyn TaskTracker>) -> Self {
        Self { inner }
    }
    pub fn into_server(self) -> pb::task_service_server::TaskServiceServer<Self> {
        pb::task_service_server::TaskServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl pb::task_service_server::TaskService for TaskServiceSvc {
    async fn write(
        &self,
        request: Request<pb::TaskWriteRequest>,
    ) -> Result<Response<pb::TaskList>, Status> {
        let sp = span("tasks.write", request.metadata());
        let inner = self.inner.clone();
        async move {
            let todos: Vec<agent_core::Todo> = request
                .into_inner()
                .todos
                .into_iter()
                .map(Into::into)
                .collect();
            // The at-most-one-`in_progress` invariant is enforced by the store,
            // so a rejected write leaves it unchanged and surfaces as an error.
            let out = inner
                .write(todos)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::TaskList {
                todos: out.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp)
        .await
    }

    async fn update(
        &self,
        request: Request<pb::TaskUpdateRequest>,
    ) -> Result<Response<pb::TaskList>, Status> {
        let sp = span("tasks.update", request.metadata());
        let inner = self.inner.clone();
        async move {
            let patch: agent_core::TodoPatch = request.into_inner().into();
            let out = inner
                .update(patch)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::TaskList {
                todos: out.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp)
        .await
    }

    async fn list(
        &self,
        request: Request<pb::TaskListRequest>,
    ) -> Result<Response<pb::TaskList>, Status> {
        let sp = span("tasks.list", request.metadata());
        let inner = self.inner.clone();
        async move {
            let out = inner.list().await.map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::TaskList {
                todos: out.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp)
        .await
    }

    async fn clear(
        &self,
        request: Request<pb::TaskClearRequest>,
    ) -> Result<Response<pb::TaskClearResponse>, Status> {
        let sp = span("tasks.clear", request.metadata());
        let inner = self.inner.clone();
        async move {
            inner.clear().await.map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::TaskClearResponse {}))
        }
        .instrument(sp)
        .await
    }
}

pub fn task_router(inner: Arc<dyn TaskTracker>) -> Router {
    Server::builder().add_service(TaskServiceSvc::new(inner).into_server())
}
