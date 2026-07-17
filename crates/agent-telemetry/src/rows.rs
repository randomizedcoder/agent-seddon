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
            content: event.message.content.clone(),
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
    use serde_json::json;

    #[test]
    fn event_row_serializes_tool_calls_and_role() {
        let mut msg = Message::assistant("");
        msg.tool_calls = vec![ToolCall {
            id: "call_1".into(),
            name: "bash".into(),
            arguments: json!({ "command": "ls" }),
        }];
        let event = MemoryEvent {
            kind: "assistant".into(),
            message: msg,
            ts_ms: 1000,
            session_id: "sess".into(),
            usage: None,
            iter: None,
        };
        let row = EventRow::from_event(&event, 3);
        assert_eq!(row.seq, 3);
        assert_eq!(row.kind, "assistant");
        assert_eq!(row.role, "assistant");
        assert_eq!(row.session_id, "sess");
        assert!(row.tool_calls.contains("bash"));
    }

    #[test]
    fn event_row_empty_tool_calls_is_blank() {
        let event = MemoryEvent {
            kind: "goal".into(),
            message: Message::user("hello"),
            ts_ms: 1,
            session_id: "s".into(),
            usage: None,
            iter: None,
        };
        let row = EventRow::from_event(&event, 0);
        assert_eq!(row.role, "user");
        assert_eq!(row.content, "hello");
        assert_eq!(row.tool_calls, "");
    }

    #[test]
    fn usage_row_from_usage_event() {
        let event = MemoryEvent {
            kind: "usage".into(),
            message: Message::assistant(""),
            ts_ms: 1,
            session_id: "s".into(),
            usage: Some(Usage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
            }),
            iter: Some(2),
        };
        let row = UsageRow::from_event(&event).expect("usage row");
        assert_eq!(row.prompt_tokens, 10);
        assert_eq!(row.total_tokens, 15);
        assert_eq!(row.iter, 2);
    }

    #[test]
    fn usage_row_none_without_usage() {
        let event = MemoryEvent {
            kind: "usage".into(),
            message: Message::assistant(""),
            ts_ms: 1,
            session_id: "s".into(),
            usage: None,
            iter: None,
        };
        assert!(UsageRow::from_event(&event).is_none());
    }
}
