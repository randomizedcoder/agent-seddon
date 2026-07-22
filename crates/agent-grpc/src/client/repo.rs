//! The `RepoBackend` seam over the wire — a multi-branch git gateway.

use agent_core::{
    BlobContent, Checkpoint, CommitInfo, DiffResult, GrepHit, Oid, RepoBackend, RepoStatus, Result,
    Revision, TreeEntry, WorktreeHandle, WorktreeSpec,
};
use agent_proto::pb;
use async_trait::async_trait;
use tonic::transport::Channel;

use super::{call_retry, grpc_retry_policy, outbound};
use crate::transport::Endpoint;

/// A `RepoBackend` that calls a remote `RepoService` (multi-branch git gateway).
pub struct GrpcRepo {
    client: pb::repo_service_client::RepoServiceClient<Channel>,
    retry: agent_retry::RetryPolicy,
}

impl GrpcRepo {
    pub fn connect(endpoint: &Endpoint) -> Result<Self> {
        let channel = endpoint
            .connect_lazy()
            .map_err(|e| agent_core::Error::Repo(e.to_string()))?;
        Ok(Self {
            client: pb::repo_service_client::RepoServiceClient::new(channel),
            retry: grpc_retry_policy(),
        })
    }
}

/// Map a transport `Status` to a repo error.
fn repo_err(s: tonic::Status) -> agent_core::Error {
    agent_core::Error::Repo(s.to_string())
}

#[async_trait]
impl RepoBackend for GrpcRepo {
    async fn resolve(&self, rev: &Revision) -> Result<Oid> {
        let req = pb::ResolveRequest {
            revision: rev.0.clone(),
        };
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.resolve(outbound(r)).await }
        })
        .await
        .map_err(repo_err)?;
        Ok(Oid(resp.into_inner().oid))
    }

    async fn read_file(&self, rev: &Revision, path: &std::path::Path) -> Result<BlobContent> {
        let req = pb::ReadFileRequest {
            revision: rev.0.clone(),
            path: path.to_string_lossy().into_owned(),
        };
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.read_file(outbound(r)).await }
        })
        .await
        .map_err(repo_err)?;
        Ok(resp.into_inner().into())
    }

    async fn list_tree(
        &self,
        rev: &Revision,
        path: &std::path::Path,
        recursive: bool,
    ) -> Result<Vec<TreeEntry>> {
        let req = pb::ListTreeRequest {
            revision: rev.0.clone(),
            path: path.to_string_lossy().into_owned(),
            recursive,
        };
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.list_tree(outbound(r)).await }
        })
        .await
        .map_err(repo_err)?;
        Ok(resp
            .into_inner()
            .entries
            .into_iter()
            .map(Into::into)
            .collect())
    }

    async fn diff(
        &self,
        base: &Revision,
        target: &Revision,
        path_globs: &[String],
    ) -> Result<DiffResult> {
        let req = pb::DiffRequest {
            base: base.0.clone(),
            target: target.0.clone(),
            path_globs: path_globs.to_vec(),
        };
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.diff(outbound(r)).await }
        })
        .await
        .map_err(repo_err)?;
        Ok(resp.into_inner().into())
    }

    async fn grep(
        &self,
        rev: &Revision,
        pattern: &str,
        path_globs: &[String],
        limit: usize,
    ) -> Result<Vec<GrepHit>> {
        let req = pb::GrepRequest {
            revision: rev.0.clone(),
            pattern: pattern.to_string(),
            path_globs: path_globs.to_vec(),
            limit: limit as u64,
        };
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.grep(outbound(r)).await }
        })
        .await
        .map_err(repo_err)?;
        Ok(resp.into_inner().hits.into_iter().map(Into::into).collect())
    }

    async fn log(
        &self,
        rev: &Revision,
        path: Option<&std::path::Path>,
        limit: usize,
    ) -> Result<Vec<CommitInfo>> {
        let req = pb::LogRequest {
            revision: rev.0.clone(),
            path: path.map(|p| p.to_string_lossy().into_owned()),
            limit: limit as u64,
        };
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.log(outbound(r)).await }
        })
        .await
        .map_err(repo_err)?;
        Ok(resp
            .into_inner()
            .commits
            .into_iter()
            .map(Into::into)
            .collect())
    }

    async fn branches(&self) -> Result<Vec<(String, Oid)>> {
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            async move { client.branches(outbound(pb::BranchesRequest {})).await }
        })
        .await
        .map_err(repo_err)?;
        Ok(resp
            .into_inner()
            .branches
            .into_iter()
            .map(|b| (b.name, Oid(b.oid)))
            .collect())
    }

    async fn status(&self) -> Result<RepoStatus> {
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            async move { client.status(outbound(pb::RepoStatusRequest {})).await }
        })
        .await
        .map_err(repo_err)?;
        Ok(resp.into_inner().into())
    }

    async fn fetch(&self) -> Result<RepoStatus> {
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            async move { client.fetch(outbound(pb::FetchRequest {})).await }
        })
        .await
        .map_err(repo_err)?;
        Ok(resp.into_inner().into())
    }

    async fn worktree_add(&self, spec: &WorktreeSpec) -> Result<WorktreeHandle> {
        let req = pb::WorktreeSpec::from(spec.clone());
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.worktree_add(outbound(r)).await }
        })
        .await
        .map_err(repo_err)?;
        Ok(resp.into_inner().into())
    }

    async fn worktree_list(&self) -> Result<Vec<WorktreeHandle>> {
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            async move {
                client
                    .worktree_list(outbound(pb::WorktreeListRequest {}))
                    .await
            }
        })
        .await
        .map_err(repo_err)?;
        Ok(resp
            .into_inner()
            .worktrees
            .into_iter()
            .map(Into::into)
            .collect())
    }

    async fn worktree_remove(&self, id: &str) -> Result<()> {
        let req = pb::WorktreeRemoveRequest { id: id.to_string() };
        call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.worktree_remove(outbound(r)).await }
        })
        .await
        .map_err(repo_err)?;
        Ok(())
    }

    async fn checkpoint(&self, worktree_id: &str, name: &str) -> Result<Checkpoint> {
        let req = pb::CheckpointRequest {
            worktree_id: worktree_id.to_string(),
            name: name.to_string(),
        };
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.create_checkpoint(outbound(r)).await }
        })
        .await
        .map_err(repo_err)?;
        Ok(resp.into_inner().into())
    }

    async fn push(&self, checkpoint: &Checkpoint, remote_ref: &str) -> Result<()> {
        let req = pb::PushRequest {
            checkpoint: Some(pb::Checkpoint::from(checkpoint.clone())),
            remote_ref: remote_ref.to_string(),
        };
        call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.push(outbound(r)).await }
        })
        .await
        .map_err(repo_err)?;
        Ok(())
    }
}
