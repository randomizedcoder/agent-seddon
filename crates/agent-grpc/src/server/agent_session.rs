//! The agent-session observation surface as a service (docs/design/portal): a live
//! structured feed of the running loop + a status snapshot.
//!
//! Read-only. `Subscribe` sends the current `StatusSnapshot` first (so a late
//! joiner is consistent), then the live tail — a slow consumer lags and drops
//! rather than stalling the loop (bounded broadcast on the runtime side).

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use agent_core::{DriverError, SessionDriver, SessionSource, SessionSourceRegistry};
use agent_proto::{pb, snapshot_event};
use futures_util::{Stream, StreamExt};
use tonic::transport::server::Router;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::Instrument;

use super::{identity_key, span};

/// A `Send` response stream that owns the run's [`agent_core::RunHandle`]: when the
/// stream is dropped (client disconnect) the handle drops, cancelling the in-flight run.
struct GuardedStream {
    inner: Pin<Box<dyn Stream<Item = Result<pb::SessionEvent, Status>> + Send>>,
    _guard: agent_core::RunHandle,
}

impl Stream for GuardedStream {
    type Item = Result<pb::SessionEvent, Status>;
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.get_mut().inner.as_mut().poll_next(cx)
    }
}

pub struct AgentSessionSvc {
    registry: Arc<dyn SessionSourceRegistry>,
    /// The optional **write** side: admit-or-resolve a session and drive a goal. `None`
    /// on an observe-only endpoint (`--serve-all`, the observe sidecar) ⇒ `Send` is
    /// `UNIMPLEMENTED`. Only the opt-in `--serve-sessions` gateway sets it (via
    /// [`Self::with_driver`]), because `Send` is `--serve-mcp`-class arbitrary execution.
    driver: Option<Arc<dyn SessionDriver>>,
}

impl AgentSessionSvc {
    pub fn new(registry: Arc<dyn SessionSourceRegistry>) -> Self {
        Self {
            registry,
            driver: None,
        }
    }
    /// Enable the driving `Send` RPC by attaching a [`SessionDriver`] (the runtime's
    /// `SessionManager`). Kept opt-in — the observe-only serve paths never call this.
    #[must_use]
    pub fn with_driver(mut self, driver: Arc<dyn SessionDriver>) -> Self {
        self.driver = Some(driver);
        self
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

    type SendStream = Pin<Box<dyn Stream<Item = Result<pb::SessionEvent, Status>> + Send>>;

    /// Drive a goal and stream the run. **Opt-in and `--serve-mcp`-class**: only an
    /// endpoint given a [`SessionDriver`] via [`AgentSessionSvc::with_driver`] can run
    /// this (an observe-only endpoint returns `UNIMPLEMENTED`). The `(user, session)`
    /// identity rides gRPC metadata (fail-closed: absent ⇒ `UNAUTHENTICATED`); an empty
    /// goal is `INVALID_ARGUMENT`; a capacity cap is `RESOURCE_EXHAUSTED`.
    ///
    /// Like `Subscribe`, the stream leads with a `status_snapshot` taken **before**
    /// the run starts, so `RunStarted` is never missed; it then carries the live tail
    /// and ends after `RunFinished`. The returned [`GuardedStream`] owns the run's
    /// cancel handle, so a client disconnect cancels the run.
    #[allow(clippy::result_large_err)]
    async fn send(
        &self,
        request: Request<pb::GoalRequest>,
    ) -> Result<Response<Self::SendStream>, Status> {
        let _sp = span("agent_session.send", request.metadata());
        let Some(driver) = self.driver.as_ref() else {
            return Err(Status::unimplemented(
                "Send is not enabled on this endpoint (observe-only; no session driver)",
            ));
        };
        // Identity is the tenant key; there is no auth layer, so it is only as
        // trustworthy as the transport (docs/design/multi-session/07-security.md), but a
        // driven run must still be attributable — fail closed when it is absent.
        let key = identity_key(request.metadata())
            .ok_or_else(|| Status::unauthenticated("Send requires (user, session) identity"))?;
        let goal = request.into_inner().goal;
        if goal.trim().is_empty() {
            return Err(Status::invalid_argument("goal must not be empty"));
        }

        let ds = driver.session_for(key).map_err(|e| match e {
            DriverError::PerUserLimit(_) | DriverError::TotalLimit(_) => {
                Status::resource_exhausted(e.to_string())
            }
        })?;

        // Subscribe *before* starting the run so the leading `RunStarted` is captured
        // (the snapshot is taken first, exactly as `subscribe` does).
        let snapshot = snapshot_event(ds.source.snapshot());
        let tail = ds.source.subscribe();
        let guard = ds.runner.start(goal); // RunHandle — drop == cancel

        // Emit events until (and including) `RunFinished`, then end the stream.
        let body = tail.map(pb::SessionEvent::from).scan(false, |done, ev| {
            if *done {
                return std::future::ready(None);
            }
            if matches!(ev.kind, Some(pb::session_event::Kind::RunFinished(_))) {
                *done = true;
            }
            std::future::ready(Some(Ok::<_, Status>(ev)))
        });
        let inner = futures_util::stream::once(async move { Ok(snapshot) }).chain(body);
        let stream = GuardedStream {
            inner: Box::pin(inner),
            _guard: guard,
        };
        Ok(Response::new(Box::pin(stream) as Self::SendStream))
    }
}

pub fn agent_session_router(registry: Arc<dyn SessionSourceRegistry>) -> Router {
    Server::builder().add_service(AgentSessionSvc::new(registry).into_server())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{SessionEvent, SessionEventStream, StatusSnapshot};
    // The generated service trait, so tests can call the `send` RPC method directly.
    use pb::agent_session_service_server::AgentSessionService as _;

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

    // Send is opt-in: an endpoint built without a driver (every observe-only serve path)
    // must refuse to drive a goal, never silently no-op.
    #[tokio::test]
    async fn negative_send_without_driver_is_unimplemented() {
        let err = svc(&["s1"])
            .send(Request::new(pb::GoalRequest {
                goal: "do a thing".into(),
                session_id: String::new(),
            }))
            .await
            .map(|_| ())
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::Unimplemented);
    }

