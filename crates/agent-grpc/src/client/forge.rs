//! The `Forge` and `TaskTracker` seams over the wire.
//!
//! **Failure semantic: hard, for both.** For `Forge` this matters more than
//! usual: it is the only seam that writes to the outside world, and a fabricated
//! success would tell the model its pull request was opened when it was not.
//!
//! ## Nothing that writes is retried
//!
//! `create_pr`, `comment` and `review_pr` are **not** idempotent. Retrying after
//! a lost response opens a second pull request, or posts the same comment twice,
//! or submits a duplicate review — visibly, publicly, to other people. The reads
//! retry; the writes do not.

use agent_core::{
    Comment, CreatePrRequest, Forge, Issue, Page, PullRequest, Result, ReviewVerdict, TaskTracker,
    Todo, TodoPatch,
};
use agent_proto::pb;
use async_trait::async_trait;
use tonic::transport::Channel;

use super::{call_retry, grpc_retry_policy, outbound};
use crate::transport::Endpoint;

/// A `Forge` that calls a remote `ForgeService`.
///
/// The platform credential lives on the server, so an agent can open a pull
/// request without ever holding a token that could also delete a repository.
pub struct GrpcForge {
    client: pb::forge_service_client::ForgeServiceClient<Channel>,
    retry: agent_retry::RetryPolicy,
}

impl GrpcForge {
    pub fn connect(endpoint: &Endpoint) -> Result<Self> {
        let channel = endpoint
            .connect_lazy()
            .map_err(|e| agent_core::Error::Config(e.to_string()))?;
        Ok(Self {
            client: pb::forge_service_client::ForgeServiceClient::new(channel),
            retry: grpc_retry_policy(),
        })
    }
}

fn err(s: tonic::Status) -> agent_core::Error {
    agent_core::Error::Web(s.to_string())
}

#[async_trait]
impl Forge for GrpcForge {
    fn name(&self) -> &str {
        "grpc"
    }

    async fn get_pr(&self, number: u64) -> Result<PullRequest> {
        let req = pb::ForgeNumber { number };
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req;
            async move { client.get_pr(outbound(r)).await }
        })
        .await
        .map_err(err)?;
        Ok(resp.into_inner().into())
    }

    async fn list_prs(&self, page: u32) -> Result<Page<PullRequest>> {
        let req = pb::ForgePage { page };
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req;
            async move { client.list_prs(outbound(r)).await }
        })
        .await
        .map_err(err)?
        .into_inner();
        Ok(Page {
            items: resp.items.into_iter().map(Into::into).collect(),
            next_page: resp.next_page,
        })
    }

    async fn list_issues(&self, page: u32) -> Result<Page<Issue>> {
        let req = pb::ForgePage { page };
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req;
            async move { client.list_issues(outbound(r)).await }
        })
        .await
        .map_err(err)?
        .into_inner();
        Ok(Page {
            items: resp.items.into_iter().map(Into::into).collect(),
            next_page: resp.next_page,
        })
    }

    async fn import_issue(&self, number: u64) -> Result<Issue> {
        let req = pb::ForgeNumber { number };
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req;
            async move { client.import_issue(outbound(r)).await }
        })
        .await
        .map_err(err)?;
        Ok(resp.into_inner().into())
    }

    async fn create_pr(&self, req: &CreatePrRequest) -> Result<PullRequest> {
        // NOT retried: a retry after a lost response opens a SECOND pull request.
        let mut client = self.client.clone();
        let resp = client
            .create_pr(outbound(pb::ForgeCreatePrRequest::from(req.clone())))
            .await
            .map_err(err)?;
        Ok(resp.into_inner().into())
    }

    async fn comment(&self, number: u64, body: &str) -> Result<Comment> {
        // NOT retried: a duplicate comment is visible to other people.
        let mut client = self.client.clone();
        let resp = client
            .comment(outbound(pb::ForgeCommentRequest {
                number,
                body: body.to_string(),
            }))
            .await
            .map_err(err)?;
        Ok(resp.into_inner().into())
    }

    async fn review_pr(&self, number: u64, verdict: ReviewVerdict, body: &str) -> Result<Comment> {
        // NOT retried: a duplicate review is public, and an approval is not
        // something to submit twice by accident.
        let mut client = self.client.clone();
        let resp = client
            .review_pr(outbound(pb::ForgeReviewRequest {
                number,
                verdict: pb::ForgeReviewVerdict::from(verdict) as i32,
                body: body.to_string(),
            }))
            .await
            .map_err(err)?;
        Ok(resp.into_inner().into())
    }
}

/// A `TaskTracker` that calls a remote `TaskService`.
pub struct GrpcTasks {
    client: pb::task_service_client::TaskServiceClient<Channel>,
    retry: agent_retry::RetryPolicy,
}

impl GrpcTasks {
    pub fn connect(endpoint: &Endpoint) -> Result<Self> {
        let channel = endpoint
            .connect_lazy()
            .map_err(|e| agent_core::Error::Config(e.to_string()))?;
        Ok(Self {
            client: pb::task_service_client::TaskServiceClient::new(channel),
            retry: grpc_retry_policy(),
        })
    }
}

fn terr(s: tonic::Status) -> agent_core::Error {
    agent_core::Error::Tasks(s.to_string())
}

#[async_trait]
impl TaskTracker for GrpcTasks {
    async fn write(&self, todos: Vec<Todo>) -> Result<Vec<Todo>> {
        let req = pb::TaskWriteRequest {
            todos: todos.into_iter().map(Into::into).collect(),
        };
        // Idempotent: `write` swaps the whole list, so a repeat sets the same
        // state rather than accumulating.
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.write(outbound(r)).await }
        })
        .await
        .map_err(terr)?;
        Ok(resp
            .into_inner()
            .todos
            .into_iter()
            .map(Into::into)
            .collect())
    }

    async fn update(&self, patch: TodoPatch) -> Result<Vec<Todo>> {
        let req = pb::TaskUpdateRequest::from(patch);
        // Also idempotent: a patch sets fields to given values rather than
        // incrementing anything.
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.update(outbound(r)).await }
        })
        .await
        .map_err(terr)?;
        Ok(resp
            .into_inner()
            .todos
            .into_iter()
            .map(Into::into)
            .collect())
    }

    async fn list(&self) -> Result<Vec<Todo>> {
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            async move { client.list(outbound(pb::TaskListRequest {})).await }
        })
        .await
        .map_err(terr)?;
        Ok(resp
            .into_inner()
            .todos
            .into_iter()
            .map(Into::into)
            .collect())
    }

    async fn clear(&self) -> Result<()> {
        call_retry(&self.retry, || {
            let mut client = self.client.clone();
            async move { client.clear(outbound(pb::TaskClearRequest {})).await }
        })
        .await
        .map_err(terr)?;
        Ok(())
    }
}
