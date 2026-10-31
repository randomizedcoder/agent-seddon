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
        requested_by: r.requested_by,
        approved_by: r.approved_by,
    }
}

/// The caller's requester label (S19) — who asked for or approved a review — from the
/// verified principal the auth layer installed. `None` when auth is off (`mode = "none"`).
fn caller_label() -> Option<String> {
    agent_core::current_principal().map(|p| agent_core::requester_label(&p))
}

/// Cap on `ListReviewsRequest.exclude_status` entries (there are five statuses).
const MAX_EXCLUDE_STATUS: usize = 8;

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
        let key = super::identity_key(request.metadata());
        let sp = span("fleet.list", request.metadata());
        let inner = self.inner.clone();
        let work = async move {
            let rows = inner.list().await.map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::FleetSessionList {
                sessions: rows.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }

    async fn get(
        &self,
        request: Request<pb::FleetSessionRef>,
    ) -> Result<Response<pb::FleetSession>, Status> {
        let key = super::identity_key(request.metadata());
        let sp = span("fleet.get", request.metadata());
        let inner = self.inner.clone();
        let work = async move {
            let row = inner
                .get(&request.into_inner().id)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(row.into()))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }

    async fn put(
        &self,
        request: Request<pb::FleetSession>,
    ) -> Result<Response<pb::FleetSession>, Status> {
        super::authz::require(agent_core::Action::Write, agent_core::ResourceType::Fleet)?;
        let key = super::identity_key(request.metadata());
        let tenant = super::request_tenant(key.as_ref());
        let sp = span("fleet.put", request.metadata());
        let inner = self.inner.clone();
        let work = async move {
            // Wire → core clamps numbers; the store validates fail-closed.
            let mut session = agent_core::FleetSession::from(request.into_inner());
            bind_owner(&mut session, tenant.as_deref())?;
            let stored = inner
                .put(session)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(stored.into()))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }

    async fn delete(
        &self,
        request: Request<pb::FleetSessionRef>,
    ) -> Result<Response<pb::FleetDeleteReply>, Status> {
        super::authz::require(agent_core::Action::Delete, agent_core::ResourceType::Fleet)?;
        let key = super::identity_key(request.metadata());
        let sp = span("fleet.delete", request.metadata());
        let inner = self.inner.clone();
        let work = async move {
            let deleted = inner
                .delete(&request.into_inner().id)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::FleetDeleteReply { deleted }))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }

    async fn set_enabled(
        &self,
        request: Request<pb::FleetSetEnabledRequest>,
    ) -> Result<Response<pb::FleetSession>, Status> {
        super::authz::require(agent_core::Action::Write, agent_core::ResourceType::Fleet)?;
        let key = super::identity_key(request.metadata());
        let sp = span("fleet.set_enabled", request.metadata());
        let inner = self.inner.clone();
        let work = async move {
            let req = request.into_inner();
            let row = inner
                .set_enabled(&req.id, req.enabled)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(row.into()))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }

    async fn review_now(
        &self,
        request: Request<pb::ReviewNowRequest>,
    ) -> Result<Response<pb::ReviewNowReply>, Status> {
        super::authz::require(agent_core::Action::Trigger, agent_core::ResourceType::Fleet)?;
        let key = super::identity_key(request.metadata());
        let sp = span("fleet.review_now", request.metadata());
        let requester = caller_label();
        // Opt-in: only the full fleet process (with an orchestrator) wires a sink.
        let Some(triggers) = self.triggers.clone() else {
            return Err(Status::unimplemented(
                "ReviewNow requires the fleet orchestrator (run `agent --serve-fleet`)",
            ));
        };
        let work = async move {
            let req = request.into_inner();
            // `enqueue` is fire-and-forget into a bounded, coalescing queue — it never
            // blocks or rejects; the outcome says whether it queued or coalesced.
            // The caller's tenant (in scope here): the roster row was written under it, so
            // the fleet must resolve `session_id` there too (forge-auth F6).
            let outcome = triggers.enqueue(FleetTrigger {
                session_id: req.session_id,
                pr_number: req.pr_number,
                requested_by: requester.into_iter().collect(),
                tenant: agent_core::scoped_tenant().unwrap_or_default(),
            });
            Ok(Response::new(pb::ReviewNowReply {
                accepted: matches!(outcome, TriggerOutcome::Accepted),
            }))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }

    async fn approve(
        &self,
        request: Request<pb::ApproveRequest>,
    ) -> Result<Response<pb::ApproveReply>, Status> {
        super::authz::require(
            agent_core::Action::Approve,
            agent_core::ResourceType::Review,
        )?;
        let key = super::identity_key(request.metadata());
        let sp = span("fleet.approve", request.metadata());
        let approver_label = caller_label();
        // Opt-in: only the full fleet process with persisted history wires an approver.
        let Some(approver) = self.approver.clone() else {
            return Err(Status::unimplemented(
                "Approve requires the fleet orchestrator with persisted history \
                 (run `agent --serve-fleet` with `[telemetry]` enabled)",
            ));
        };
        let work = async move {
            let review_id = request.into_inner().review_id;
            let outcome = approver
                .approve(&review_id, approver_label.as_deref())
                .await
                .map_err(|e| status_from_error(&e))?;
            // NotFound/AlreadyPosted are ordinary outcomes (a total reply), not transport
            // errors — the caller reads `status`; only a genuine fault is an `Err` above.
            let (status, detail) = match outcome {
                ApproveOutcome::Posted { url } => ("posted", url),
                ApproveOutcome::AlreadyPosted => ("already_posted", String::new()),
                ApproveOutcome::NotFound => ("not_found", String::new()),
                ApproveOutcome::Skipped => ("skipped", String::new()),
            };
            Ok(Response::new(pb::ApproveReply {
                status: status.to_string(),
                detail,
            }))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }

    async fn skip_review(
        &self,
        request: Request<pb::SkipReviewRequest>,
    ) -> Result<Response<pb::SkipReviewReply>, Status> {
        // Deciding a draft is never posted is the approver's call, like Approve.
        super::authz::require(
            agent_core::Action::Approve,
            agent_core::ResourceType::Review,
        )?;
        let key = super::identity_key(request.metadata());
        let sp = span("fleet.skip_review", request.metadata());
        let skipped_by = caller_label();
        let Some(approver) = self.approver.clone() else {
            return Err(Status::unimplemented(
                "SkipReview requires the fleet orchestrator with persisted history \
                 (run `agent --serve-fleet` with `[telemetry]` enabled)",
            ));
        };
        let work = async move {
            let review_id = request.into_inner().review_id;
            let outcome = approver
                .skip(&review_id, skipped_by.as_deref())
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::SkipReviewReply {
                status: outcome.as_wire().to_string(),
            }))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }

    async fn preflight(
        &self,
        request: Request<pb::PreflightRequest>,
    ) -> Result<Response<pb::PreflightReply>, Status> {
        // Read-only diagnostic (like list/get): no authz gate. The report carries only
        // status classes and trusted config values — never a secret.
        let key = super::identity_key(request.metadata());
        let sp = span("fleet.preflight", request.metadata());
        // Opt-in: only the full fleet process has the config to build the probes.
        let Some(preflight) = self.preflight.clone() else {
            return Err(Status::unimplemented(
                "Preflight requires the fleet process (run `agent --serve-fleet`)",
            ));
        };
        let work = async move {
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
        .instrument(sp);
        super::run_scoped(key, work).await
    }

    async fn list_reviews(
        &self,
        request: Request<pb::ListReviewsRequest>,
    ) -> Result<Response<pb::ListReviewsReply>, Status> {
        // Read-only (like list/get/preflight): no authz gate. Every filter value is bound as a
        // query argument in the impl (never interpolated), and the impl caps the row count.
        let key = super::identity_key(request.metadata());
        let sp = span("fleet.list_reviews", request.metadata());
        // Opt-in: only a process with persisted history can list drafts.
        let Some(history) = self.history.clone() else {
            return Err(Status::unimplemented(
                "ListReviews requires persisted fleet history \
                 (run `agent --serve-fleet` with `[telemetry]` enabled)",
            ));
        };
        let work = async move {
            let req = request.into_inner();
            // Untrusted: bound the list (each value only ever compared, never queried).
            if req.exclude_status.len() > MAX_EXCLUDE_STATUS {
                return Err(Status::invalid_argument(format!(
                    "exclude_status has {} entries; at most {MAX_EXCLUDE_STATUS}",
                    req.exclude_status.len()
                )));
            }
            let filter = ReviewDraftFilter {
                repo: filter_opt(req.repo),
                session_id: filter_opt(req.session_id),
                status: filter_opt(req.status),
                exclude_status: req.exclude_status,
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
        .instrument(sp);
        super::run_scoped(key, work).await
    }

    async fn get_review(
        &self,
        request: Request<pb::GetReviewRequest>,
    ) -> Result<Response<pb::GetReviewReply>, Status> {
        // Read-only: no authz gate. `review_id` is untrusted wire input — the reader looks it
        // up as a bound query arg and reads the body from the draft's own `draft_path`
        // (confined under the fleet root), never a wire-supplied path; the body is byte-capped.
        let key = super::identity_key(request.metadata());
        let sp = span("fleet.get_review", request.metadata());
        // Opt-in: needs the draft-body reader (persisted history + a fleet root).
        let Some(reader) = self.reader.clone() else {
            return Err(Status::unimplemented(
                "GetReview requires the fleet draft reader \
                 (run `agent --serve-fleet` with `[telemetry]` enabled)",
            ));
        };
        let work = async move {
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
        .instrument(sp);
        super::run_scoped(key, work).await
    }

    async fn update_review(
        &self,
        request: Request<pb::UpdateReviewRequest>,
    ) -> Result<Response<pb::UpdateReviewReply>, Status> {
        // A write to a persisted draft: gated `Write` on the Review resource (S7: drafts, not the roster).
        // `review_id` is untrusted — the editor looks it up as a bound query arg and writes the
        // body to the draft's own `draft_path` (confined under the fleet root), never a
        // wire-supplied path; an over-cap body is rejected, a posted/approved draft is locked.
        super::authz::require(agent_core::Action::Write, agent_core::ResourceType::Review)?;
        let key = super::identity_key(request.metadata());
        let sp = span("fleet.update_review", request.metadata());
        // Opt-in: needs the draft-body editor (persisted history + a fleet root).
        let Some(editor) = self.editor.clone() else {
            return Err(Status::unimplemented(
                "UpdateReview requires the fleet draft editor \
                 (run `agent --serve-fleet` with `[telemetry]` enabled)",
            ));
        };
        let work = async move {
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
        .instrument(sp);
        super::run_scoped(key, work).await
    }
}

/// Bind a written row's owner to the caller's tenant.
///
/// A row's `user` is its **tenant** to the fleet: the workspace root, the metrics
/// label, and — load-bearing — the tenant the `ghapp:` mint gate checks when the row's
/// forge credential is minted (`mint_or_resolve_tenant_token(&row.user, …)`). So a
/// caller in tenant A that could store `user = "B"` (with a forge card naming one of
/// B's installations) would pass B's mint gate: a cross-tenant IDOR. An empty `user` is
/// filled with the caller's tenant; any other value must equal it, else
/// `PERMISSION_DENIED` (never a silent re-owning). With no tenant in scope (a
/// single-operator agent, no identity) the row is stored as sent.
#[allow(clippy::result_large_err)]
fn bind_owner(session: &mut agent_core::FleetSession, tenant: Option<&str>) -> Result<(), Status> {
    let Some(tenant) = tenant else {
        return Ok(());
    };
    if session.user.is_empty() {
        session.user = tenant.to_string();
    } else if session.user != tenant {
        return Err(Status::permission_denied(
            "a fleet session's user must be the caller's tenant",
        ));
    }
    Ok(())
}

pub fn review_fleet_router(inner: Arc<dyn FleetRegistry>) -> Router {
    Server::builder().add_service(ReviewFleetSvc::new(inner).into_server())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_proto::identity::{SESSION_ID_KEY, USER_ID_KEY};
    use pb::review_fleet_service_server::ReviewFleetService as _;
    use rstest::rstest;
    use std::sync::Mutex;
    use tonic::metadata::MetadataValue;

    // Records the ambient tenant (`current_identity().user`) at each store call — proves the
    // served handler scopes the caller into a `PerTenant`-wrapped fleet roster (C31 Principle 3).
    #[derive(Default)]
    struct TenantProbeFleet {
        seen: Arc<Mutex<Vec<Option<String>>>>,
    }
    impl TenantProbeFleet {
        fn record(&self) {
            self.seen
                .lock()
                .unwrap()
                .push(agent_core::current_identity().map(|k| k.user.as_str().to_string()));
        }
    }
    #[tonic::async_trait]
    impl FleetRegistry for TenantProbeFleet {
        async fn list(&self) -> agent_core::Result<Vec<agent_core::FleetSession>> {
            self.record();
            Ok(vec![])
        }
        async fn put(
            &self,
            session: agent_core::FleetSession,
        ) -> agent_core::Result<agent_core::FleetSession> {
            self.record();
            Ok(session)
        }
        async fn get(&self, _id: &str) -> agent_core::Result<agent_core::FleetSession> {
            unimplemented!()
        }
        async fn delete(&self, _id: &str) -> agent_core::Result<bool> {
            unimplemented!()
        }
        async fn set_enabled(
            &self,
            _id: &str,
            _enabled: bool,
        ) -> agent_core::Result<agent_core::FleetSession> {
            unimplemented!()
        }
    }

    fn req_with<T>(payload: T, user: Option<&str>, session: Option<&str>) -> Request<T> {
        let mut req = Request::new(payload);
        if let Some(u) = user {
            req.metadata_mut()
                .insert(USER_ID_KEY, MetadataValue::try_from(u).unwrap());
        }
        if let Some(s) = session {
            req.metadata_mut()
                .insert(SESSION_ID_KEY, MetadataValue::try_from(s).unwrap());
        }
        req
    }

    // Every caller-identity class → the tenant the store runs under. Present + path-safe
    // scopes; partial/hostile fails closed to the default tenant (`None`).
    #[rstest]
    #[case::positive_tenant_header_scopes_to_tenant(Some("acme".into()), Some("s1".into()), Some("acme".into()))]
    #[case::negative_no_identity_runs_as_local(None, None, None)]
    #[case::negative_second_tenant_is_isolated(Some("globex".into()), Some("s1".into()), Some("globex".into()))]
    #[case::boundary_max_len_tenant_segment(Some("a".repeat(128)), Some("s1".into()), Some("a".repeat(128)))]
    #[case::corner_user_without_session_runs_as_local(Some("acme".into()), None, None)]
    #[case::adversarial_traversal_header_fails_to_local(Some("../../heads/main".into()), Some("s1".into()), None)]
    #[case::adversarial_empty_header_fails_to_local(Some(String::new()), Some("s1".into()), None)]
    #[tokio::test]
    async fn fleet_list_scopes_caller_tenant(
        #[case] user: Option<String>,
        #[case] session: Option<String>,
        #[case] expected: Option<String>,
    ) {
        let store = Arc::new(TenantProbeFleet::default());
        let svc = ReviewFleetSvc::new(store.clone());
        let req = req_with(
            pb::FleetListRequest::default(),
            user.as_deref(),
            session.as_deref(),
        );
        svc.list(req).await.unwrap();
        assert_eq!(store.seen.lock().unwrap().as_slice(), &[expected]);
    }

    // The write path composes with the RBAC gate: with no verified principal `authz::require`
    // is a pass-through, and the handler still scopes the caller into the store.
    #[tokio::test]
    async fn fleet_put_scopes_under_authz_passthrough() {
        let store = Arc::new(TenantProbeFleet::default());
        let svc = ReviewFleetSvc::new(store.clone());
        let req = req_with(pb::FleetSession::default(), Some("acme"), Some("s1"));
        svc.put(req).await.unwrap();
        assert_eq!(
            store.seen.lock().unwrap().as_slice(),
            &[Some("acme".to_string())]
        );
    }

    fn admin(tenant: &str) -> agent_core::VerifiedPrincipal {
        agent_core::VerifiedPrincipal {
            tenant: tenant.into(),
            subject: "alice".into(),
            roles: vec![agent_core::ROLE_ORG_ADMIN.into()],
        }
    }

    // A written row's `user` (its tenant to the fleet, incl. the `ghapp:` mint gate) is
    // bound to the caller's tenant: empty ⇒ filled, equal ⇒ kept, anything else refused
    // before the store is touched. `principal` is the verified tenant (wins over the
    // self-asserted header); `stored` is `None` when the Put must be refused.
    #[rstest]
    #[case::positive_own_tenant_kept(None, Some("acme"), "acme", Some("acme"))]
    #[case::positive_empty_user_takes_tenant(None, Some("acme"), "", Some("acme"))]
    #[case::positive_verified_tenant_owns_row(Some("acme"), Some("acme"), "acme", Some("acme"))]
    #[case::negative_other_tenant_refused(None, Some("acme"), "globex", None)]
    #[case::corner_no_identity_stored_as_sent(None, None, "globex", Some("globex"))]
    #[case::corner_hostile_header_names_no_tenant(None, Some("../x"), "globex", Some("globex"))]
    #[case::boundary_case_differs_refused(None, Some("acme"), "ACME", None)]
    #[case::boundary_prefix_refused(None, Some("acme"), "acme2", None)]
    #[case::adversarial_header_cannot_claim_other_tenant(
        Some("acme"),
        Some("globex"),
        "globex",
        None
    )]
    #[case::adversarial_verified_tenant_fills_empty_not_header(
        Some("acme"),
        Some("globex"),
        "",
        Some("acme")
    )]
    #[case::adversarial_traversal_user_refused(None, Some("acme"), "../acme", None)]
    #[tokio::test]
    async fn fleet_put_binds_owner_to_caller_tenant(
        #[case] principal: Option<&str>,
        #[case] header: Option<&str>,
        #[case] user: &str,
        #[case] stored: Option<&str>,
    ) {
        let store = Arc::new(TenantProbeFleet::default());
        let svc = ReviewFleetSvc::new(store.clone());
        let row = pb::FleetSession {
            id: "s1".into(),
            user: user.into(),
            ..Default::default()
        };
        let req = req_with(row, header, header.map(|_| "s1"));
        let res = match principal {
            Some(t) => agent_core::principal_scope(admin(t), svc.put(req)).await,
            None => svc.put(req).await,
        };
        match stored {
            Some(want) => assert_eq!(res.expect("put").into_inner().user, want),
            None => {
                let err = res.expect_err("refused");
                assert_eq!(err.code(), tonic::Code::PermissionDenied);
                assert!(
                    store.seen.lock().unwrap().is_empty(),
                    "a refused Put never reaches the store"
                );
            }
        }
    }

    #[derive(Default)]
    struct RecordingSink {
        got: Mutex<Vec<FleetTrigger>>,
    }
    impl TriggerSink for RecordingSink {
        fn enqueue(&self, trigger: FleetTrigger) -> TriggerOutcome {
            self.got.lock().unwrap().push(trigger);
            TriggerOutcome::Accepted
        }
    }

    // `ReviewNow` stamps the trigger with the caller's tenant (forge-auth F6) so the fleet
    // drain resolves `session_id` in the roster the row was written under — never a tenant
    // named in the request. The verified principal wins over the self-asserted header;
    // a hostile or missing identity stamps "" (unscoped ⇒ the operator's own roster).
    #[rstest]
    #[case::positive_header_tenant_stamped(None, Some("acme"), Some("s1"), "acme")]
    #[case::positive_verified_tenant_stamped(Some("acme"), Some("acme"), Some("s1"), "acme")]
    #[case::negative_no_identity_is_unscoped(None, None, None, "")]
    #[case::corner_user_without_session_is_unscoped(None, Some("acme"), None, "")]
    #[case::boundary_max_len_tenant(None, Some(&*"a".repeat(128)), Some("s1"), &*"a".repeat(128))]
    #[case::adversarial_header_cannot_name_other_tenant(
        Some("acme"),
        Some("globex"),
        Some("s1"),
        "acme"
    )]
    #[case::adversarial_traversal_header_is_unscoped(None, Some("../../x"), Some("s1"), "")]
    #[tokio::test]
    async fn review_now_stamps_caller_tenant(
        #[case] principal: Option<&str>,
        #[case] header: Option<&str>,
        #[case] session: Option<&str>,
        #[case] want: &str,
    ) {
        let sink = Arc::new(RecordingSink::default());
        let svc =
            ReviewFleetSvc::new(Arc::new(TenantProbeFleet::default())).with_triggers(sink.clone());
        let req = req_with(
            pb::ReviewNowRequest {
                session_id: "row1".into(),
                pr_number: 7,
            },
            header,
            session,
        );
        let res = match principal {
            Some(t) => agent_core::principal_scope(admin(t), svc.review_now(req)).await,
            None => svc.review_now(req).await,
        };
        assert!(res.expect("review_now").into_inner().accepted);
        let got = sink.got.lock().unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(
            (
                got[0].tenant.as_str(),
                got[0].session_id.as_str(),
                got[0].pr_number
            ),
            (want, "row1", 7)
        );
    }
}
