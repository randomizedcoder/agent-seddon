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

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{Message, ToolCall, Usage};
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
}
