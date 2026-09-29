//! `agent-telemetry` — streams the agent's transaction history, logs, and token
//! usage into ClickHouse.
//!
//! It plugs into two existing seams without changing the loop:
//!   * [`CompositeMemory`] wraps any `MemoryStore` and mirrors every appended
//!     `MemoryEvent` into ClickHouse (`agent_events` / `agent_usage`).
//!   * [`ClickHouseLayer`] is a `tracing` layer that streams log events
//!     (`agent_logs`).
//!
//! Both feed a single background writer over a bounded channel, so ClickHouse
//! latency or outages never block or fail the agent — rows are simply dropped
//! (with a one-time warning) while the JSONL episodic log keeps the full record.

mod ch;
mod history;
mod layer;
mod memory;
mod otel;
mod recall;
mod rows;
mod writer;

use agent_core::campaign::{EventSink, TaskEvent, TaskId};
use agent_core::MemoryEvent;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

pub use history::ClickHouseHistory;
pub use layer::ClickHouseLayer;
pub use memory::CompositeMemory;
pub use otel::{otlp_layer, OtelConfig, OtelGuard};
pub use recall::ClickHouseRecall;

use rows::{
    AuthEventRow, DimensionRow, EventRow, ReviewCollectorRow, ReviewDraftRow, ReviewFeedbackRow,
    ReviewRow, ReviewToolRow, UsageRow, VerificationRow,
};
use writer::{Msg, WriterConfig, TARGET};

/// The W3C trace id (32 lowercase hex) of the current span, or `""` when the
/// span carries no valid OpenTelemetry context.
fn current_trace_id() -> String {
    use opentelemetry::trace::TraceContextExt;
    use tracing_opentelemetry::OpenTelemetrySpanExt;
    let context = tracing::Span::current().context();
    let span = context.span();
    let sc = span.span_context();
    if sc.is_valid() {
        sc.trace_id().to_string()
    } else {
        String::new()
    }
}

/// Bounded channel size. Overflow drops rows rather than blocking the loop.
const CHANNEL_CAPACITY: usize = 16_384;

/// Connection + batching settings for the ClickHouse writer.
#[derive(Debug, Clone)]
pub struct TelemetryConfig {
    /// Native-protocol `host:port` (e.g. `localhost:9000`).
    pub addr: String,
    pub database: String,
    pub user: String,
    pub password: String,
    pub batch_max_rows: usize,
    pub flush_interval: Duration,
}

/// A cheap, cloneable handle to the telemetry writer. Cloning shares the same
/// channel, session id, and event sequence counter.
#[derive(Clone)]
pub struct TelemetryHandle {
    tx: mpsc::Sender<Msg>,
    session_id: Arc<str>,
    seq: Arc<AtomicU32>,
    warned: Arc<AtomicBool>,
}

