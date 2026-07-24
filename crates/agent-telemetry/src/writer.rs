//! The background writer task: connects to ClickHouse over the **native
//! protocol** (`klickhouse`, port 9000) and batches rows per table, flushing on
//! a size threshold, a periodic tick, and at shutdown.
//!
//! Failures never propagate — a warning is logged (on a filtered target so the
//! tracing layer doesn't feed itself) and the batch is dropped. Telemetry is
//! best-effort; the JSONL episodic log remains the source of truth.

use crate::rows::{EventRow, LogRow, ReviewCollectorRow, ReviewRow, UsageRow, VerificationRow};
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
    /// Flush everything and stop; the ack fires once the final flush completes.
    /// Needed because the global tracing subscriber holds a `Sender` clone for
    /// the process lifetime, so channel-close can't be the shutdown signal.
    Shutdown(tokio::sync::oneshot::Sender<()>),
}

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
    let client = match connect(&cfg).await {
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

    let mut events: Vec<EventRow> = Vec::new();
    let mut logs: Vec<LogRow> = Vec::new();
    let mut usage: Vec<UsageRow> = Vec::new();
    let mut verifications: Vec<VerificationRow> = Vec::new();
    let mut reviews: Vec<ReviewRow> = Vec::new();
    let mut review_collectors: Vec<ReviewCollectorRow> = Vec::new();

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
                Some(Msg::Shutdown(ack)) => {
                    flush(&client, "agent_events", &mut events).await;
                    flush(&client, "agent_logs", &mut logs).await;
                    flush(&client, "agent_usage", &mut usage).await;
                    flush(&client, "agent_verifications", &mut verifications).await;
                    flush(&client, "agent_reviews", &mut reviews).await;
                    flush(&client, "agent_review_collectors", &mut review_collectors).await;
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

async fn flush<T>(client: &Client, table: &str, buf: &mut Vec<T>)
where
    T: Row + Send + Sync + 'static,
{
    if buf.is_empty() {
        return;
    }
    let n = buf.len();
    let rows = std::mem::take(buf);
    let query = format!("INSERT INTO {table} FORMAT native");
    match tokio::time::timeout(FLUSH_TIMEOUT, client.insert_native_block(query, rows)).await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            tracing::warn!(target: TARGET, "clickhouse insert into {table} failed ({n} rows dropped): {e}")
        }
        Err(_) => {
            tracing::warn!(target: TARGET, "clickhouse insert into {table} timed out ({n} rows dropped)")
        }
    }
}
