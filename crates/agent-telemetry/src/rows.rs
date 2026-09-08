//! ClickHouse row types (native protocol), matching `nix/clickhouse/schema.sql`
//! column-for-column. `klickhouse::Row` maps struct fields to columns by name.
//! `ts` is `DateTime64(3, 'UTC')`, built from a unix-millis timestamp.

use agent_core::MemoryEvent;
use klickhouse::{DateTime64, Row, Tz};
use std::time::{SystemTime, UNIX_EPOCH};

/// Milliseconds since the unix epoch, for `agent_logs` timestamps.
pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn dt64_from_ms(ms: u64) -> DateTime64<3> {
    DateTime64::<3>(Tz::UTC, ms)
}

/// One row of the full transaction history (`agent_events`).
#[derive(Debug, Clone, Row)]
pub struct EventRow {
    pub session_id: String,
    /// The verified owning identity (`SessionKey.user`; tenant == user at this
    /// tier), stamped at the emit funnel. Empty for events emitted outside a scope.
    pub user: String,
    pub ts: DateTime64<3>,
    pub seq: u32,
    pub kind: String,
    pub role: String,
    pub content: String,
    pub tool_calls: String,
    pub tool_call_id: String,
}

impl EventRow {
    /// Build an event row from a recorded `MemoryEvent` (non-usage kinds).
    pub fn from_event(event: &MemoryEvent, seq: u32) -> Self {
        let tool_calls = if event.message.tool_calls.is_empty() {
            String::new()
        } else {
            serde_json::to_string(&event.message.tool_calls).unwrap_or_default()
        };
        Self {
            session_id: event.session_id.clone(),
            user: event.user.clone(),
            ts: dt64_from_ms(event.ts_ms),
            seq,
            kind: event.kind.clone(),
            role: event.message.role.as_str().to_string(),
            // The telemetry row is a flat text column; media blocks are
            // summarized by `content_text` rather than base64'd into ClickHouse.
            content: event.message.content_text(),
            tool_calls,
            tool_call_id: event.message.tool_call_id.clone().unwrap_or_default(),
        }
    }
}

/// One streamed tracing/log event (`agent_logs`).
#[derive(Debug, Clone, Row)]
pub struct LogRow {
    pub session_id: String,
    /// The verified owning identity (`SessionKey.user`; tenant == user at this
    /// tier), stamped at the emit funnel. Empty for events emitted outside a scope.
    pub user: String,
    pub ts: DateTime64<3>,
    pub level: String,
    pub target: String,
    pub message: String,
    pub fields: String,
}

impl LogRow {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        session_id: String,
        user: String,
        level: String,
        target: String,
        message: String,
        fields: String,
    ) -> Self {
        Self {
            session_id,
            user,
            ts: dt64_from_ms(now_ms()),
            level,
            target,
            message,
            fields,
        }
    }
}

