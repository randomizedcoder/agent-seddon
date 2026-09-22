//! The background writer task: connects to ClickHouse over the **native
//! protocol** (`klickhouse`, port 9000) and batches rows per table, flushing on
//! a size threshold, a periodic tick, and at shutdown.
//!
//! Failures never propagate — a warning is logged (on a filtered target so the
//! tracing layer doesn't feed itself) and the batch is dropped. Telemetry is
//! best-effort; the JSONL episodic log remains the source of truth.
//!
//! The native connection can die out from under us — an idle-timeout close, or a
//! ClickHouse restart — and every later insert then fails with "channel closed".
//! A failed/timed-out insert therefore drops the batch **and drops the cached
//! client**, so the next flush reconnects (the reader
//! [`crate::history::ClickHouseHistory`] already self-heals the same way). Without
//! this the writer fails every insert forever after a single transient blip,
//! silently stranding not just best-effort logs but the durable
//! `agent_review_drafts` rows that approve→post later reads.

use crate::rows::{
    DimensionRow, EventRow, LogRow, ReviewCollectorRow, ReviewDraftRow, ReviewFeedbackRow,
    ReviewRow, ReviewToolRow, UsageRow, VerificationRow,
};
use klickhouse::{Client, ClientOptions, Row};
use std::time::Duration;
use tokio::sync::mpsc;

/// Log target for the writer's own diagnostics. The tracing layer filters this
/// prefix to avoid a tracing → insert → tracing feedback loop.
pub(crate) const TARGET: &str = "agent_telemetry";

pub(crate) enum Msg {
    Event(EventRow),
    Log(LogRow),
    Usage(UsageRow),
    Verification(VerificationRow),
    Review(ReviewRow),
    ReviewCollector(ReviewCollectorRow),
    ReviewTool(ReviewToolRow),
    ReviewDraft(ReviewDraftRow),
    ReviewFeedback(ReviewFeedbackRow),
    Dimension(DimensionRow),
    /// Flush everything and stop; the ack fires once the final flush completes.
    /// Needed because the global tracing subscriber holds a `Sender` clone for
    /// the process lifetime, so channel-close can't be the shutdown signal.
    Shutdown(tokio::sync::oneshot::Sender<()>),
}

#[derive(Clone)]
pub(crate) struct WriterConfig {
    /// `host:port` for the native protocol (e.g. `localhost:9000`).
    pub addr: String,
    pub database: String,
    pub user: String,
    pub password: String,
    pub batch_max_rows: usize,
    pub flush_interval: Duration,
}