impl TelemetryHandle {
    /// Spawn the background writer and return a handle to it.
    pub fn spawn(cfg: TelemetryConfig, session_id: impl Into<String>) -> Self {
        let (tx, rx) = mpsc::channel(CHANNEL_CAPACITY);
        let writer_cfg = WriterConfig {
            addr: cfg.addr,
            database: cfg.database,
            user: cfg.user,
            password: cfg.password,
            batch_max_rows: cfg.batch_max_rows,
            flush_interval: cfg.flush_interval,
        };
        tokio::spawn(writer::run(rx, writer_cfg));
        Self {
            tx,
            session_id: Arc::from(session_id.into()),
            seq: Arc::new(AtomicU32::new(0)),
            warned: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// A handle whose writer is **not** spawned: rows land in the returned receiver
    /// instead of ClickHouse, so a test can drive the layer/recorders and assert the
    /// exact `Msg`/`LogRow` produced without a live database.
    #[cfg(test)]
    pub(crate) fn for_test(session_id: impl Into<String>) -> (Self, mpsc::Receiver<Msg>) {
        let (tx, rx) = mpsc::channel(CHANNEL_CAPACITY);
        (
            Self {
                tx,
                session_id: Arc::from(session_id.into()),
                seq: Arc::new(AtomicU32::new(0)),
                warned: Arc::new(AtomicBool::new(false)),
            },
            rx,
        )
    }

    /// Mirror a recorded event into ClickHouse. `kind = "usage"` rows route to
    /// `agent_usage`, `kind = "verification"` to `agent_verifications`; everything
    /// else becomes an `agent_events` row.
    pub fn record_event(&self, event: &MemoryEvent) {
        if event.kind == "usage" {
            if let Some(row) = UsageRow::from_event(event) {
                self.send(Msg::Usage(row));
            }
        } else if event.kind == "verification" {
            if let Some(row) = VerificationRow::from_event(event) {
                self.send(Msg::Verification(row));
            }
        } else if event.kind == "review" {
            if let Some(row) = ReviewRow::from_event(event) {
                self.send(Msg::Review(row));
            }
            // One drill-down row per collector (the parallelism detail).
            for row in ReviewCollectorRow::rows_from_event(event) {
                self.send(Msg::ReviewCollector(row));
            }
            // One drill-down row per analyzer tool (review-analysis-depth Inc 2-tel).
            for row in ReviewToolRow::rows_from_event(event) {
                self.send(Msg::ReviewTool(row));
            }
        } else if event.kind == "draft" {
            // The fleet's operational review-draft record (review-fleet C14).
            if let Some(row) = ReviewDraftRow::from_event(event) {
                self.send(Msg::ReviewDraft(row));
            }
        } else if event.kind == "feedback" {
            // One row per feedback item, carried across rounds (review-fleet C15/C16).
            for row in ReviewFeedbackRow::rows_from_event(event) {
                self.send(Msg::ReviewFeedback(row));
            }
        } else if event.kind == "dimension" {
            // One row per accepted per-dimension summary (adaptive-cognition 03).
            for row in DimensionRow::rows_from_event(event) {
                self.send(Msg::Dimension(row));
            }
        } else {
            let seq = self.seq.fetch_add(1, Ordering::Relaxed);
            self.send(Msg::Event(EventRow::from_event(event, seq)));
        }
    }

    /// Record one authentication/authorization event (`agent_auth_events`,
    /// security-hardening S11). Stamped with the current time, the handle's
    /// sequence (ties within a millisecond) and the trace id of the span it was
    /// recorded in, so an audit row joins the request's OTLP trace.
    pub fn record_auth_event(&self, event: agent_core::AuthEvent) {
        let seq = self.seq.fetch_add(1, Ordering::Relaxed);
        self.send(Msg::AuthEvent(AuthEventRow::from_event(
            event,
            rows::now_ms(),
            seq,
            current_trace_id(),
        )));
    }

    /// Mirror one committed campaign `task_events` row into `agent_events` as a
    /// `kind = "campaign"` row (docs/design/campaigns/04-executor.md
    /// §Observability, CP-08) — see [`rows::EventRow::from_campaign`] for the
    /// shape. Non-blocking like every recorder: a full channel drops the row.
    pub fn record_campaign_event(&self, tenant: &str, campaign: TaskId, event: &TaskEvent) {
        let seq = self.seq.fetch_add(1, Ordering::Relaxed);
        self.send(Msg::Event(EventRow::from_campaign(
            tenant, campaign, event, seq,
        )));
    }

    pub(crate) fn record_log(&self, row: rows::LogRow) {
        self.send(Msg::Log(row));
    }

    /// Flush and stop the writer, awaiting the final flush. Best-effort.
    pub async fn shutdown(&self) {
        let (ack_tx, ack_rx) = oneshot::channel();
        if self.tx.send(Msg::Shutdown(ack_tx)).await.is_ok() {
            let _ = ack_rx.await;
        }
    }

    /// Non-blocking send. Drops on a full/closed channel; warns once on overflow.
    fn send(&self, msg: Msg) {
        match self.tx.try_send(msg) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                if !self.warned.swap(true, Ordering::Relaxed) {
                    tracing::warn!(
                        target: TARGET,
                        "telemetry channel full; dropping rows (further drops silent)"
                    );
                }
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {}
        }
    }
}

/// The campaign stores' after-commit mirror (`with_sink`): every committed
/// `task_events` row becomes one `agent_events` row, written by whichever process
/// performed the write (the driver, a CLI verb, an `agent --run-task` child).
impl EventSink for TelemetryHandle {
    fn emit(&self, tenant: &str, campaign: TaskId, event: &TaskEvent) {
        self.record_campaign_event(tenant, campaign, event);
    }
}

#[cfg(test)]
mod auth_event_tests {
    use super::*;
    use agent_core::{AuthEvent, AuthEventKind};
    use opentelemetry::trace::TracerProvider as _;
    use rstest::rstest;
    use tracing_subscriber::layer::SubscriberExt;

    fn take(rx: &mut mpsc::Receiver<Msg>) -> rows::AuthEventRow {
        match rx.try_recv() {
            Ok(Msg::AuthEvent(row)) => row,
            Ok(_) => panic!("expected an auth-event row, got another message"),
            Err(e) => panic!("expected an auth-event row: {e}"),
        }
    }

