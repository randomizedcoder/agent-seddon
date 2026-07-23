//! The `ReviewCollector` seam as a service — grounded fact collection.
//!
//! > **It executes local git.** Like `--serve-repo`, the collector shells out to
//! > git on the host it runs on; the socket's permissions are the access control
//! > (see docs/grpc.md). It does not run model-authored code.

use std::sync::Arc;

use agent_core::{ReviewCollector, ReviewTarget};
use agent_proto::{pb, status_from_error};
use tonic::transport::server::Router;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::Instrument;

use super::span;

pub struct FactCollectorServiceSvc {
    inner: Arc<dyn ReviewCollector>,
}

impl FactCollectorServiceSvc {
    pub fn new(inner: Arc<dyn ReviewCollector>) -> Self {
        Self { inner }
    }
    pub fn into_server(
        self,
    ) -> pb::fact_collector_service_server::FactCollectorServiceServer<Self> {
        pb::fact_collector_service_server::FactCollectorServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl pb::fact_collector_service_server::FactCollectorService for FactCollectorServiceSvc {
    async fn collect(
        &self,
        request: Request<pb::ReviewCollectRequest>,
    ) -> Result<Response<pb::ReviewFacts>, Status> {
        let sp = span("review.collect", request.metadata());
        let inner = self.inner.clone();
        async move {
            let target =
                decode_target(&request.into_inner().target).map_err(Status::invalid_argument)?;
            let facts = inner
                .collect(&target)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(facts.into()))
        }
        .instrument(sp)
        .await
    }
}

/// Decode the wire target: `pr:<n>` | `branch:<name>` | `worktree`. The value is
/// attacker-controlled — a bad number or an unknown form is rejected, and an
/// unsafe branch name is caught downstream by the orchestrator's `safe_segment`.
pub(crate) fn decode_target(s: &str) -> Result<ReviewTarget, String> {
    if s == "worktree" {
        return Ok(ReviewTarget::WorkingTree);
    }
    if let Some(n) = s.strip_prefix("pr:") {
        return n
            .parse::<u64>()
            .map(ReviewTarget::Pr)
            .map_err(|_| format!("invalid PR number `{n}`"));
    }
    if let Some(b) = s.strip_prefix("branch:") {
        return Ok(ReviewTarget::Branch(b.to_string()));
    }
    if let Some(range) = s.strip_prefix("revs:") {
        let (base, head) = range
            .split_once("..")
            .ok_or_else(|| format!("malformed revs target `{s}` (want `revs:<base>..<head>`)"))?;
        return Ok(ReviewTarget::Revs {
            base: base.to_string(),
            head: head.to_string(),
        });
    }
    Err(format!("unrecognized review target `{s}`"))
}

pub fn review_router(inner: Arc<dyn ReviewCollector>) -> Router {
    Server::builder().add_service(FactCollectorServiceSvc::new(inner).into_server())
}

#[cfg(test)]
mod tests {
    use super::decode_target;
    use agent_core::ReviewTarget;
    use rstest::rstest;

    #[rstest]
    #[case::worktree("worktree", ReviewTarget::WorkingTree)]
    #[case::pr("pr:42", ReviewTarget::Pr(42))]
    #[case::branch("branch:feature", ReviewTarget::Branch("feature".into()))]
    #[case::revs("revs:main..HEAD", ReviewTarget::Revs { base: "main".into(), head: "HEAD".into() })]
    fn positive_decodes_known_targets(#[case] wire: &str, #[case] want: ReviewTarget) {
        assert_eq!(decode_target(wire).unwrap(), want);
    }

    #[rstest]
    #[case::empty("")]
    #[case::unknown("nonsense")]
    #[case::pr_not_numeric("pr:abc")]
    #[case::revs_no_range("revs:mainHEAD")]
    fn adversarial_malformed_targets_are_rejected(#[case] wire: &str) {
        assert!(decode_target(wire).is_err(), "must reject `{wire}`");
    }
}
