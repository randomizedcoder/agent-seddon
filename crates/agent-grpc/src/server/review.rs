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

/// Decode the wire target: `pr:<n>` | `pr:<n>@<oid>` | `branch:<name>` | `worktree` |
/// `revs:<base>..<head>`. The value is
/// attacker-controlled — a bad number or an unknown form is rejected, and an
/// unsafe branch name is caught downstream by the orchestrator's `safe_segment`.
pub(crate) fn decode_target(s: &str) -> Result<ReviewTarget, String> {
    if s == "worktree" {
        return Ok(ReviewTarget::WorkingTree);
    }
    if let Some(n) = s.strip_prefix("pr:") {
        // The head is checked as an object id by the orchestrator, before git.
        if let Some((n, head)) = n.split_once('@') {
            return n
                .parse::<u64>()
                .map(|number| ReviewTarget::PrAt {
                    number,
                    head: head.to_string(),
                })
                .map_err(|_| format!("invalid PR number `{n}`"));
        }
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
    #[case::pr_at("pr:42@abc", ReviewTarget::PrAt { number: 42, head: "abc".into() })]
    #[case::branch("branch:feature", ReviewTarget::Branch("feature".into()))]
    #[case::revs("revs:main..HEAD", ReviewTarget::Revs { base: "main".into(), head: "HEAD".into() })]
    fn positive_decodes_known_targets(#[case] wire: &str, #[case] want: ReviewTarget) {
        assert_eq!(decode_target(wire).unwrap(), want);
    }

    #[rstest]
    #[case::empty("")]
    #[case::unknown("nonsense")]
    #[case::pr_not_numeric("pr:abc")]
    #[case::pr_at_not_numeric("pr:x@abc")]
    #[case::pr_at_overflow("pr:99999999999999999999@abc")]
    #[case::revs_no_range("revs:mainHEAD")]
    fn adversarial_malformed_targets_are_rejected(#[case] wire: &str) {
        assert!(decode_target(wire).is_err(), "must reject `{wire}`");
    }

    /// Every target survives encode → decode (the client and server agree).
    #[rstest]
    #[case::pr(ReviewTarget::Pr(7))]
    #[case::pr_at(ReviewTarget::PrAt { number: 7, head: "c".repeat(40) })]
    #[case::branch(ReviewTarget::Branch("feature".into()))]
    #[case::worktree(ReviewTarget::WorkingTree)]
    #[case::revs(ReviewTarget::Revs { base: "main".into(), head: "HEAD".into() })]
    fn positive_targets_round_trip(#[case] t: ReviewTarget) {
        let wire = crate::client::review::encode_target(&t);
        assert_eq!(decode_target(&wire).unwrap(), t);
    }
}
