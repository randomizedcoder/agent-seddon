//! The `RepoBackend` seam as a service — a multi-branch git gateway.

use std::sync::Arc;

use agent_core::RepoBackend;
use agent_proto::{pb, status_from_error};

use tonic::transport::server::Router;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::Instrument;

use super::{missing, span};

pub struct RepoServiceSvc {
    inner: Arc<dyn RepoBackend>,
}

impl RepoServiceSvc {
    pub fn new(inner: Arc<dyn RepoBackend>) -> Self {
        Self { inner }
    }
    pub fn into_server(self) -> pb::repo_service_server::RepoServiceServer<Self> {
        pb::repo_service_server::RepoServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl pb::repo_service_server::RepoService for RepoServiceSvc {
    async fn resolve(
        &self,
        request: Request<pb::ResolveRequest>,
    ) -> Result<Response<pb::ResolveResponse>, Status> {
        let sp = span("repo.resolve", request.metadata());
        let inner = self.inner.clone();
        async move {
            let rev = agent_core::Revision(request.into_inner().revision);
            let oid = inner
                .resolve(&rev)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::ResolveResponse { oid: oid.0 }))
        }
        .instrument(sp)
        .await
    }

    async fn read_file(
        &self,
        request: Request<pb::ReadFileRequest>,
    ) -> Result<Response<pb::BlobContent>, Status> {
        let sp = span("repo.read_file", request.metadata());
        let inner = self.inner.clone();
        async move {
            let req = request.into_inner();
            let rev = agent_core::Revision(req.revision);
            let blob = inner
                .read_file(&rev, std::path::Path::new(&req.path))
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(blob.into()))
        }
        .instrument(sp)
        .await
    }

    async fn list_tree(
        &self,
        request: Request<pb::ListTreeRequest>,
    ) -> Result<Response<pb::ListTreeResponse>, Status> {
        let sp = span("repo.list_tree", request.metadata());
        let inner = self.inner.clone();
        async move {
            let req = request.into_inner();
            let rev = agent_core::Revision(req.revision);
            let entries = inner
                .list_tree(&rev, std::path::Path::new(&req.path), req.recursive)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::ListTreeResponse {
                entries: entries.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp)
        .await
    }

    async fn diff(
        &self,
        request: Request<pb::DiffRequest>,
    ) -> Result<Response<pb::DiffResult>, Status> {
        let sp = span("repo.diff", request.metadata());
        let inner = self.inner.clone();
        async move {
            let req = request.into_inner();
            let base = agent_core::Revision(req.base);
            let target = agent_core::Revision(req.target);
            let result = inner
                .diff(&base, &target, &req.path_globs)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(result.into()))
        }
        .instrument(sp)
        .await
    }

    async fn grep(
        &self,
        request: Request<pb::GrepRequest>,
    ) -> Result<Response<pb::GrepResponse>, Status> {
        let sp = span("repo.grep", request.metadata());
        let inner = self.inner.clone();
        async move {
            let req = request.into_inner();
            let rev = agent_core::Revision(req.revision);
            let hits = inner
                .grep(&rev, &req.pattern, &req.path_globs, req.limit as usize)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::GrepResponse {
                hits: hits.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp)
        .await
    }

    async fn log(
        &self,
        request: Request<pb::LogRequest>,
    ) -> Result<Response<pb::LogResponse>, Status> {
        let sp = span("repo.log", request.metadata());
        let inner = self.inner.clone();
        async move {
            let req = request.into_inner();
            let rev = agent_core::Revision(req.revision);
            let path = req.path.map(std::path::PathBuf::from);
            let commits = inner
                .log(&rev, path.as_deref(), req.limit as usize)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::LogResponse {
                commits: commits.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp)
        .await
    }

    async fn branches(
        &self,
        request: Request<pb::BranchesRequest>,
    ) -> Result<Response<pb::BranchesResponse>, Status> {
        let sp = span("repo.branches", request.metadata());
        let inner = self.inner.clone();
        async move {
            let branches = inner.branches().await.map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::BranchesResponse {
                branches: branches
                    .into_iter()
                    .map(|(name, oid)| pb::Branch { name, oid: oid.0 })
                    .collect(),
            }))
        }
        .instrument(sp)
        .await
    }

    async fn status(
        &self,
        request: Request<pb::RepoStatusRequest>,
    ) -> Result<Response<pb::RepoStatus>, Status> {
        let sp = span("repo.status", request.metadata());
        let inner = self.inner.clone();
        async move {
            let status = inner.status().await.map_err(|e| status_from_error(&e))?;
            Ok(Response::new(status.into()))
        }
        .instrument(sp)
        .await
    }

    async fn fetch(
        &self,
        request: Request<pb::FetchRequest>,
    ) -> Result<Response<pb::RepoStatus>, Status> {
        let sp = span("repo.fetch", request.metadata());
        let inner = self.inner.clone();
        async move {
            let status = inner.fetch().await.map_err(|e| status_from_error(&e))?;
            Ok(Response::new(status.into()))
        }
        .instrument(sp)
        .await
    }

    async fn worktree_add(
        &self,
        request: Request<pb::WorktreeSpec>,
    ) -> Result<Response<pb::WorktreeHandle>, Status> {
        let sp = span("repo.worktree_add", request.metadata());
        let inner = self.inner.clone();
        async move {
            let spec = request.into_inner().into();
            let handle = inner
                .worktree_add(&spec)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(handle.into()))
        }
        .instrument(sp)
        .await
    }

    async fn worktree_list(
        &self,
        request: Request<pb::WorktreeListRequest>,
    ) -> Result<Response<pb::WorktreeListResponse>, Status> {
        let sp = span("repo.worktree_list", request.metadata());
        let inner = self.inner.clone();
        async move {
            let ws = inner
                .worktree_list()
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::WorktreeListResponse {
                worktrees: ws.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp)
        .await
    }

    async fn worktree_remove(
        &self,
        request: Request<pb::WorktreeRemoveRequest>,
    ) -> Result<Response<pb::WorktreeRemoveResponse>, Status> {
        let sp = span("repo.worktree_remove", request.metadata());
        let inner = self.inner.clone();
        async move {
            inner
                .worktree_remove(&request.into_inner().id)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::WorktreeRemoveResponse {}))
        }
        .instrument(sp)
        .await
    }

    async fn create_checkpoint(
        &self,
        request: Request<pb::CheckpointRequest>,
    ) -> Result<Response<pb::Checkpoint>, Status> {
        let sp = span("repo.checkpoint", request.metadata());
        let inner = self.inner.clone();
        async move {
            let req = request.into_inner();
            let cp = inner
                .checkpoint(&req.worktree_id, &req.name)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(cp.into()))
        }
        .instrument(sp)
        .await
    }

    async fn push(
        &self,
        request: Request<pb::PushRequest>,
    ) -> Result<Response<pb::PushResponse>, Status> {
        let sp = span("repo.push", request.metadata());
        let inner = self.inner.clone();
        async move {
            let req = request.into_inner();
            let checkpoint = req
                .checkpoint
                .ok_or_else(|| missing("PushRequest.checkpoint"))?;
            inner
                .push(&checkpoint.into(), &req.remote_ref)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::PushResponse {}))
        }
        .instrument(sp)
        .await
    }
}

pub fn repo_router(inner: Arc<dyn RepoBackend>) -> Router {
    Server::builder().add_service(RepoServiceSvc::new(inner).into_server())
}
