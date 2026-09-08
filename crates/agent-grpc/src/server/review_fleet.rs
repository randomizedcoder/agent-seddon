//! The `FleetRegistry` seam as a service (review-fleet C3): the review-fleet
//! roster — CRUD over `FleetSession` rows (`agent --serve-fleet` control plane).
//!
//! Fails **hard** (`Err` → `Status`), like the provider-registry seam: a
//! control-plane mutation that fails should surface to the operator, not
//! silently degrade. Every id/row is untrusted and validated inside the store
//! (`safe_segment`, number clamps, count caps); a rejection maps to
//! `InvalidArgument` — and a store's `not found` to `NotFound` — via
//! `status_from_error`. Rows carry `token_ref` *references*; the server never
//! resolves one (there is no token to leak), so `Get`/`List` return the
//! reference verbatim.

use std::sync::Arc;

use agent_core::{
    ApproveOutcome, FleetApprover, FleetRegistry, FleetTrigger, TriggerOutcome, TriggerSink,
};
use agent_proto::{pb, status_from_error};
use tonic::transport::server::Router;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::Instrument;

use super::span;

pub struct ReviewFleetSvc {
    inner: Arc<dyn FleetRegistry>,
    /// The orchestrator's trigger intake (review-fleet C8). `None` on the bare
    /// control-plane endpoint (roster CRUD only), so `ReviewNow` there is
    /// `UNIMPLEMENTED`; the full `--serve-fleet` process wires it via
    /// [`Self::with_triggers`]. Mirrors `AgentSessionSvc::with_driver`.
    triggers: Option<Arc<dyn TriggerSink>>,
    /// The approve→post tail (review-fleet C17). `None` unless the full `--serve-fleet`
    /// process wires it (and only when persisted history exists to look a draft up), so
    /// `Approve` on the bare control plane is `UNIMPLEMENTED`. Wired via
    /// [`Self::with_approver`].
    approver: Option<Arc<dyn FleetApprover>>,
}

impl ReviewFleetSvc {
    pub fn new(inner: Arc<dyn FleetRegistry>) -> Self {
        Self {
            inner,
            triggers: None,
            approver: None,
        }
    }
    /// Enable the `ReviewNow` RPC by attaching the orchestrator's trigger sink.
    pub fn with_triggers(mut self, triggers: Arc<dyn TriggerSink>) -> Self {
        self.triggers = Some(triggers);
        self
    }
    /// Enable the `Approve` RPC by attaching the approve→post tail (review-fleet C17).
    pub fn with_approver(mut self, approver: Arc<dyn FleetApprover>) -> Self {
        self.approver = Some(approver);
        self
    }
    pub fn into_server(self) -> pb::review_fleet_service_server::ReviewFleetServiceServer<Self> {
        pb::review_fleet_service_server::ReviewFleetServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl pb::review_fleet_service_server::ReviewFleetService for ReviewFleetSvc {
    async fn list(
        &self,
        request: Request<pb::FleetListRequest>,
    ) -> Result<Response<pb::FleetSessionList>, Status> {
        let sp = span("fleet.list", request.metadata());
        let inner = self.inner.clone();
        async move {
            let rows = inner.list().await.map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::FleetSessionList {
                sessions: rows.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp)
        .await
    }

    async fn get(
        &self,
        request: Request<pb::FleetSessionRef>,
    ) -> Result<Response<pb::FleetSession>, Status> {
        let sp = span("fleet.get", request.metadata());
        let inner = self.inner.clone();
        async move {
            let row = inner
                .get(&request.into_inner().id)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(row.into()))
        }
        .instrument(sp)
        .await
    }

    async fn put(
        &self,
        request: Request<pb::FleetSession>,
    ) -> Result<Response<pb::FleetSession>, Status> {
        let sp = span("fleet.put", request.metadata());
        let inner = self.inner.clone();
        async move {
            // Wire → core clamps numbers; the store validates fail-closed.
            let session = agent_core::FleetSession::from(request.into_inner());
            let stored = inner
                .put(session)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(stored.into()))
        }
        .instrument(sp)
        .await
    }

    async fn delete(
        &self,
        request: Request<pb::FleetSessionRef>,
    ) -> Result<Response<pb::FleetDeleteReply>, Status> {
        let sp = span("fleet.delete", request.metadata());
        let inner = self.inner.clone();
        async move {
            let deleted = inner
                .delete(&request.into_inner().id)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::FleetDeleteReply { deleted }))
        }
        .instrument(sp)
        .await
    }

    async fn set_enabled(
        &self,
        request: Request<pb::FleetSetEnabledRequest>,
    ) -> Result<Response<pb::FleetSession>, Status> {
        let sp = span("fleet.set_enabled", request.metadata());
        let inner = self.inner.clone();
        async move {
            let req = request.into_inner();
            let row = inner
                .set_enabled(&req.id, req.enabled)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(row.into()))
        }
        .instrument(sp)
        .await
    }

    async fn review_now(
        &self,
        request: Request<pb::ReviewNowRequest>,
    ) -> Result<Response<pb::ReviewNowReply>, Status> {
        let sp = span("fleet.review_now", request.metadata());
        // Opt-in: only the full fleet process (with an orchestrator) wires a sink.
        let Some(triggers) = self.triggers.clone() else {
            return Err(Status::unimplemented(
                "ReviewNow requires the fleet orchestrator (run `agent --serve-fleet`)",
            ));
        };
        async move {
            let req = request.into_inner();
            // `enqueue` is fire-and-forget into a bounded, coalescing queue — it never
            // blocks or rejects; the outcome says whether it queued or coalesced.
            let outcome = triggers.enqueue(FleetTrigger {
                session_id: req.session_id,
                pr_number: req.pr_number,
            });
            Ok(Response::new(pb::ReviewNowReply {
                accepted: matches!(outcome, TriggerOutcome::Accepted),
            }))
        }
        .instrument(sp)
        .await
    }

    async fn approve(
        &self,
        request: Request<pb::ApproveRequest>,
    ) -> Result<Response<pb::ApproveReply>, Status> {
        let sp = span("fleet.approve", request.metadata());
        // Opt-in: only the full fleet process with persisted history wires an approver.
        let Some(approver) = self.approver.clone() else {
            return Err(Status::unimplemented(
                "Approve requires the fleet orchestrator with persisted history \
                 (run `agent --serve-fleet` with `[telemetry]` enabled)",
            ));
        };
        async move {
            let review_id = request.into_inner().review_id;
            let outcome = approver
                .approve(&review_id)
                .await
                .map_err(|e| status_from_error(&e))?;
            // NotFound/AlreadyPosted are ordinary outcomes (a total reply), not transport
            // errors — the caller reads `status`; only a genuine fault is an `Err` above.
            let (status, detail) = match outcome {
                ApproveOutcome::Posted { url } => ("posted", url),
                ApproveOutcome::AlreadyPosted => ("already_posted", String::new()),
                ApproveOutcome::NotFound => ("not_found", String::new()),
            };
            Ok(Response::new(pb::ApproveReply {
                status: status.to_string(),
                detail,
            }))
        }
        .instrument(sp)
        .await
    }
}

pub fn review_fleet_router(inner: Arc<dyn FleetRegistry>) -> Router {
    Server::builder().add_service(ReviewFleetSvc::new(inner).into_server())
}