    // --- Send driving (fake driver, since agent-grpc can't depend on agent-runtime) ---

    use agent_core::{DriverError, DriverSession, RunHandle, RunStarter, SessionDriver};

    /// A source that replays a fixed event script (the run's events would normally be
    /// published by the loop; here they are scripted to drive the handler's assembly).
    struct ScriptSource {
        events: Vec<SessionEvent>,
    }
    impl SessionSource for ScriptSource {
        fn snapshot(&self) -> StatusSnapshot {
            StatusSnapshot::default()
        }
        fn subscribe(&self) -> SessionEventStream {
            Box::pin(futures_util::stream::iter(self.events.clone()))
        }
    }
    struct NoopRunner;
    impl RunStarter for NoopRunner {
        fn start(&self, _goal: String) -> RunHandle {
            RunHandle::new(())
        }
    }
    /// Admits and hands back a scripted source.
    struct OkDriver {
        events: Vec<SessionEvent>,
    }
    impl SessionDriver for OkDriver {
        fn session_for(&self, _key: agent_core::SessionKey) -> Result<DriverSession, DriverError> {
            Ok(DriverSession {
                source: Arc::new(ScriptSource {
                    events: self.events.clone(),
                }),
                runner: Arc::new(NoopRunner),
            })
        }
    }
    /// Always over capacity.
    struct FullDriver;
    impl SessionDriver for FullDriver {
        fn session_for(&self, _key: agent_core::SessionKey) -> Result<DriverSession, DriverError> {
            Err(DriverError::TotalLimit(1))
        }
    }

    fn driving_svc(driver: Arc<dyn SessionDriver>) -> AgentSessionSvc {
        AgentSessionSvc::new(Arc::new(FakeRegistry { ids: vec![] })).with_driver(driver)
    }
    fn goal_req(goal: &str, ident: Option<(&str, &str)>) -> Request<pb::GoalRequest> {
        let mut req = Request::new(pb::GoalRequest {
            goal: goal.into(),
            session_id: String::new(),
        });
        if let Some((u, s)) = ident {
            agent_proto::identity::inject_identity(u, s, req.metadata_mut());
        }
        req
    }

    /// `positive_`: the stream leads with a snapshot, carries the run's events, and ends
    /// **at** `RunFinished` — a later event is not delivered.
    #[tokio::test]
    async fn positive_send_streams_snapshot_then_run_until_finished() {
        let events = vec![
            SessionEvent::RunStarted { goal: "go".into() },
            SessionEvent::RunFinished { ok: true },
            // Must NOT be delivered — the stream ends at RunFinished.
            SessionEvent::TokenDelta {
                text: "after".into(),
            },
        ];
        let svc = driving_svc(Arc::new(OkDriver { events }));
        let resp = svc
            .send(goal_req("go", Some(("alice", "s1"))))
            .await
            .unwrap();
        let kinds: Vec<_> = resp
            .into_inner()
            .map(|r| r.unwrap().kind.unwrap())
            .collect()
            .await;
        assert_eq!(
            kinds.len(),
            3,
            "snapshot + RunStarted + RunFinished, nothing after"
        );
        assert!(matches!(
            kinds[0],
            pb::session_event::Kind::StatusSnapshot(_)
        ));
        assert!(matches!(kinds[1], pb::session_event::Kind::RunStarted(_)));
        assert!(matches!(kinds[2], pb::session_event::Kind::RunFinished(_)));
    }

    /// `negative_`: a driven run must be attributable — absent identity metadata fails
    /// closed (there is no auth layer, but the tenant key is still required).
    #[tokio::test]
    async fn negative_send_without_identity_is_unauthenticated() {
        let svc = driving_svc(Arc::new(OkDriver { events: vec![] }));
        let err = svc
            .send(goal_req("go", None))
            .await
            .map(|_| ())
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::Unauthenticated);
    }

    /// `corner_`: an empty (or whitespace) goal is rejected before any session work.
    #[tokio::test]
    async fn corner_send_empty_goal_is_invalid_argument() {
        let svc = driving_svc(Arc::new(OkDriver { events: vec![] }));
        let err = svc
            .send(goal_req("   ", Some(("alice", "s1"))))
            .await
            .map(|_| ())
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
    }

    /// `adversarial_`: a spray of new goals that would exceed a capacity cap is shed with
    /// RESOURCE_EXHAUSTED (the amplification guard, via the driver's `DriverError`).
    #[tokio::test]
    async fn adversarial_send_over_capacity_is_resource_exhausted() {
        let svc = driving_svc(Arc::new(FullDriver));
        let err = svc
            .send(goal_req("go", Some(("alice", "s1"))))
            .await
            .map(|_| ())
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::ResourceExhausted);
    }
}