    #[rstest]
    #[case::positive_one_row_per_event(AuthEventKind::Login, "login")]
    #[case::positive_denial_row(AuthEventKind::AuthzDeny, "authz_deny")]
    #[case::corner_role_change_row(AuthEventKind::RoleDelete, "role_delete")]
    fn record_auth_event_routes_to_its_table(#[case] kind: AuthEventKind, #[case] want: &str) {
        let (handle, mut rx) = TelemetryHandle::for_test("s");
        handle.record_auth_event(AuthEvent::new(kind));
        let row = take(&mut rx);
        assert_eq!(row.event, want);
        assert!(row.trace_id.is_empty(), "no span, no trace id");
        assert!(rx.try_recv().is_err(), "exactly one row");
    }

    /// Two events in the same millisecond still order: `seq` advances per row.
    #[rstest]
    #[case::boundary_seq_advances(3)]
    fn record_auth_event_sequences(#[case] n: u32) {
        let (handle, mut rx) = TelemetryHandle::for_test("s");
        for _ in 0..n {
            handle.record_auth_event(AuthEvent::new(AuthEventKind::Refresh));
        }
        let seqs: Vec<u32> = (0..n).map(|_| take(&mut rx).seq).collect();
        assert_eq!(seqs, (0..n).collect::<Vec<_>>());
    }

    /// Inside an OpenTelemetry span the row carries that trace's id, so it joins
    /// `otel_traces`.
    #[rstest]
    #[case::positive_trace_id_from_current_span()]
    fn record_auth_event_carries_the_trace_id() {
        let provider = opentelemetry_sdk::trace::TracerProvider::builder().build();
        let subscriber = tracing_subscriber::registry()
            .with(tracing_opentelemetry::layer().with_tracer(provider.tracer("t")));
        let (handle, mut rx) = TelemetryHandle::for_test("s");
        tracing::subscriber::with_default(subscriber, || {
            let _g = tracing::info_span!("rpc").entered();
            handle.record_auth_event(AuthEvent::new(AuthEventKind::AuthzAllow));
        });
        let row = take(&mut rx);
        assert_eq!(row.trace_id.len(), 32);
        assert!(row.trace_id.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(row.trace_id, "0".repeat(32));
    }

    /// A full channel drops the row rather than blocking the request path.
    #[rstest]
    #[case::adversarial_flood_never_blocks(CHANNEL_CAPACITY + 10)]
    fn record_auth_event_drops_on_overflow(#[case] n: usize) {
        let (handle, rx) = TelemetryHandle::for_test("s");
        for _ in 0..n {
            handle.record_auth_event(AuthEvent::new(AuthEventKind::VerifyFail));
        }
        assert_eq!(rx.len(), CHANNEL_CAPACITY);
        assert!(handle.warned.load(Ordering::Relaxed));
    }
}

/// T17, the ClickHouse half (`docs/design/campaigns/06-test-matrix.md`): the
/// `kind = "campaign"` row a committed `task_events` row becomes.
#[cfg(test)]
mod campaign_event_tests {
    use super::*;
    use agent_core::campaign::{EventId, TaskState};
    use rows::{CAMPAIGN_CONTENT_MAX, CAMPAIGN_DETAIL_MAX};
    use rstest::rstest;

    fn take(rx: &mut mpsc::Receiver<Msg>) -> EventRow {
        match rx.try_recv() {
            Ok(Msg::Event(row)) => row,
            Ok(_) => panic!("expected an event row, got another message"),
            Err(e) => panic!("expected an event row: {e}"),
        }
    }

    fn event(actor: &str, detail: serde_json::Value) -> TaskEvent {
        TaskEvent {
            event_id: EventId(42),
            task_id: TaskId(11),
            from_state: Some(TaskState::Ready),
            to_state: TaskState::Decomposing,
            actor: actor.to_string(),
            version: 3,
            detail,
            at_ms: 1_700_000_000_123,
        }
    }

    fn content(row: &EventRow) -> serde_json::Value {
        serde_json::from_str(&row.content).expect("content is JSON")
    }

    // desc: the row shape — kind, session grouping, tenant, class role, task id, ts, body.
    #[rstest]
    #[case::positive_clickhouse_row_shape()]
    fn positive_clickhouse_row_shape() {
        let (handle, mut rx) = TelemetryHandle::for_test("s");
        handle.emit(
            "acme",
            TaskId(7),
            &event("planner", serde_json::json!({"k": "v"})),
        );
        let row = take(&mut rx);
        assert_eq!(row.kind, "campaign");
        assert_eq!(row.session_id, "campaign-7");
        assert_eq!(row.user, "acme");
        assert_eq!(row.role, "planner");
        assert_eq!(row.tool_call_id, "11");
        assert!(row.tool_calls.is_empty());
        assert_eq!(row.ts.1, 1_700_000_000_123);
        let body = content(&row);
        assert_eq!(body["task_id"], 11);
        assert_eq!(body["event_id"], 42);
        assert_eq!(body["from"], "ready");
        assert_eq!(body["to"], "decomposing");
        assert_eq!(body["version"], 3);
        assert_eq!(body["actor"], "planner");
        assert_eq!(body["detail"]["k"], "v");
        assert!(rx.try_recv().is_err(), "exactly one row");
    }

