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
            "skipped" => Ok(ApproveOutcome::Skipped),
            other => Err(Error::Fleet(format!(
                "approve: unrecognized status {other:?} from server"
            ))),
        }
    }

    /// Skip a persisted draft so it is never posted. Shares Approve's post lease
    /// server-side. An unrecognized wire status is a protocol fault (`Err`).
    pub async fn skip_review(&self, review_id: &str) -> Result<agent_core::SkipOutcome> {
        let req = pb::SkipReviewRequest {
            review_id: review_id.to_string(),
        };
        let status = unary!(self, skip_review, req)
            .map_err(status_to_err)?
            .into_inner()
            .status;
        agent_core::SkipOutcome::from_wire(&status).ok_or_else(|| {
            Error::Fleet(format!("skip: unrecognized status {status:?} from server"))
        })
    }

    /// Skip several drafts in one call (at most [`agent_core::MAX_SKIP_BATCH`] ids; the
    /// server rejects more). Returns each distinct id with its outcome, in request order.
    /// The reply must answer each distinct id exactly once ([`check_skip_results`]).
    pub async fn skip_reviews(
        &self,
        review_ids: &[String],
    ) -> Result<Vec<(String, agent_core::SkipOutcome)>> {
        let req = pb::SkipReviewsRequest {
            review_ids: review_ids.to_vec(),
        };
        let results = unary!(self, skip_reviews, req)
            .map_err(status_to_err)?
            .into_inner()
            .results;
        check_skip_results(review_ids, results)
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
            exclude_status: filter.exclude_status.clone(),
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
        requested_by: s.requested_by,
        approved_by: s.approved_by,
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

/// Check a `SkipReviews` reply against the ids asked for, in reply order. The server is
/// untrusted: a result for an id not asked for, a second result for one id, an id left
/// unanswered, or an unrecognized status is a protocol fault (`Err`), so a tally built
/// from the reply can't over- or under-count.
fn check_skip_results(
    asked: &[String],
    results: Vec<pb::SkipResult>,
) -> Result<Vec<(String, agent_core::SkipOutcome)>> {
    use std::collections::HashSet;
    let cut = |s: &str| {
        s.chars()
            .take(agent_core::MAX_ECHO_CHARS)
            .collect::<String>()
    };
    let mut open: HashSet<&str> = asked.iter().map(String::as_str).collect();
    let mut out = Vec::with_capacity(results.len());
    for r in results {
        if !open.remove(r.review_id.as_str()) {
            let why = if asked.contains(&r.review_id) {
                "a second result"
            } else {
                "a result for an id not asked for"
            };
            return Err(Error::Fleet(format!(
                "skip: {why}: {:?}",
                cut(&r.review_id)
            )));
        }
        let outcome = agent_core::SkipOutcome::from_wire(&r.status).ok_or_else(|| {
            Error::Fleet(format!(
                "skip: unrecognized status {:?} from server",
                cut(&r.status)
            ))
        })?;
        out.push((r.review_id, outcome));
    }
    if let Some(missing) = open.iter().min() {
        return Err(Error::Fleet(format!(
            "skip: no result for {:?} ({} unanswered)",
            cut(missing),
            open.len()
        )));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::SkipOutcome;
    use rstest::rstest;

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(ToString::to_string).collect()
    }

    fn res(v: &[(&str, &str)]) -> Vec<pb::SkipResult> {
        v.iter()
            .map(|(id, status)| pb::SkipResult {
                review_id: (*id).into(),
                status: (*status).into(),
            })
            .collect()
    }

    /// A `SkipReviews` reply checked against the request: description, asked ids, reply,
    /// expected outcomes in reply order or an error fragment.
    #[rstest]
    #[case::positive_every_id_answered(
        "each asked id answered once",
        ids(&["a", "b"]),
        res(&[("a", "skipped"), ("b", "already_posted")]),
        Ok(vec![("a", SkipOutcome::Skipped), ("b", SkipOutcome::AlreadyPosted)])
    )]
    #[case::corner_duplicate_asks_one_answer(
        "an id asked twice is answered once (the server dedups)",
        ids(&["a", "a"]),
        res(&[("a", "skipped")]),
        Ok(vec![("a", SkipOutcome::Skipped)])
    )]
    #[case::corner_reply_order_kept(
        "outcomes follow the reply's order",
        ids(&["a", "b"]),
        res(&[("b", "not_found"), ("a", "already_skipped")]),
        Ok(vec![("b", SkipOutcome::NotFound), ("a", SkipOutcome::AlreadySkipped)])
    )]
    #[case::boundary_empty_both("nothing asked, nothing answered", ids(&[]), res(&[]), Ok(vec![]))]
    #[case::negative_missing_result(
        "an asked id with no result is a fault, not a short tally",
        ids(&["a", "b"]),
        res(&[("a", "skipped")]),
        Err("no result for \"b\"")
    )]
    #[case::negative_unknown_status(
        "an unrecognized status is a fault",
        ids(&["a"]),
        res(&[("a", "obliterated")]),
        Err("unrecognized status")
    )]
    #[case::adversarial_unasked_id(
        "a result for an id never asked for",
        ids(&["a"]),
        res(&[("a", "skipped"), ("zzz", "skipped")]),
        Err("not asked for")
    )]
    #[case::adversarial_repeated_result(
        "one id answered twice would double-count",
        ids(&["a", "b"]),
        res(&[("a", "skipped"), ("a", "skipped"), ("b", "skipped")]),
        Err("a second result")
    )]
    #[case::adversarial_huge_status_cut(
        "a huge hostile status is cut before it is echoed",
        ids(&["a"]),
        vec![pb::SkipResult { review_id: "a".into(), status: "x".repeat(10_000) }],
        Err("unrecognized status")
    )]
    fn check_skip_results_cases(
        #[case] description: &str,
        #[case] asked: Vec<String>,
        #[case] reply: Vec<pb::SkipResult>,
        #[case] expected: std::result::Result<Vec<(&str, SkipOutcome)>, &str>,
    ) {
        let got = check_skip_results(&asked, reply);
        match expected {
            Ok(want) => {
                let want: Vec<(String, SkipOutcome)> =
                    want.into_iter().map(|(i, o)| (i.to_string(), o)).collect();
                assert_eq!(got.expect(description), want, "{description}");
            }
            Err(frag) => {
                let msg = got.expect_err(description).to_string();
                assert!(msg.contains(frag), "{description}: {msg}");
                assert!(msg.len() < 200, "{description}: echo is cut: {}", msg.len());
            }
        }
    }
}
