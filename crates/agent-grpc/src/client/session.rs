//! The `SessionStore` seam over the wire.
//!
//! **Failure semantic: hard.** A transport failure surfaces as `Err`, never as a
//! silently-missing checkpoint. Session history is the agent's undo/branch
//! substrate: a `checkpoint` that quietly no-ops loses work the user believes is
//! saved, and a `restore` that quietly returns an empty working set is worse than
//! an error — it looks like a successful restore of nothing.

use agent_core::{CheckpointDiff, CheckpointId, CheckpointMeta, Result, SessionStore, WorkingSet};
use agent_proto::pb;
use async_trait::async_trait;
use tonic::transport::Channel;

use super::{call_retry, grpc_retry_policy, outbound};
use crate::transport::Endpoint;

/// A `SessionStore` that calls a remote `SessionService`.
///
/// Checkpoints are content-addressed and immutable, so several agents pointed at
/// one store share objects and dedup for free — the property that makes this seam
/// worth distributing rather than merely possible to distribute.
pub struct GrpcSession {
    client: pb::session_service_client::SessionServiceClient<Channel>,
    retry: agent_retry::RetryPolicy,
}

impl GrpcSession {
    pub fn connect(endpoint: &Endpoint) -> Result<Self> {
        let channel = endpoint
            .connect_lazy()
            .map_err(|e| agent_core::Error::Config(e.to_string()))?;
        Ok(Self {
            client: pb::session_service_client::SessionServiceClient::new(channel),
            retry: grpc_retry_policy(),
        })
    }
}

fn err(s: tonic::Status) -> agent_core::Error {
    agent_core::Error::Session(s.to_string())
}

#[async_trait]
impl SessionStore for GrpcSession {
    async fn checkpoint(
        &self,
        session: &str,
        ws: &WorkingSet,
        label: &str,
    ) -> Result<CheckpointId> {
        let req = pb::SessionCheckpointRequest {
            session: session.to_string(),
            working: Some(ws.clone().into()),
            label: label.to_string(),
        };
        // NOT retried, despite being content-addressed. The id is a hash of
        // content + **parent** + label, and a successful append moves the head —
        // so if the response is lost after the server committed, the retry sees a
        // new parent, hashes differently, and appends a *second* node with the
        // same content rather than deduplicating onto the first. Retrying here
        // would silently double history on a flaky link.
        //
        // Dedup is real, but it is across *sessions* sharing a parent (which is
        // what makes a shared store dedup between agents), not across retries of
        // the same append.
        let mut client = self.client.clone();
        let resp = client.checkpoint(outbound(req)).await.map_err(err)?;
        Ok(resp.into_inner().id)
    }

    async fn list(&self, session: &str) -> Result<Vec<CheckpointMeta>> {
        let req = pb::SessionRef {
            session: session.to_string(),
        };
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.list(outbound(r)).await }
        })
        .await
        .map_err(err)?;
        Ok(resp
            .into_inner()
            .checkpoints
            .into_iter()
            .map(Into::into)
            .collect())
    }

    async fn restore(&self, id: &CheckpointId) -> Result<WorkingSet> {
        let req = pb::SessionCheckpointRef { id: id.clone() };
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.restore(outbound(r)).await }
        })
        .await
        .map_err(err)?;
        resp.into_inner()
            .try_into()
            .map_err(|e: agent_proto::ConvertError| agent_core::Error::Session(e.to_string()))
    }

    async fn branch(&self, session: &str, from: &CheckpointId, name: &str) -> Result<()> {
        let req = pb::SessionBranchRequest {
            session: session.to_string(),
            from: from.clone(),
            name: name.to_string(),
        };
        call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.branch(outbound(r)).await }
        })
        .await
        .map_err(err)?;
        Ok(())
    }

    async fn undo(&self, session: &str, n: u32) -> Result<CheckpointId> {
        let req = pb::SessionUndoRequest {
            session: session.to_string(),
            n,
        };
        // `undo` moves a head back `n` from the *current* head, so it is NOT
        // idempotent — a blind retry after an ambiguous failure could rewind twice.
        // One shot; the caller decides whether to retry.
        let mut client = self.client.clone();
        let resp = client.undo(outbound(req)).await.map_err(err)?;
        Ok(resp.into_inner().id)
    }

    async fn fork(&self, session: &str) -> Result<String> {
        let req = pb::SessionRef {
            session: session.to_string(),
        };
        // Also not idempotent: each fork mints a new session id, so a retry would
        // leave an orphaned one behind.
        let mut client = self.client.clone();
        let resp = client.fork(outbound(req)).await.map_err(err)?;
        Ok(resp.into_inner().session)
    }

    async fn diff(&self, a: &CheckpointId, b: &CheckpointId) -> Result<CheckpointDiff> {
        let req = pb::SessionDiffRequest {
            a: a.clone(),
            b: b.clone(),
        };
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.diff(outbound(r)).await }
        })
        .await
        .map_err(err)?;
        Ok(resp.into_inner().into())
    }

    async fn prune(&self, session: &str) -> Result<usize> {
        let req = pb::SessionRef {
            session: session.to_string(),
        };
        // Prune is idempotent in effect (unreachable nodes stay unreachable), but
        // the *count* it returns is not — a retry reports what the second pass
        // reclaimed, which may be zero. Retried anyway: converging on the right
        // state matters more than the count, which is advisory.
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.prune(outbound(r)).await }
        })
        .await
        .map_err(err)?;
        Ok(usize::try_from(resp.into_inner().reclaimed).unwrap_or(usize::MAX))
    }
}