/// A single flush may not block the task forever if ClickHouse is unreachable.
const FLUSH_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) async fn run(mut rx: mpsc::Receiver<Msg>, cfg: WriterConfig) {
    // Require an initial connection to start; after that a dropped connection is
    // healed lazily by `ReconnectingClient` on the next flush.
    let initial = match connect(&cfg).await {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(
                target: TARGET,
                "clickhouse connect to {} failed ({e}); telemetry disabled for this run",
                cfg.addr
            );
            drain(rx).await;
            return;
        }
    };
    // `run` keeps `cfg` (it reads `batch_max_rows`/`flush_interval` throughout the
    // loop); the reconnecting client only needs the connection fields, so hand it a
    // clone (a single startup allocation).
    let client = ReconnectingClient::new(cfg.clone(), initial);

    let mut events: Vec<EventRow> = Vec::new();
    let mut logs: Vec<LogRow> = Vec::new();
    let mut usage: Vec<UsageRow> = Vec::new();
    let mut verifications: Vec<VerificationRow> = Vec::new();
    let mut reviews: Vec<ReviewRow> = Vec::new();
    let mut review_collectors: Vec<ReviewCollectorRow> = Vec::new();
    let mut review_tools: Vec<ReviewToolRow> = Vec::new();
    let mut review_drafts: Vec<ReviewDraftRow> = Vec::new();
    let mut review_feedback: Vec<ReviewFeedbackRow> = Vec::new();
    let mut dimensions: Vec<DimensionRow> = Vec::new();

    let mut ticker = tokio::time::interval(cfg.flush_interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            maybe = rx.recv() => match maybe {
                Some(Msg::Event(r)) => {
                    events.push(r);
                    if events.len() >= cfg.batch_max_rows {
                        flush(&client, "agent_events", &mut events).await;
                    }
                }
                Some(Msg::Log(r)) => {
                    logs.push(r);
                    if logs.len() >= cfg.batch_max_rows {
                        flush(&client, "agent_logs", &mut logs).await;
                    }
                }
                Some(Msg::Usage(r)) => {
                    usage.push(r);
                    if usage.len() >= cfg.batch_max_rows {
                        flush(&client, "agent_usage", &mut usage).await;
                    }
                }
                Some(Msg::Verification(r)) => {
                    verifications.push(r);
                    if verifications.len() >= cfg.batch_max_rows {
                        flush(&client, "agent_verifications", &mut verifications).await;
                    }
                }
                Some(Msg::Review(r)) => {
                    reviews.push(r);
                    if reviews.len() >= cfg.batch_max_rows {
                        flush(&client, "agent_reviews", &mut reviews).await;
                    }
                }
                Some(Msg::ReviewCollector(r)) => {
                    review_collectors.push(r);
                    if review_collectors.len() >= cfg.batch_max_rows {
                        flush(&client, "agent_review_collectors", &mut review_collectors).await;
                    }
                }
                Some(Msg::ReviewTool(r)) => {
                    review_tools.push(r);
                    if review_tools.len() >= cfg.batch_max_rows {
                        flush(&client, "agent_review_tools", &mut review_tools).await;
                    }
                }
                Some(Msg::ReviewDraft(r)) => {
                    review_drafts.push(r);
                    if review_drafts.len() >= cfg.batch_max_rows {
                        flush(&client, "agent_review_drafts", &mut review_drafts).await;
                    }
                }
                Some(Msg::ReviewFeedback(r)) => {
                    review_feedback.push(r);
                    if review_feedback.len() >= cfg.batch_max_rows {
                        flush(&client, "agent_review_feedback", &mut review_feedback).await;
                    }
                }
                Some(Msg::Dimension(r)) => {
                    dimensions.push(r);
                    if dimensions.len() >= cfg.batch_max_rows {
                        flush(&client, "agent_dimension_summaries", &mut dimensions).await;
                    }
                }
                Some(Msg::Shutdown(ack)) => {
                    flush(&client, "agent_events", &mut events).await;
                    flush(&client, "agent_logs", &mut logs).await;
                    flush(&client, "agent_usage", &mut usage).await;
                    flush(&client, "agent_verifications", &mut verifications).await;
                    flush(&client, "agent_reviews", &mut reviews).await;
                    flush(&client, "agent_review_collectors", &mut review_collectors).await;
                    flush(&client, "agent_review_tools", &mut review_tools).await;
                    flush(&client, "agent_review_drafts", &mut review_drafts).await;
                    flush(&client, "agent_review_feedback", &mut review_feedback).await;
                    flush(&client, "agent_dimension_summaries", &mut dimensions).await;
                    let _ = ack.send(());
                    return;
                }
                // All senders dropped → drain and exit.
                None => break,
            },
            _ = ticker.tick() => {
                flush(&client, "agent_events", &mut events).await;
                flush(&client, "agent_logs", &mut logs).await;
                flush(&client, "agent_usage", &mut usage).await;
                flush(&client, "agent_verifications", &mut verifications).await;
                flush(&client, "agent_reviews", &mut reviews).await;
                flush(&client, "agent_review_collectors", &mut review_collectors).await;
                flush(&client, "agent_review_tools", &mut review_tools).await;
                flush(&client, "agent_review_drafts", &mut review_drafts).await;
                flush(&client, "agent_review_feedback", &mut review_feedback).await;
                flush(&client, "agent_dimension_summaries", &mut dimensions).await;
            }
        }
    }

    // Final flush of whatever remains.
    flush(&client, "agent_events", &mut events).await;
    flush(&client, "agent_logs", &mut logs).await;
    flush(&client, "agent_usage", &mut usage).await;
    flush(&client, "agent_verifications", &mut verifications).await;
    flush(&client, "agent_reviews", &mut reviews).await;
    flush(&client, "agent_review_collectors", &mut review_collectors).await;
    flush(&client, "agent_review_tools", &mut review_tools).await;
    flush(&client, "agent_review_drafts", &mut review_drafts).await;
    flush(&client, "agent_review_feedback", &mut review_feedback).await;
    flush(&client, "agent_dimension_summaries", &mut dimensions).await;
}

async fn connect(cfg: &WriterConfig) -> klickhouse::Result<Client> {
    let client = Client::connect(
        cfg.addr.as_str(),
        ClientOptions {
            username: cfg.user.clone(),
            password: cfg.password.clone(),
            default_database: cfg.database.clone(),
            tcp_nodelay: true,
        },
    )
    .await?;

    // Keep our high-frequency telemetry inserts out of ClickHouse's own
    // system.query_log / system.query_thread_log (they persist for the session).
    if let Err(e) = client
        .execute("SET log_queries = 0, log_query_threads = 0")
        .await
    {
        tracing::warn!(target: TARGET, "could not disable clickhouse query logging: {e}");
    }
    Ok(client)
}

