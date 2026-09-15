//! The `FleetRegistry` seam over the wire (review-fleet C3). Fails **hard**: a
//! transport or validation error surfaces as an `Err` rather than degrading — a
//! control-plane read/write the operator asked for should not silently no-op.
//! Everything a remote roster returns is untrusted: the shared wire→core decode
//! clamps every number before a row is used. `token_ref` rides as a reference;
//! nothing here resolves it.

use agent_core::{
    ApproveOutcome, DraftBody, Error, FleetRegistry, FleetSession, Result, ReviewDraftFilter,
    ReviewDraftRecord, UpdateOutcome,
};
use agent_proto::pb;
use async_trait::async_trait;
use tonic::transport::Channel;

use super::{call_retry, grpc_retry_policy, outbound};
use crate::transport::Endpoint;

pub struct GrpcFleet {
    client: pb::review_fleet_service_client::ReviewFleetServiceClient<Channel>,
    retry: agent_retry::RetryPolicy,
}

impl GrpcFleet {
    pub fn connect(endpoint: &Endpoint) -> Result<Self> {
        let channel = endpoint
            .connect_lazy()
            .map_err(|e| Error::Fleet(e.to_string()))?;
        Ok(Self {
            client: pb::review_fleet_service_client::ReviewFleetServiceClient::new(channel),
            retry: grpc_retry_policy(),
        })
    }

    /// Manually queue a review (review-fleet C8). Not part of the [`FleetRegistry`]
    /// seam (roster CRUD) — it drives the orchestrator, served only by the full
    /// `--serve-fleet` process. Returns whether the trigger queued (`true`) or coalesced
    /// into a pending/in-flight one (`false`); a bare control-plane endpoint (no
    /// orchestrator) answers `UNIMPLEMENTED`.
    pub async fn review_now(&self, session_id: &str, pr_number: u64) -> Result<bool> {
        let req = pb::ReviewNowRequest {
            session_id: session_id.to_string(),
            pr_number,
        };
        let resp = unary!(self, review_now, req).map_err(status_to_err)?;
        Ok(resp.into_inner().accepted)
    }

    /// Approve a persisted draft → post it to its forge (review-fleet C17). The human
    /// approval gesture: the only path that posts. Idempotent — a second call on a posted
    /// draft returns [`ApproveOutcome::AlreadyPosted`]. Served only by the full
    /// `--serve-fleet` process with persisted history; a bare control plane (or one with no
    /// history) answers `UNIMPLEMENTED`. The wire `status` string is mapped back to the
    /// [`ApproveOutcome`]; an unrecognized status is a protocol fault (`Err`).
    pub async fn approve(&self, review_id: &str) -> Result<ApproveOutcome> {
        let req = pb::ApproveRequest {
            review_id: review_id.to_string(),
        };
        let resp = unary!(self, approve, req)
            .map_err(status_to_err)?
            .into_inner();
        match resp.status.as_str() {
            "posted" => Ok(ApproveOutcome::Posted { url: resp.detail }),
            "already_posted" => Ok(ApproveOutcome::AlreadyPosted),
            "not_found" => Ok(ApproveOutcome::NotFound),
            other => Err(Error::Fleet(format!(
                "approve: unrecognized status {other:?} from server"
            ))),
        }
    }

    /// Operational self-diagnosis of a running fleet (docs/design/doctor/): run the
    /// process's probe set and return the report. Served only by the full
    /// `--serve-fleet` process; a bare control plane answers `UNIMPLEMENTED`. Each
    /// wire status string maps back to a [`ProbeStatus`](agent_core::ProbeStatus);
    /// an unrecognized one is a protocol fault (`Err`) — fail closed on an untrusted
    /// server rather than silently dropping a probe.
    pub async fn preflight(&self) -> Result<agent_core::DoctorReport> {
        let resp = unary!(self, preflight, pb::PreflightRequest {})
            .map_err(status_to_err)?
            .into_inner();
        let mut probes = Vec::with_capacity(resp.probes.len());
        for p in resp.probes {
            let status = agent_core::ProbeStatus::parse(&p.status).ok_or_else(|| {
                Error::Fleet(format!(
                    "preflight: unrecognized probe status {:?} from server",
                    p.status
                ))
            })?;
            probes.push(agent_core::ProbeOutcome {
                name: p.name,
                status,
                detail: p.detail,
                latency_ms: p.latency_ms,
            });
        }
        Ok(agent_core::DoctorReport { probes })
    }

    /// List persisted review drafts (review-fleet C14), newest state per review. Read-only.
    /// Served only by a process with persisted fleet history; a process without it answers
    /// `UNIMPLEMENTED`. Every filter value rides as a bound query arg server-side; the server
    /// caps the row count. The returned records carry an EMPTY `draft_path` — the wire summary
    /// never exposes the server-minted path; fetch a body via [`Self::get_review`].
    pub async fn list_reviews(&self, filter: &ReviewDraftFilter) -> Result<Vec<ReviewDraftRecord>> {
        let req = pb::ListReviewsRequest {
            repo: filter.repo.clone().unwrap_or_default(),
            session_id: filter.session_id.clone().unwrap_or_default(),
            status: filter.status.clone().unwrap_or_default(),
            limit: filter.limit.min(u32::MAX as usize) as u32,
        };
        let resp = unary!(self, list_reviews, req)
            .map_err(status_to_err)?
            .into_inner();
        Ok(resp.reviews.into_iter().map(record_from_summary).collect())
    }

