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
    ApproveOutcome, FleetApprover, FleetDraftEditor, FleetDraftReader, FleetHistory, FleetRegistry,
    FleetTrigger, PreflightProvider, ReviewDraftFilter, ReviewDraftRecord, TriggerOutcome,
    TriggerSink, UpdateOutcome,
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
    /// Operational self-diagnosis source (docs/design/doctor/). `None` on the bare
    /// control plane (no config to build probes from), so `Preflight` there is
    /// `UNIMPLEMENTED`; the full `--serve-fleet` process wires it via
    /// [`Self::with_preflight`].
    preflight: Option<Arc<dyn PreflightProvider>>,
    /// Persisted review-draft history (review-fleet C14). `None` unless a process wires
    /// persisted history, so `ListReviews` there is `UNIMPLEMENTED`; the `--serve-fleet`
    /// process wires it via [`Self::with_history`]. Read-only.
    history: Option<Arc<dyn FleetHistory>>,
    /// Persisted draft-body reader (review-fleet C14). `None` unless a process wires it (needs
    /// both history and a fleet root), so `GetReview` there is `UNIMPLEMENTED`; wired via
    /// [`Self::with_reader`]. Read-only.
    reader: Option<Arc<dyn FleetDraftReader>>,
    /// Persisted draft-body editor (review-fleet C14). `None` unless a process wires it (needs
    /// both history and a fleet root), so `UpdateReview` there is `UNIMPLEMENTED`; wired via
    /// [`Self::with_editor`]. Edits the local draft only — it never posts.
    editor: Option<Arc<dyn FleetDraftEditor>>,
}

/// A persisted draft record → its wire METADATA (`ReviewSummary`). The `.md` body and the
/// server-minted `draft_path` are deliberately NOT projected here — the body rides only in
/// `GetReview`, and the path is never exposed on the wire.
fn summary_from_record(r: ReviewDraftRecord) -> pb::ReviewSummary {
    pb::ReviewSummary {
        review_id: r.review_id,
        repo: r.repo,
        pr_number: r.pr_number,
        head_sha: r.head_sha,
        risk_score: r.risk_score,
        gate_failed: r.gate_failed,
        n_findings: r.n_findings,
        files_changed: r.files_changed,
        additions: r.additions,
        deletions: r.deletions,
        status: r.status,
    }
}

/// An empty wire string means "no constraint" (the proto default); a non-empty one is a bound
/// filter value. Keeps the `""` ⇒ `None` mapping in one place.
fn filter_opt(s: String) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

