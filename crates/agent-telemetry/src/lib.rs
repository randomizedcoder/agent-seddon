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

mod layer;
mod memory;
mod rows;
mod writer;

use agent_core::MemoryEvent;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

pub use layer::ClickHouseLayer;
pub use memory::CompositeMemory;

use rows::{EventRow, UsageRow};
use writer::{Msg, WriterConfig, TARGET};

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

    /// Mirror a recorded event into ClickHouse. `kind = "usage"` rows route to
    /// `agent_usage`; everything else becomes an `agent_events` row.
    pub fn record_event(&self, event: &MemoryEvent) {
        if event.kind == "usage" {
            if let Some(row) = UsageRow::from_event(event) {
                self.send(Msg::Usage(row));
            }
        } else {
            let seq = self.seq.fetch_add(1, Ordering::Relaxed);
            self.send(Msg::Event(EventRow::from_event(event, seq)));
        }
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
