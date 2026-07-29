//! The agent-session observation surface as a service (docs/design/portal): a live
//! structured feed of the running loop + a status snapshot.
//!
//! Read-only. `Subscribe` sends the current `StatusSnapshot` first (so a late
//! joiner is consistent), then the live tail — a slow consumer lags and drops
//! rather than stalling the loop (bounded broadcast on the runtime side).

use std::pin::Pin;
use std::sync::Arc;

use agent_core::{SessionSource, SessionSourceRegistry};
use agent_proto::{pb, snapshot_event};
use futures_util::{Stream, StreamExt};
use tonic::transport::server::Router;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::Instrument;

use super::span;

pub struct AgentSessionSvc {
    registry: Arc<dyn SessionSourceRegistry>,
}

impl AgentSessionSvc {
    pub fn new(registry: Arc<dyn SessionSourceRegistry>) -> Self {
        Self { registry }
    }
    pub fn into_server(self) -> pb::agent_session_service_server::AgentSessionServiceServer<Self> {
        pb::agent_session_service_server::AgentSessionServiceServer::new(self)
    }

    /// Resolve a request's `session_id` to the source to observe. `session_id` is
    /// **attacker-controlled wire input**, so it is validated with `safe_segment`
    /// before any lookup (fail closed on a malformed selector). An empty selector is
    /// the single-session convenience: the sole live session, or `INVALID_ARGUMENT`
    /// when zero or several are live (ambiguous). A well-formed but unknown id is
    /// `NOT_FOUND`.
    #[allow(clippy::result_large_err)]
    fn resolve(&self, session_id: &str) -> Result<Arc<dyn SessionSource>, Status> {
        if session_id.is_empty() {
            return self.registry.sole_source().ok_or_else(|| {
                Status::invalid_argument(
                    "session_id required: zero or multiple live sessions to observe",
                )
            });
        }
        if !agent_core::safe_segment(session_id) {
            return Err(Status::invalid_argument("malformed session_id"));
        }
        self.registry
            .source(session_id)
            .ok_or_else(|| Status::not_found("no such live session"))
    }
}

#[tonic::async_trait]
impl pb::agent_session_service_server::AgentSessionService for AgentSessionSvc {
    type SubscribeStream = Pin<Box<dyn Stream<Item = Result<pb::SessionEvent, Status>> + Send>>;

    #[allow(clippy::result_large_err)]
    async fn subscribe(
        &self,
        request: Request<pb::SubscribeRequest>,
    ) -> Result<Response<Self::SubscribeStream>, Status> {
        let sp = span("agent_session.subscribe", request.metadata());
        let source = self.resolve(&request.get_ref().session_id)?;
        // The snapshot is taken *before* subscribing so no event is missed between
        // the two (the leading snapshot may double a just-published event — the
        // client renders idempotently).
        let snapshot = snapshot_event(source.snapshot());
        let tail = source.subscribe().map(|ev| Ok(pb::SessionEvent::from(ev)));
        let stream = futures_util::stream::once(async move { Ok(snapshot) }).chain(tail);
        let _ = sp;
        Ok(Response::new(Box::pin(stream) as Self::SubscribeStream))
    }

    async fn snapshot(
        &self,
        request: Request<pb::SnapshotRequest>,
    ) -> Result<Response<pb::StatusSnapshot>, Status> {
        let sp = span("agent_session.snapshot", request.metadata());
        let source = self.resolve(&request.get_ref().session_id)?;
        async move { Ok(Response::new(source.snapshot().into())) }
            .instrument(sp)
            .await
    }
}

pub fn agent_session_router(registry: Arc<dyn SessionSourceRegistry>) -> Router {
    Server::builder().add_service(AgentSessionSvc::new(registry).into_server())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{SessionEvent, SessionEventStream, StatusSnapshot};

    // A stub source (its contents are irrelevant to selector resolution).
    struct StubSource;
    impl SessionSource for StubSource {
        fn snapshot(&self) -> StatusSnapshot {
            StatusSnapshot::default()
        }
        fn subscribe(&self) -> SessionEventStream {
            Box::pin(futures_util::stream::empty::<SessionEvent>())
        }
    }

    // A registry over a fixed set of ids — enough to drive every `resolve` branch.
    struct FakeRegistry {
        ids: Vec<String>,
    }
    impl SessionSourceRegistry for FakeRegistry {
        fn source(&self, session_id: &str) -> Option<Arc<dyn SessionSource>> {
            self.ids
                .iter()
                .any(|i| i == session_id)
                .then(|| Arc::new(StubSource) as Arc<dyn SessionSource>)
        }
        fn sole_source(&self) -> Option<Arc<dyn SessionSource>> {
            (self.ids.len() == 1).then(|| Arc::new(StubSource) as Arc<dyn SessionSource>)
        }
        fn live_session_ids(&self) -> Vec<String> {
            self.ids.clone()
        }
    }

    fn svc(ids: &[&str]) -> AgentSessionSvc {
        AgentSessionSvc::new(Arc::new(FakeRegistry {
            ids: ids.iter().map(std::string::ToString::to_string).collect(),
        }))
    }

    #[test]
    fn positive_empty_selector_resolves_the_sole_session() {
        assert!(svc(&["s1"]).resolve("").is_ok());
    }

    #[test]
    fn positive_named_selector_resolves_a_live_session() {
        assert!(svc(&["s1", "s2"]).resolve("s2").is_ok());
    }

    #[test]
    fn boundary_empty_selector_with_many_sessions_is_invalid_argument() {
        // The ambiguity case the design calls out: which of several is "the" session?
        let err = svc(&["s1", "s2"]).resolve("").map(|_| ()).unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
    }

    #[test]
    fn corner_empty_selector_with_zero_sessions_is_invalid_argument() {
        let err = svc(&[]).resolve("").map(|_| ()).unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
    }

    #[test]
    fn negative_unknown_id_is_not_found() {
        let err = svc(&["s1"]).resolve("ghost").map(|_| ()).unwrap_err();
        assert_eq!(err.code(), tonic::Code::NotFound);
    }

    // `session_id` is attacker-controlled: a traversal / injection selector must be
    // rejected by `safe_segment` *before* any registry lookup, never silently missed.
    #[rstest::rstest]
    #[case::traversal("../../etc/passwd")]
    #[case::separator("a/b")]
    #[case::leading_dash("-rf")]
    #[case::dotdot("..")]
    fn adversarial_malformed_selector_is_invalid_argument(#[case] bad: &str) {
        let err = svc(&["s1"]).resolve(bad).map(|_| ()).unwrap_err();
        assert_eq!(
            err.code(),
            tonic::Code::InvalidArgument,
            "malformed selector {bad:?} must fail closed"
        );
    }
}