/// One per-turn token usage record (`agent_usage`).
#[derive(Debug, Clone, Row)]
pub struct UsageRow {
    pub session_id: String,
    /// The verified owning identity (`SessionKey.user`; tenant == user at this
    /// tier), stamped at the emit funnel. Empty for events emitted outside a scope.
    pub user: String,
    pub ts: DateTime64<3>,
    pub iter: u32,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

impl UsageRow {
    /// Build a usage row from a `kind = "usage"` `MemoryEvent`.
    pub fn from_event(event: &MemoryEvent) -> Option<Self> {
        let usage = event.usage.as_ref()?;
        Some(Self {
            session_id: event.session_id.clone(),
            user: event.user.clone(),
            ts: dt64_from_ms(event.ts_ms),
            iter: event.iter.unwrap_or(0),
            prompt_tokens: usage.prompt_tokens,
            completion_tokens: usage.completion_tokens,
            total_tokens: usage.total_tokens,
        })
    }
}

/// One tool-call verification (`agent_verifications`). `Nullable(UInt8)` columns
/// map to `Option<u8>`; the `bool`s are stored `UInt8` (0/1).
#[derive(Debug, Clone, Row)]
pub struct VerificationRow {
    pub session_id: String,
    /// The verified owning identity (`SessionKey.user`; tenant == user at this
    /// tier), stamped at the emit funnel. Empty for events emitted outside a scope.
    pub user: String,
    pub ts: DateTime64<3>,
    pub iter: u32,
    pub tool_name: String,
    pub args_hash: String,
    pub goal_hash: String,
    pub task_type: String,
    pub verifier_model: String,
    pub verifier_cfg: String,
    pub verdict: String,
    pub confidence: f32,
    pub latency_ms: u32,
    pub cached: u8,
    pub call_errored: Option<u8>,
    pub revised_after: Option<u8>,
    pub task_succeeded: Option<u8>,
}

impl VerificationRow {
    /// Build a verification row from a `kind = "verification"` `MemoryEvent`.
    pub fn from_event(event: &MemoryEvent) -> Option<Self> {
        let v = event.verification.as_ref()?;
        Some(Self {
            session_id: event.session_id.clone(),
            user: event.user.clone(),
            ts: dt64_from_ms(event.ts_ms),
            iter: event.iter.unwrap_or(0),
            tool_name: v.tool_name.clone(),
            args_hash: v.args_hash.clone(),
            goal_hash: v.goal_hash.clone(),
            task_type: v.task_type.clone(),
            verifier_model: v.verifier_model.clone(),
            verifier_cfg: v.verifier_cfg.clone(),
            verdict: v.verdict.clone(),
            confidence: v.confidence,
            latency_ms: v.latency_ms,
            cached: v.cached as u8,
            call_errored: v.call_errored.map(|b| b as u8),
            revised_after: v.revised_after.map(|b| b as u8),
            task_succeeded: v.task_succeeded.map(|b| b as u8),
        })
    }
}

/// One review run (`agent_reviews`) — the headline row. `is_fork` is stored `UInt8`.
#[derive(Debug, Clone, Row)]
pub struct ReviewRow {
    pub session_id: String,
    /// The verified owning identity (`SessionKey.user`; tenant == user at this
    /// tier), stamped at the emit funnel. Empty for events emitted outside a scope.
    pub user: String,
    pub ts: DateTime64<3>,
    pub repo_hash: String,
    pub base_rev: String,
    pub head_rev: String,
    pub mode_via: String,
    pub project: String,
    pub is_fork: u8,
    pub changed_files: u32,
    pub findings: u32,
    pub findings_in_diff: u32,
    pub summaries: u32,
    pub total_ms: u32,
    pub sum_work_ms: u32,
    pub critical_path: String,
}

impl ReviewRow {
    /// Build the headline row from a `kind = "review"` `MemoryEvent`.
    pub fn from_event(event: &MemoryEvent) -> Option<Self> {
        let r = event.review.as_ref()?;
        Some(Self {
            session_id: event.session_id.clone(),
            user: event.user.clone(),
            ts: dt64_from_ms(event.ts_ms),
            repo_hash: r.repo_hash.clone(),
            base_rev: r.base_rev.clone(),
            head_rev: r.head_rev.clone(),
            mode_via: r.mode_via.clone(),
            project: r.project.clone(),
            is_fork: r.is_fork as u8,
            changed_files: r.changed_files,
            findings: r.findings,
            findings_in_diff: r.findings_in_diff,
            summaries: r.summaries,
            total_ms: r.total_ms,
            sum_work_ms: r.sum_work_ms,
            critical_path: r.critical_path.clone(),
        })
    }
}

/// One fleet review draft (`agent_review_drafts`, review-fleet C14) — the operational
/// record: per-PR draft state, the head-oid dedup key, summary stats, and the `.md` path.
/// Named `repo`/`pr_number` (fleet config, not model-derived), unlike the anonymized
/// [`ReviewRow`]; joins to `agent_reviews` on `head_sha == head_rev`. `gate_failed` is
/// stored `UInt8`.
#[derive(Debug, Clone, Row)]
pub struct ReviewDraftRow {
    pub session_id: String,
    /// The verified owning identity (`SessionKey.user`; tenant == user at this tier).
    pub user: String,
    pub ts: DateTime64<3>,
    pub review_id: String,
    pub repo: String,
    pub pr_number: u64,
    pub head_sha: String,
    pub risk_score: f64,
    pub gate_failed: u8,
    pub n_findings: u32,
    pub files_changed: u32,
    pub additions: u32,
    pub deletions: u32,
    pub draft_path: String,
    pub status: String,
}

impl ReviewDraftRow {
    /// Build the draft row from a `kind = "draft"` `MemoryEvent`.
    pub fn from_event(event: &MemoryEvent) -> Option<Self> {
        let d = event.draft.as_ref()?;
        Some(Self {
            session_id: event.session_id.clone(),
            user: event.user.clone(),
            ts: dt64_from_ms(event.ts_ms),
            review_id: d.review_id.clone(),
            repo: d.repo.clone(),
            pr_number: d.pr_number,
            head_sha: d.head_sha.clone(),
            risk_score: d.risk_score,
            gate_failed: d.gate_failed as u8,
            n_findings: d.n_findings,
            files_changed: d.files_changed,
            additions: d.additions,
            deletions: d.deletions,
            draft_path: d.draft_path.clone(),
            status: d.status.clone(),
        })
    }
}

/// One fleet review-feedback item (`agent_review_feedback`, review-fleet C15) — one row per
/// item, carried across rounds by the cross-round tracker (C16). `review_id`/`repo`/
/// `pr_number` are the persisting round's context; the rest is the item's lifecycle. Joins to
/// [`ReviewDraftRow`] on `(repo, pr_number)`.
#[derive(Debug, Clone, Row)]
pub struct ReviewFeedbackRow {
    pub session_id: String,
    /// The verified owning identity (`SessionKey.user`; tenant == user at this tier).
    pub user: String,
    pub ts: DateTime64<3>,
    pub item_id: String,
    pub review_id: String,
    pub repo: String,
    pub pr_number: u64,
    pub category: String,
    pub severity: String,
    pub title: String,
    pub body: String,
    pub status: String,
    pub first_seen_review: String,
    pub first_seen_sha: String,
    pub addressed_review: String,
    pub addressed_sha: String,
}

impl ReviewFeedbackRow {
    /// One row per item in a `kind = "feedback"` `MemoryEvent`.
    pub fn rows_from_event(event: &MemoryEvent) -> Vec<Self> {
        let Some(round) = event.feedback.as_ref() else {
            return Vec::new();
        };
        let ts = dt64_from_ms(event.ts_ms);
        round
            .items
            .iter()
            .map(|it| ReviewFeedbackRow {
                session_id: event.session_id.clone(),
                user: event.user.clone(),
                ts,
                item_id: it.item_id.clone(),
                review_id: round.review_id.clone(),
                repo: round.repo.clone(),
                pr_number: round.pr_number,
                category: it.category.clone(),
                severity: it.severity.clone(),
                title: it.title.clone(),
                body: it.body.clone(),
                status: it.status.clone(),
                first_seen_review: it.first_seen_review.clone(),
                first_seen_sha: it.first_seen_sha.clone(),
                addressed_review: it.addressed_review.clone(),
                addressed_sha: it.addressed_sha.clone(),
            })
            .collect()
    }
}

/// One collector per review (`agent_review_collectors`) — the parallelism drill-down.
#[derive(Debug, Clone, Row)]
pub struct ReviewCollectorRow {
    pub session_id: String,
    /// The verified owning identity (`SessionKey.user`; tenant == user at this
    /// tier), stamped at the emit funnel. Empty for events emitted outside a scope.
    pub user: String,
    pub ts: DateTime64<3>,
    pub collector: String,
    pub status: String,
    pub duration_ms: u32,
    pub items: u32,
}

impl ReviewCollectorRow {
    /// One row per collector in a `kind = "review"` `MemoryEvent`. `items` reflects
    /// the well-known collectors' aggregate counts (0 otherwise — a per-collector
    /// count isn't carried on `CollectorStatus`).
    pub fn rows_from_event(event: &MemoryEvent) -> Vec<Self> {
        let Some(r) = event.review.as_ref() else {
            return Vec::new();
        };
        let ts = dt64_from_ms(event.ts_ms);
        r.collectors
            .iter()
            .map(|c| {
                let items = match c.collector.as_str() {
                    "analyzer" => r.findings,
                    "summaries" => r.summaries,
                    "repo-change" => r.changed_files,
                    _ => 0,
                };
                ReviewCollectorRow {
                    session_id: event.session_id.clone(),
                    user: event.user.clone(),
                    ts,
                    collector: c.collector.clone(),
                    status: c.status.as_str().to_string(),
                    duration_ms: c.duration_ms,
                    items,
                }
            })
            .collect()
    }
}

/// One per-dimension summary (`agent_dimension_summaries`) — adaptive-cognition 03.
/// `summary_len` (not the body) is stored: counts/lengths only, never the text.
#[derive(Debug, Clone, Row)]
pub struct DimensionRow {
    pub session_id: String,
    /// The verified owning identity (`SessionKey.user`; tenant == user at this
    /// tier), stamped at the emit funnel. Empty for events emitted outside a scope.
    pub user: String,
    pub ts: DateTime64<3>,
    pub dimension: String,
    pub is_new: u8,
    pub summary_len: u32,
}

impl DimensionRow {
    /// One row per accepted summary in a `kind = "dimension"` `MemoryEvent`.
    pub fn rows_from_event(event: &MemoryEvent) -> Vec<Self> {
        let Some(d) = event.dimensional.as_ref() else {
            return Vec::new();
        };
        let ts = dt64_from_ms(event.ts_ms);
        d.summaries
            .iter()
            .map(|s| DimensionRow {
                session_id: event.session_id.clone(),
                user: event.user.clone(),
                ts,
                dimension: s.dimension.clone(),
                is_new: s.is_new as u8,
                summary_len: s.summary.chars().count() as u32,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{Message, ToolCall, Usage, VerificationRecord};
    use rstest::rstest;
    use serde_json::json;

    fn ev(kind: &str, message: Message, usage: Option<Usage>) -> MemoryEvent {
        MemoryEvent {
            kind: kind.into(),
            message,
            ts_ms: 1,
            session_id: "s".into(),
            user: "u".into(),
            usage,
            iter: Some(2),
            verification: None,
            review: None,
            dimensional: None,
            draft: None,
            feedback: None,
        }
    }

    fn verification(rec: VerificationRecord) -> MemoryEvent {
        MemoryEvent {
            kind: "verification".into(),
            message: Message::assistant(""),
            ts_ms: 1,
            session_id: "s".into(),
            user: "u".into(),
            usage: None,
            iter: Some(7),
            verification: Some(rec),
            review: None,
            dimensional: None,
            draft: None,
            feedback: None,
        }
    }

    fn sample_record() -> VerificationRecord {
        VerificationRecord {
            tool_name: "bash".into(),
            args_hash: "aaaa".into(),
            goal_hash: "bbbb".into(),
            task_type: "bash".into(),
            verifier_model: "schema".into(),
            verifier_cfg: "{\"name\":\"schema\",\"mode\":\"shadow\"}".into(),
            verdict: "revise".into(),
            confidence: 1.0,
            latency_ms: 3,
            cached: false,
            call_errored: Some(true),
            revised_after: None,
            task_succeeded: None,
        }
    }

    // --- EventRow: role/content extraction + blank tool_calls --------------
    #[rstest]
    #[case::user(ev("goal", Message::user("hello"), None), "user", "hello")]
    #[case::system(ev("x", Message::system("sys"), None), "system", "sys")]
    #[case::assistant_empty(ev("assistant", Message::assistant(""), None), "assistant", "")]
    fn event_row_role_and_content_cases(
        #[case] event: MemoryEvent,
        #[case] role: &str,
        #[case] content: &str,
    ) {
        let row = EventRow::from_event(&event, 0);
        assert_eq!(row.role, role);
        assert_eq!(row.content, content);
        assert_eq!(row.tool_calls, ""); // no tool calls ⇒ blank
    }

    #[test]
    fn event_row_serializes_tool_calls() {
        let mut msg = Message::assistant("");
        msg.tool_calls = vec![ToolCall {
            id: "call_1".into(),
            name: "bash".into(),
            arguments: json!({ "command": "ls" }),
        }];
        let row = EventRow::from_event(&ev("assistant", msg, None), 3);
        assert_eq!(row.seq, 3);
        assert_eq!(row.session_id, "s");
        assert!(row.tool_calls.contains("bash"));
    }

    // --- UsageRow: present ⇒ Some(tokens); absent ⇒ None ------------------
    #[rstest]
    #[case::present(Some(Usage { prompt_tokens: 10, completion_tokens: 5, total_tokens: 15, ..Default::default() }), Some((10, 15)))]
    #[case::absent(None, None)]
    fn usage_row_cases(#[case] usage: Option<Usage>, #[case] expected: Option<(u32, u32)>) {
        let row = UsageRow::from_event(&ev("usage", Message::assistant(""), usage));
        match (row, expected) {
            (Some(r), Some((prompt, total))) => {
                assert_eq!(r.prompt_tokens, prompt);
                assert_eq!(r.total_tokens, total);
                assert_eq!(r.iter, 2);
            }
            (None, None) => {}
            (r, exp) => panic!("got Some={}, expected {exp:?}", r.is_some()),
        }
    }

    // --- VerificationRow: envelope + payload mapping ----------------------
    #[test]
    fn positive_verification_row_maps_payload_and_envelope() {
        let row = VerificationRow::from_event(&verification(sample_record()))
            .expect("verification present");
        assert_eq!(row.session_id, "s");
        assert_eq!(row.iter, 7); // from the envelope, not the record
        assert_eq!(row.tool_name, "bash");
        assert_eq!(row.verdict, "revise");
        assert_eq!(row.cached, 0); // bool false ⇒ UInt8 0
        assert_eq!(row.call_errored, Some(1)); // Some(true) ⇒ Some(1)
        assert_eq!(row.revised_after, None); // deferred proxy stays NULL
        assert_eq!(row.task_succeeded, None);
    }

    // A non-verification event carries no record ⇒ no row (mirrors UsageRow).
    #[test]
    fn negative_verification_row_absent_without_record() {
        assert!(VerificationRow::from_event(&ev("tool", Message::assistant(""), None)).is_none());
    }

    // Outcome proxy `None` (a blocked call that never ran) maps to a NULL column.
    #[test]
    fn boundary_verification_row_none_call_errored_is_null() {
        let mut rec = sample_record();
        rec.call_errored = None;
        let row = VerificationRow::from_event(&verification(rec)).unwrap();
        assert_eq!(row.call_errored, None);
    }

    // Adversarial: the payload is model-derived. A non-finite confidence must not
    // reach the row (it is clamped at the source, but assert the row is finite so a
    // regression that skips clamping is caught here too, before ClickHouse).
    #[test]
    fn adversarial_verification_row_confidence_is_finite() {
        let row = VerificationRow::from_event(&verification(sample_record())).unwrap();
        assert!(row.confidence.is_finite() && (0.0..=1.0).contains(&row.confidence));
    }

    // --- ReviewRow / ReviewCollectorRow ------------------------------------
    fn review_event(rec: agent_core::ReviewRecord) -> MemoryEvent {
        MemoryEvent {
            kind: "review".into(),
            message: Message::assistant(""),
            ts_ms: 5,
            session_id: "s".into(),
            user: "u".into(),
            usage: None,
            iter: None,
            verification: None,
            review: Some(rec),
            dimensional: None,
            draft: None,
            feedback: None,
        }
    }

    fn sample_review() -> agent_core::ReviewRecord {
        agent_core::ReviewRecord {
            repo_hash: "deadbeef".into(),
            base_rev: "aaa".into(),
            head_rev: "bbb".into(),
            mode_via: "explicit".into(),
            project: "rust".into(),
            is_fork: true,
            changed_files: 4,
            findings: 3,
            findings_in_diff: 2,
            summaries: 1,
            total_ms: 120,
            sum_work_ms: 200,
            critical_path: "analyzer".into(),
            collectors: vec![
                agent_core::CollectorStatus {
                    collector: "analyzer".into(),
                    status: agent_core::CollectStatus::Ok,
                    reason: String::new(),
                    duration_ms: 90,
                },
                agent_core::CollectorStatus {
                    collector: "summaries".into(),
                    status: agent_core::CollectStatus::Skipped,
                    reason: "no pool".into(),
                    duration_ms: 1,
                },
            ],
        }
    }

    #[test]
    fn positive_review_row_maps_headline_and_bool() {
        let row = ReviewRow::from_event(&review_event(sample_review())).expect("review present");
        assert_eq!(row.session_id, "s");
        assert_eq!(row.repo_hash, "deadbeef");
        assert_eq!(row.is_fork, 1); // bool true ⇒ UInt8 1
        assert_eq!(row.changed_files, 4);
        assert_eq!(row.findings_in_diff, 2);
        assert_eq!(row.critical_path, "analyzer");
    }

    #[test]
    fn positive_review_collector_rows_one_per_collector_with_items() {
        let rows = ReviewCollectorRow::rows_from_event(&review_event(sample_review()));
        assert_eq!(rows.len(), 2);
        let analyzer = rows.iter().find(|r| r.collector == "analyzer").unwrap();
        assert_eq!(analyzer.status, "ok");
        assert_eq!(analyzer.items, 3); // well-known collector ⇒ its aggregate count
        let summaries = rows.iter().find(|r| r.collector == "summaries").unwrap();
        assert_eq!(summaries.status, "skipped");
        assert_eq!(summaries.items, 1);
    }

    #[test]
    fn negative_review_rows_absent_without_record() {
        let e = ev("tool", Message::assistant(""), None);
        assert!(ReviewRow::from_event(&e).is_none());
        assert!(ReviewCollectorRow::rows_from_event(&e).is_empty());
    }

    // --- R2: every row carries the verified `user` from the source event -------
    #[test]
    fn positive_all_rows_carry_user_from_event() {
        // The `ev` / `verification` / `review_event` helpers all seed `user = "u"`.
        let usage = Some(Usage {
            prompt_tokens: 1,
            completion_tokens: 1,
            total_tokens: 2,
            ..Default::default()
        });
        assert_eq!(
            EventRow::from_event(&ev("goal", Message::user("h"), None), 0).user,
            "u"
        );
        assert_eq!(
            UsageRow::from_event(&ev("usage", Message::assistant(""), usage))
                .unwrap()
                .user,
            "u"
        );
        assert_eq!(
            VerificationRow::from_event(&verification(sample_record()))
                .unwrap()
                .user,
            "u"
        );
        assert_eq!(
            ReviewRow::from_event(&review_event(sample_review()))
                .unwrap()
                .user,
            "u"
        );
        for r in ReviewCollectorRow::rows_from_event(&review_event(sample_review())) {
            assert_eq!(r.user, "u");
        }

        // DimensionRow comes from a `kind = "dimension"` event; build one inline.
        let dim_event = MemoryEvent {
            kind: "dimension".into(),
            message: Message::assistant(""),
            ts_ms: 1,
            session_id: "s".into(),
            user: "u".into(),
            usage: None,
            iter: None,
            verification: None,
            review: None,
            dimensional: Some(agent_core::DimensionalRecord {
                summaries: vec![agent_core::DimensionSummary {
                    dimension: "arch".into(),
                    summary: "s".into(),
                    is_new: false,
                }],
            }),
            draft: None,
            feedback: None,
        };
        let dim_rows = DimensionRow::rows_from_event(&dim_event);
        assert_eq!(dim_rows.len(), 1);
        assert_eq!(dim_rows[0].user, "u");
    }

    // --- ReviewDraftRow (C14) --------------------------------------------
    fn draft_event(rec: agent_core::ReviewDraftRecord) -> MemoryEvent {
        MemoryEvent {
            kind: "draft".into(),
            message: Message::assistant(""),
            ts_ms: 1,
            session_id: "s".into(),
            user: "u".into(),
            usage: None,
            iter: None,
            verification: None,
            review: None,
            dimensional: None,
            draft: Some(rec),
            feedback: None,
        }
    }

    #[test]
    fn positive_draft_row_from_event() {
        // desc: a `kind = "draft"` event → one ReviewDraftRow with its columns mapped.
        // expect: identity + fleet fields + gate_failed stored as UInt8.
        let rec = agent_core::ReviewDraftRecord {
            review_id: "rid".into(),
            repo: "acme__web".into(),
            pr_number: 42,
            head_sha: "deadbeef".into(),
            risk_score: 0.9,
            gate_failed: true,
            n_findings: 3,
            files_changed: 2,
            additions: 10,
            deletions: 4,
            draft_path: "/w/reviews/pr-42-rrid.md".into(),
            status: "drafted".into(),
        };
        let row = ReviewDraftRow::from_event(&draft_event(rec)).expect("draft row");
        assert_eq!(row.user, "u");
        assert_eq!(row.review_id, "rid");
        assert_eq!(row.repo, "acme__web");
        assert_eq!(row.pr_number, 42);
        assert_eq!(row.head_sha, "deadbeef");
        assert_eq!(row.gate_failed, 1, "bool stored as UInt8");
        assert_eq!(row.status, "drafted");
    }

    #[test]
    fn negative_non_draft_event_yields_no_draft_row() {
        // desc: an event of another kind has no `draft` side-channel. expect: None.
        assert!(ReviewDraftRow::from_event(&ev("goal", Message::user("x"), None)).is_none());
    }

    #[test]
    fn adversarial_hostile_counts_clamped_in_record() {
        // desc: a fan-out reporting more files than u32 can't overflow the row. expect:
        // ReviewDraftRecord::from_facts saturates additions/deletions at u32::MAX.
        let mut f = agent_core::ReviewFacts::default();
        for _ in 0..2 {
            f.change.files.push(agent_core::ChangedFile {
                path: "x".into(),
                change: agent_core::ChangeKind::Modified,
                additions: u32::MAX,
                deletions: u32::MAX,
                is_binary: false,
                lang: "rust".into(),
                patch: String::new(),
            });
        }
        let rec = agent_core::ReviewDraftRecord::from_facts(
            "rid",
            "acme__web",
            1,
            &f,
            "/p.md",
            "drafted",
        );
        assert_eq!(rec.additions, u32::MAX, "additions saturate, never wrap");
        assert_eq!(rec.deletions, u32::MAX, "deletions saturate, never wrap");
    }

    // --- ReviewFeedbackRow (C15) -----------------------------------------
    fn feedback_event(round: agent_core::FeedbackRound) -> MemoryEvent {
        MemoryEvent {
            kind: "feedback".into(),
            message: Message::assistant(""),
            ts_ms: 7,
            session_id: "s".into(),
            user: "u".into(),
            usage: None,
            iter: None,
            verification: None,
            review: None,
            dimensional: None,
            draft: None,
            feedback: Some(round),
        }
    }

    fn fb_item(item_id: &str, status: &str) -> agent_core::Feedback {
        agent_core::Feedback {
            item_id: item_id.into(),
            category: "analyzer".into(),
            severity: "warning".into(),
            title: "errcheck: main.go".into(),
            body: "unchecked error".into(),
            status: status.into(),
            first_seen_review: "r0".into(),
            first_seen_sha: "sha0".into(),
            addressed_review: String::new(),
            addressed_sha: String::new(),
        }
    }

    #[test]
    fn positive_feedback_rows_one_per_item_carry_round_context() {
        // desc: a `kind = "feedback"` round with two items → two rows, each carrying the
        // round's review_id/repo/pr and the verified user. expect: 2 rows mapped 1:1.
        let round = agent_core::FeedbackRound {
            review_id: "r1".into(),
            repo: "acme__web".into(),
            pr_number: 42,
            items: vec![fb_item("id-a", "open"), fb_item("id-b", "addressed")],
        };
        let rows = ReviewFeedbackRow::rows_from_event(&feedback_event(round));
        assert_eq!(rows.len(), 2, "one row per item");
        for r in &rows {
            assert_eq!(r.user, "u", "verified user stamped");
            assert_eq!(r.review_id, "r1");
            assert_eq!(r.repo, "acme__web");
            assert_eq!(r.pr_number, 42);
        }
        assert_eq!(rows[0].item_id, "id-a");
        assert_eq!(rows[0].status, "open");
        assert_eq!(rows[1].status, "addressed");
    }

    #[test]
    fn negative_non_feedback_event_yields_no_rows() {
        // desc: an event of another kind has no `feedback` side-channel. expect: empty.
        assert!(
            ReviewFeedbackRow::rows_from_event(&ev("goal", Message::user("x"), None)).is_empty()
        );
    }
}
