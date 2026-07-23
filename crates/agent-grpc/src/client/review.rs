//! The `ReviewCollector` seam over the wire.
//!
//! **Failure semantic: soft-ish.** `collect` retries (a fact collection is an
//! idempotent read) and surfaces a hard error only when the remote cannot
//! produce a bundle at all — matching the local orchestrator, which always
//! assembles a (possibly partial) `ReviewFacts` unless the target is unresolvable.

use agent_core::{Error, Result, ReviewCollector, ReviewFacts, ReviewTarget};
use agent_proto::pb;
use async_trait::async_trait;
use tonic::transport::Channel;

use super::{call_retry, grpc_retry_policy, outbound};
use crate::transport::Endpoint;

/// A `ReviewCollector` that calls a remote `FactCollectorService`.
pub struct GrpcReview {
    client: pb::fact_collector_service_client::FactCollectorServiceClient<Channel>,
    retry: agent_retry::RetryPolicy,
}

impl GrpcReview {
    pub fn connect(endpoint: &Endpoint) -> Result<Self> {
        let channel = endpoint
            .connect_lazy()
            .map_err(|e| Error::Config(e.to_string()))?;
        Ok(Self {
            client: pb::fact_collector_service_client::FactCollectorServiceClient::new(channel),
            retry: grpc_retry_policy(),
        })
    }
}

/// Encode a target for the wire: `pr:<n>` | `branch:<name>` | `worktree` |
/// `revs:<base>..<head>`.
pub(crate) fn encode_target(t: &ReviewTarget) -> String {
    match t {
        ReviewTarget::Pr(n) => format!("pr:{n}"),
        ReviewTarget::Branch(b) => format!("branch:{b}"),
        ReviewTarget::WorkingTree => "worktree".to_string(),
        ReviewTarget::Revs { base, head } => format!("revs:{base}..{head}"),
    }
}

#[async_trait]
impl ReviewCollector for GrpcReview {
    fn name(&self) -> &str {
        "grpc"
    }

    async fn collect(&self, target: &ReviewTarget) -> Result<ReviewFacts> {
        let req = pb::ReviewCollectRequest {
            target: encode_target(target),
        };
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.collect(outbound(r)).await }
        })
        .await
        .map_err(|s| Error::Repo(format!("review collect: {s}")))?;
        Ok(resp.into_inner().into())
    }
}