    // desc (adversarial): the lease token after `worker:` / `driver:` reaches no column.
    #[rstest]
    #[case::adversarial_worker_token("worker")]
    #[case::adversarial_driver_token("driver")]
    fn adversarial_clickhouse_actor_token_redacted(#[case] class: &str) {
        let token = "0123456789abcdef0123456789abcdef";
        let (handle, mut rx) = TelemetryHandle::for_test("s");
        handle.emit(
            "acme",
            TaskId(7),
            &event(&format!("{class}:{token}"), serde_json::json!({})),
        );
        let row = take(&mut rx);
        assert_eq!(row.role, class);
        for col in [
            &row.session_id,
            &row.user,
            &row.role,
            &row.content,
            &row.tool_calls,
            &row.tool_call_id,
        ] {
            assert!(!col.contains(token), "token leaked into {col:?}");
        }
    }

    // desc (adversarial): a secret in `detail` (a model-authored error) is redacted.
    #[rstest]
    #[case::adversarial_aws_key_in_error("AKIAIOSFODNN7EXAMPLE")]
    fn adversarial_clickhouse_detail_secret_redacted(#[case] secret: &str) {
        let (handle, mut rx) = TelemetryHandle::for_test("s");
        handle.emit(
            "acme",
            TaskId(7),
            &event(
                "model:9",
                serde_json::json!({"error": format!("push failed: key = {secret} done")}),
            ),
        );
        let row = take(&mut rx);
        assert!(!row.content.contains(secret), "{}", row.content);
        assert!(row.content.contains("push failed"));
    }

    // desc (adversarial): a huge `detail` is bounded and the body stays valid JSON.
    #[rstest]
    #[case::adversarial_huge_detail(100 * 1024)]
    fn adversarial_clickhouse_content_capped(#[case] n: usize) {
        let (handle, mut rx) = TelemetryHandle::for_test("s");
        handle.emit(
            "acme",
            TaskId(7),
            &event("poller", serde_json::json!({"poll_error": "x".repeat(n)})),
        );
        let row = take(&mut rx);
        assert!(row.content.chars().count() <= CAMPAIGN_CONTENT_MAX);
        let body = content(&row);
        assert_eq!(body["detail"]["truncated"], true);
        let head = body["detail"]["head"].as_str().unwrap();
        assert!(head.chars().count() <= CAMPAIGN_DETAIL_MAX);
        assert_eq!(body["to"], "decomposing", "the fixed fields survive");
    }

    // desc (corner): an actor that is not a rendered class becomes `other`, never itself.
    #[rstest]
    #[case::corner_unknown_actor_other("root")]
    #[case::corner_empty_actor_other("")]
    fn corner_clickhouse_unknown_actor_other(#[case] actor: &str) {
        let (handle, mut rx) = TelemetryHandle::for_test("s");
        handle.emit("acme", TaskId(7), &event(actor, serde_json::json!({})));
        let row = take(&mut rx);
        assert_eq!(row.role, "other");
        assert_eq!(content(&row)["actor"], "other");
    }

    // desc (boundary): rows in one millisecond still order — `seq` advances per row,
    // shared with the handle's other recorders.
    #[rstest]
    #[case::boundary_clickhouse_seq_advances(3)]
    fn boundary_clickhouse_seq_advances(#[case] n: u32) {
        let (handle, mut rx) = TelemetryHandle::for_test("s");
        for _ in 0..n {
            handle.emit("acme", TaskId(7), &event("rollup", serde_json::json!({})));
        }
        let seqs: Vec<u32> = (0..n).map(|_| take(&mut rx).seq).collect();
        assert_eq!(seqs, (0..n).collect::<Vec<_>>());
    }

    // desc (negative): a full channel drops rather than blocks the store's commit path.
    #[rstest]
    #[case::negative_flood_never_blocks(CHANNEL_CAPACITY + 10)]
    fn negative_clickhouse_drops_on_overflow(#[case] n: usize) {
        let (handle, rx) = TelemetryHandle::for_test("s");
        for _ in 0..n {
            handle.emit("acme", TaskId(7), &event("reaper", serde_json::json!({})));
        }
        assert_eq!(rx.len(), CHANNEL_CAPACITY);
        assert!(handle.warned.load(Ordering::Relaxed));
    }
}