    /// Fetch one draft's metadata + rendered markdown body (review-fleet C14). Read-only.
    /// Returns `None` when there is no persisted draft for `review_id` (the server's
    /// `NotFound`), `Err` on a genuine fault, and `UNIMPLEMENTED` maps to `Err`. Served only by
    /// a process with the draft reader wired. The returned record's `draft_path` is empty (see
    /// [`Self::list_reviews`]).
    pub async fn get_review(&self, review_id: &str) -> Result<Option<DraftBody>> {
        let req = pb::GetReviewRequest {
            review_id: review_id.to_string(),
        };
        match unary!(self, get_review, req) {
            Ok(resp) => {
                let resp = resp.into_inner();
                let record = resp.meta.map(record_from_summary).ok_or_else(|| {
                    Error::Fleet("get_review: reply missing draft metadata".into())
                })?;
                Ok(Some(DraftBody {
                    record,
                    body: resp.body,
                    truncated: resp.truncated,
                }))
            }
            // An absent draft is a total outcome, not a transport fault.
            Err(s) if s.code() == tonic::Code::NotFound => Ok(None),
            Err(s) => Err(status_to_err(s)),
        }
    }

    /// Rewrite a draft's markdown body (review-fleet C14, the portal's edit). Returns the
    /// [`UpdateOutcome`]: `Updated`, `NotFound`, or `Locked` (a `posted`/`approved` draft). The
    /// wire reply carries only the outcome word, so a `Locked` outcome's `status` is empty
    /// client-side (the portal only needs "it's locked"). Served only by a process with the
    /// editor wired; a process without it answers `UNIMPLEMENTED` (an `Err`). An over-cap body
    /// or unwritable file is a genuine fault (`Err`). Editing never posts.
    pub async fn update_review(&self, review_id: &str, body: &str) -> Result<UpdateOutcome> {
        let req = pb::UpdateReviewRequest {
            review_id: review_id.to_string(),
            body: body.to_string(),
        };
        let resp = unary!(self, update_review, req)
            .map_err(status_to_err)?
            .into_inner();
        match resp.status.as_str() {
            "updated" => Ok(UpdateOutcome::Updated),
            "not_found" => Ok(UpdateOutcome::NotFound),
            "locked" => Ok(UpdateOutcome::Locked {
                status: String::new(),
            }),
            other => Err(Error::Fleet(format!(
                "update_review: unrecognized status {other:?} from server"
            ))),
        }
    }
}

/// A wire `ReviewSummary` → a domain [`ReviewDraftRecord`]. The summary omits the server-minted
/// `draft_path` (never exposed on the wire), so the record carries an empty path — the body is
/// fetched via `GetReview`, not by re-reading this path client-side. Numbers arrive typed
/// (proto scalars), so there is nothing further to clamp.
fn record_from_summary(s: pb::ReviewSummary) -> ReviewDraftRecord {
    ReviewDraftRecord {
        review_id: s.review_id,
        repo: s.repo,
        pr_number: s.pr_number,
        head_sha: s.head_sha,
        risk_score: s.risk_score,
        gate_failed: s.gate_failed,
        n_findings: s.n_findings,
        files_changed: s.files_changed,
        additions: s.additions,
        deletions: s.deletions,
        draft_path: String::new(),
        status: s.status,
    }
}

#[async_trait]
impl FleetRegistry for GrpcFleet {
    async fn list(&self) -> Result<Vec<FleetSession>> {
        let resp = unary!(self, list, pb::FleetListRequest {}).map_err(status_to_err)?;
        Ok(resp
            .into_inner()
            .sessions
            .into_iter()
            .map(FleetSession::from) // decode clamps hostile numbers
            .collect())
    }

    async fn get(&self, id: &str) -> Result<FleetSession> {
        let req = pb::FleetSessionRef { id: id.to_string() };
        let resp = unary!(self, get, req).map_err(status_to_err)?;
        Ok(resp.into_inner().into())
    }

    async fn put(&self, session: FleetSession) -> Result<FleetSession> {
        let req = pb::FleetSession::from(session);
        let resp = unary!(self, put, req).map_err(status_to_err)?;
        Ok(resp.into_inner().into())
    }

    async fn delete(&self, id: &str) -> Result<bool> {
        let req = pb::FleetSessionRef { id: id.to_string() };
        let resp = unary!(self, delete, req).map_err(status_to_err)?;
        Ok(resp.into_inner().deleted)
    }

    async fn set_enabled(&self, id: &str, enabled: bool) -> Result<FleetSession> {
        let req = pb::FleetSetEnabledRequest {
            id: id.to_string(),
            enabled,
        };
        let resp = unary!(self, set_enabled, req).map_err(status_to_err)?;
        Ok(resp.into_inner().into())
    }
}

fn status_to_err(s: tonic::Status) -> Error {
    // Preserve the seam's `not found` contract across the wire, so a chained
    // roster (grpc → grpc) still maps to NotFound at the outer hop.
    let m = s.message();
    if s.code() == tonic::Code::NotFound {
        Error::Fleet(format!(
            "not found: {}",
            m.strip_prefix("fleet: not found: ").unwrap_or(m)
        ))
    } else {
        Error::Fleet(m.to_string())
    }
}