impl ReviewFleetSvc {
    pub fn new(inner: Arc<dyn FleetRegistry>) -> Self {
        Self {
            inner,
            triggers: None,
            approver: None,
            preflight: None,
            history: None,
            reader: None,
            editor: None,
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
    /// Enable the `Preflight` RPC by attaching an operational self-diagnosis source
    /// (docs/design/doctor/).
    pub fn with_preflight(mut self, preflight: Arc<dyn PreflightProvider>) -> Self {
        self.preflight = Some(preflight);
        self
    }
    /// Enable the `ListReviews` RPC by attaching the persisted review-draft history
    /// (review-fleet C14). Read-only.
    pub fn with_history(mut self, history: Arc<dyn FleetHistory>) -> Self {
        self.history = Some(history);
        self
    }
    /// Enable the `GetReview` RPC by attaching the persisted draft-body reader (review-fleet
    /// C14). Read-only.
    pub fn with_reader(mut self, reader: Arc<dyn FleetDraftReader>) -> Self {
        self.reader = Some(reader);
        self
    }
    /// Enable the `UpdateReview` RPC by attaching the persisted draft-body editor (review-fleet
    /// C14). Edits the local draft only — it never posts.
    pub fn with_editor(mut self, editor: Arc<dyn FleetDraftEditor>) -> Self {
        self.editor = Some(editor);
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
        super::authz::require(agent_core::Action::Write, agent_core::ResourceType::Fleet)?;
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
        super::authz::require(agent_core::Action::Delete, agent_core::ResourceType::Fleet)?;
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
        super::authz::require(agent_core::Action::Write, agent_core::ResourceType::Fleet)?;
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
        super::authz::require(agent_core::Action::Trigger, agent_core::ResourceType::Fleet)?;
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
        super::authz::require(agent_core::Action::Approve, agent_core::ResourceType::Fleet)?;
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

    async fn preflight(
        &self,
        request: Request<pb::PreflightRequest>,
    ) -> Result<Response<pb::PreflightReply>, Status> {
        // Read-only diagnostic (like list/get): no authz gate. The report carries only
        // status classes and trusted config values — never a secret.
        let sp = span("fleet.preflight", request.metadata());
        // Opt-in: only the full fleet process has the config to build the probes.
        let Some(preflight) = self.preflight.clone() else {
            return Err(Status::unimplemented(
                "Preflight requires the fleet process (run `agent --serve-fleet`)",
            ));
        };
        async move {
            let report = preflight.preflight().await;
            Ok(Response::new(pb::PreflightReply {
                ok: report.ok(),
                probes: report
                    .probes
                    .into_iter()
                    .map(|p| pb::PreflightProbe {
                        name: p.name,
                        status: p.status.as_str().to_string(),
                        detail: p.detail,
                        latency_ms: p.latency_ms,
                    })
                    .collect(),
            }))
        }
        .instrument(sp)
        .await
    }

    async fn list_reviews(
        &self,
        request: Request<pb::ListReviewsRequest>,
    ) -> Result<Response<pb::ListReviewsReply>, Status> {
        // Read-only (like list/get/preflight): no authz gate. Every filter value is bound as a
        // query argument in the impl (never interpolated), and the impl caps the row count.
        let sp = span("fleet.list_reviews", request.metadata());
        // Opt-in: only a process with persisted history can list drafts.
        let Some(history) = self.history.clone() else {
            return Err(Status::unimplemented(
                "ListReviews requires persisted fleet history \
                 (run `agent --serve-fleet` with `[telemetry]` enabled)",
            ));
        };
        async move {
            let req = request.into_inner();
            let filter = ReviewDraftFilter {
                repo: filter_opt(req.repo),
                session_id: filter_opt(req.session_id),
                status: filter_opt(req.status),
                limit: req.limit as usize, // 0 ⇒ the impl's row cap; over-cap is clamped there
            };
            let rows = history
                .list_drafts(&filter)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::ListReviewsReply {
                reviews: rows.into_iter().map(summary_from_record).collect(),
            }))
        }
        .instrument(sp)
        .await
    }

    async fn get_review(
        &self,
        request: Request<pb::GetReviewRequest>,
    ) -> Result<Response<pb::GetReviewReply>, Status> {
        // Read-only: no authz gate. `review_id` is untrusted wire input — the reader looks it
        // up as a bound query arg and reads the body from the draft's own `draft_path`
        // (confined under the fleet root), never a wire-supplied path; the body is byte-capped.
        let sp = span("fleet.get_review", request.metadata());
        // Opt-in: needs the draft-body reader (persisted history + a fleet root).
        let Some(reader) = self.reader.clone() else {
            return Err(Status::unimplemented(
                "GetReview requires the fleet draft reader \
                 (run `agent --serve-fleet` with `[telemetry]` enabled)",
            ));
        };
        async move {
            let review_id = request.into_inner().review_id;
            let body = reader
                .read_body(&review_id)
                .await
                .map_err(|e| status_from_error(&e))?
                .ok_or_else(|| Status::not_found("no persisted draft for this review_id"))?;
            Ok(Response::new(pb::GetReviewReply {
                meta: Some(summary_from_record(body.record)),
                body: body.body,
                truncated: body.truncated,
            }))
        }
        .instrument(sp)
        .await
    }

    async fn update_review(
        &self,
        request: Request<pb::UpdateReviewRequest>,
    ) -> Result<Response<pb::UpdateReviewReply>, Status> {
        // A write to a persisted draft: gated `Write` on the Fleet resource (like put/delete).
        // `review_id` is untrusted — the editor looks it up as a bound query arg and writes the
        // body to the draft's own `draft_path` (confined under the fleet root), never a
        // wire-supplied path; an over-cap body is rejected, a posted/approved draft is locked.
        super::authz::require(agent_core::Action::Write, agent_core::ResourceType::Fleet)?;
        let sp = span("fleet.update_review", request.metadata());
        // Opt-in: needs the draft-body editor (persisted history + a fleet root).
        let Some(editor) = self.editor.clone() else {
            return Err(Status::unimplemented(
                "UpdateReview requires the fleet draft editor \
                 (run `agent --serve-fleet` with `[telemetry]` enabled)",
            ));
        };
        async move {
            let req = request.into_inner();
            let outcome = editor
                .update_body(&req.review_id, &req.body)
                .await
                .map_err(|e| status_from_error(&e))?;
            // NotFound/Locked are ordinary outcomes (a total reply), not transport errors — the
            // caller reads `status`; only a genuine fault (over-cap, unwritable) is an `Err`.
            let status = match outcome {
                UpdateOutcome::Updated => "updated".to_string(),
                UpdateOutcome::NotFound => "not_found".to_string(),
                UpdateOutcome::Locked { .. } => "locked".to_string(),
            };
            Ok(Response::new(pb::UpdateReviewReply { status }))
        }
        .instrument(sp)
        .await
    }
}

pub fn review_fleet_router(inner: Arc<dyn FleetRegistry>) -> Router {
    Server::builder().add_service(ReviewFleetSvc::new(inner).into_server())
}
