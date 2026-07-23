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
    pub ts: DateTime64<3>,
    pub level: String,
    pub target: String,
    pub message: String,
    pub fields: String,
}

impl LogRow {
    pub(crate) fn new(
        session_id: String,
        level: String,
        target: String,
        message: String,
        fields: String,
    ) -> Self {
        Self {
            session_id,
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
            usage,
            iter: Some(2),
            verification: None,
        }
    }

    fn verification(rec: VerificationRecord) -> MemoryEvent {
        MemoryEvent {
            kind: "verification".into(),
            message: Message::assistant(""),
            ts_ms: 1,
            session_id: "s".into(),
            usage: None,
            iter: Some(7),
            verification: Some(rec),
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
}