/// Consume and drop messages when we have no connection, still honoring shutdown.
async fn drain(mut rx: mpsc::Receiver<Msg>) {
    while let Some(msg) = rx.recv().await {
        if let Msg::Shutdown(ack) = msg {
            let _ = ack.send(());
            return;
        }
    }
}

/// Owns the native connection and re-establishes it on demand. A dropped
/// connection (idle close / CH restart) surfaces as an insert error; the client
/// is then dropped so the next call reconnects — mirroring the reader's
/// `with_client` discipline in [`crate::history`]. The writer is a single task,
/// so the mutex is uncontended (it only makes the client rebindable across the
/// `.await`).
struct ReconnectingClient {
    cfg: WriterConfig,
    client: tokio::sync::Mutex<Option<Client>>,
}

impl ReconnectingClient {
    fn new(cfg: WriterConfig, initial: Client) -> Self {
        Self {
            cfg,
            client: tokio::sync::Mutex::new(Some(initial)),
        }
    }

    /// Insert one batch, (re)connecting if the cached client is absent or stale.
    /// Best-effort exactly as before: on any failure the rows are dropped and a
    /// warning is logged. The only new behaviour is recovery — the batch that
    /// races a just-died connection is still lost (it was moved into the failed
    /// insert), but the connection is rebuilt so the next flush succeeds instead
    /// of the writer failing forever.
    async fn insert<T>(&self, table: &str, rows: Vec<T>)
    where
        T: Row + Send + Sync + 'static,
    {
        let n = rows.len();
        let mut guard = self.client.lock().await;
        if guard.is_none() {
            match connect(&self.cfg).await {
                Ok(c) => *guard = Some(c),
                Err(e) => {
                    tracing::warn!(target: TARGET, "clickhouse reconnect for {table} failed ({n} rows dropped): {e}");
                    return;
                }
            }
        }
        let client = guard.clone().expect("client just ensured");
        let query = format!("INSERT INTO {table} FORMAT native");
        match tokio::time::timeout(FLUSH_TIMEOUT, client.insert_native_block(query, rows)).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                *guard = None; // stale connection — the next flush reconnects
                tracing::warn!(target: TARGET, "clickhouse insert into {table} failed ({n} rows dropped): {e}");
            }
            Err(_) => {
                *guard = None; // timed out — treat the connection as stale too
                tracing::warn!(target: TARGET, "clickhouse insert into {table} timed out ({n} rows dropped)");
            }
        }
    }
}

async fn flush<T>(client: &ReconnectingClient, table: &str, buf: &mut Vec<T>)
where
    T: Row + Send + Sync + 'static,
{
    if buf.is_empty() {
        return;
    }
    client.insert(table, std::mem::take(buf)).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rows::LogRow;

    /// A client pointed at a refused port with no cached connection: every insert
    /// takes the (re)connect path and must fail soft.
    fn dead_client() -> ReconnectingClient {
        ReconnectingClient {
            cfg: WriterConfig {
                addr: "127.0.0.1:1".into(), // port 1 refuses immediately
                database: "x".into(),
                user: "x".into(),
                password: String::new(),
                batch_max_rows: 1,
                flush_interval: Duration::from_secs(1),
            },
            client: tokio::sync::Mutex::new(None),
        }
    }

    fn a_row() -> LogRow {
        LogRow::new(
            "s".into(),
            "u".into(),
            String::new(),
            String::new(),
            "INFO".into(),
            "t".into(),
            "m".into(),
            String::new(),
        )
    }

    /// A persistently-dead ClickHouse must never panic or hang the writer, the
    /// batch is dropped, and — the R8-1 fix — the cached client stays `None` so the
    /// next flush RE-ATTEMPTS the connection rather than reusing a dead handle
    /// forever.
    #[tokio::test]
    async fn adversarial_dead_server_flush_drops_and_reattempts() {
        let client = dead_client();
        let mut buf = vec![a_row()];
        flush(&client, "agent_logs", &mut buf).await;
        assert!(
            buf.is_empty(),
            "buffer is drained even when the insert fails"
        );
        assert!(
            client.client.lock().await.is_none(),
            "a failed (re)connect leaves no cached client, so the next flush reconnects"
        );
        // A second flush must also fail soft (re-attempt), not reuse a stale handle.
        let mut buf2 = vec![a_row()];
        flush(&client, "agent_logs", &mut buf2).await;
        assert!(buf2.is_empty());
    }

    /// An empty buffer is a no-op — no connection attempt, no allocation churn.
    #[tokio::test]
    async fn positive_empty_buffer_is_a_noop() {
        let client = dead_client();
        let mut buf: Vec<LogRow> = Vec::new();
        flush(&client, "agent_logs", &mut buf).await;
        assert!(buf.is_empty());
        // Still no cached client — flush returned before touching the connection.
        assert!(client.client.lock().await.is_none());
    }
}
