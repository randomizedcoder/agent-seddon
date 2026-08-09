//! The ClickHouse digest ledger — the default deployment target
//! (docs/design/cognition-graph/02-background-distiller.md). Append-only
//! `agent.agent_turn_digests` (`MergeTree`, `ORDER BY (session_id, seq, kind)` —
//! the session read is a sorting-key range scan). A re-distillation is a
//! **versioned insert**; reads keep the newest `ts` per `(session_id, seq, kind)`.
//!
//! **Durable, not fire-and-forget** (unlike the telemetry writer): the connection
//! sets `async_insert = 1` + `wait_for_async_insert = 1` — the server batches
//! (no tiny-parts explosion) but the caller still gets a durability ack. On an
//! error the cached client is dropped and the operation retried once on a fresh
//! connection; bounded backoff-retry beyond that belongs to the caller (the
//! distiller worker wraps `put` in `agent-retry`).
//!
//! **No untrusted string ever reaches the SQL text**: `session_id` is
//! `safe_segment`-validated (`[A-Za-z0-9._-]{1,128}`), `kind` is a closed enum,
//! everything else is numeric; keyword prefiltering runs client-side (pushdown
//! via `hasAny` is a scale deferral — STATUS.md).

use crate::{keyword_match, sanitize, sanitize_query};
use agent_core::{Digest, DigestKind, DigestQuery, DigestStore, Error, Result};
use async_trait::async_trait;
use klickhouse::{Client, ClientOptions, DateTime64, Row, Tz};
use tokio::sync::Mutex;

/// One native-protocol row of `agent.agent_turn_digests` (column-for-column with
/// `nix/clickhouse/schema.sql`; `klickhouse::Row` maps fields by name).
#[derive(Debug, Clone, Row)]
struct DigestRow {
    session_id: String,
    user_id: String,
    seq: u64,
    kind: String,
    text: String,
    keywords: Vec<String>,
    mode: String,
    model: String,
    ts: DateTime64<3>,
    duration_ms: u32,
    tokens: u32,
}

impl DigestRow {
    fn from_digest(d: &Digest) -> Self {
        Self {
            session_id: d.session_id.clone(),
            user_id: d.user_id.clone(),
            seq: d.seq,
            kind: d.kind.as_str().to_string(),
            text: d.text.clone(),
            keywords: d.keywords.clone(),
            mode: d.mode.clone(),
            model: d.model.clone(),
            ts: DateTime64::<3>(Tz::UTC, d.ts_ms),
            duration_ms: d.duration_ms,
            tokens: d.tokens,
        }
    }

    /// Decode, treating the store as untrusted: unknown kind ⇒ `None`.
    fn into_digest(self) -> Option<Digest> {
        Some(Digest {
            kind: DigestKind::parse(&self.kind)?,
            session_id: self.session_id,
            user_id: self.user_id,
            seq: self.seq,
            text: self.text,
            keywords: self.keywords,
            mode: self.mode,
            model: self.model,
            ts_ms: self.ts.1,
            duration_ms: self.duration_ms,
            tokens: self.tokens,
        })
    }
}

pub struct ClickHouseDigests {
    /// `host:port` for the native protocol (e.g. `localhost:9000`).
    addr: String,
    database: String,
    user: String,
    password: String,
    /// Lazily-connected, dropped on error so the next op reconnects.
    client: Mutex<Option<Client>>,
}

const TABLE: &str = "agent_turn_digests";

impl ClickHouseDigests {
    pub fn new(
        addr: impl Into<String>,
        database: impl Into<String>,
        user: impl Into<String>,
        password: impl Into<String>,
    ) -> Self {
        Self {
            addr: addr.into(),
            database: database.into(),
            user: user.into(),
            password: password.into(),
            client: Mutex::new(None),
        }
    }

    async fn connect(&self) -> Result<Client> {
        let client = Client::connect(
            self.addr.as_str(),
            ClientOptions {
                username: self.user.clone(),
                password: self.password.clone(),
                default_database: self.database.clone(),
                tcp_nodelay: true,
            },
        )
        .await
        .map_err(ch_err)?;
        // Durable server-side batching: the ack returns once the row is owned by
        // the server, without per-row parts (the ClickHouse tiny-insert trap).
        client
            .execute(
                "SET async_insert = 1, wait_for_async_insert = 1, \
                 log_queries = 0, log_query_threads = 0",
            )
            .await
            .map_err(ch_err)?;
        Ok(client)
    }

    /// Run `op` on the cached client; on error, reconnect once and retry the op
    /// (a dropped pod / restarted container heals on the next call).
    async fn with_client<T, F, Fut>(&self, op: F) -> Result<T>
    where
        F: Fn(Client) -> Fut,
        Fut: std::future::Future<Output = klickhouse::Result<T>>,
    {
        let mut guard = self.client.lock().await;
        if guard.is_none() {
            *guard = Some(self.connect().await?);
        }
        let client = guard.clone().expect("client just ensured");
        match op(client).await {
            Ok(v) => Ok(v),
            Err(first) => {
                *guard = None; // stale connection — rebuild and retry once
                let fresh = self.connect().await.map_err(|e| {
                    Error::Memory(format!("digest clickhouse: {first}; reconnect failed: {e}"))
                })?;
                let v = op(fresh.clone()).await.map_err(ch_err)?;
                *guard = Some(fresh);
                Ok(v)
            }
        }
    }
}

#[async_trait]
impl DigestStore for ClickHouseDigests {
    async fn put(&self, mut digest: Digest) -> Result<()> {
        sanitize(&mut digest)?;
        let row = DigestRow::from_digest(&digest);
        self.with_client(move |client| {
            let rows = vec![row.clone()];
            async move {
                client
                    .insert_native_block(format!("INSERT INTO {TABLE} FORMAT native"), rows)
                    .await
            }
        })
        .await
    }

    async fn query(&self, q: &DigestQuery) -> Result<Vec<Digest>> {
        let (q, limit) = sanitize_query(q)?;
        // Every interpolated value is validated/closed/numeric (module doc).
        let kind_clause = q
            .kind
            .map(|k| format!("AND kind = '{}'", k.as_str()))
            .unwrap_or_default();
        let since = q.since_seq.unwrap_or(0);
        let sql = format!(
            "SELECT session_id, user_id, seq, kind, text, keywords, mode, model, \
                    ts, duration_ms, tokens \
               FROM {TABLE} \
              WHERE session_id = '{sid}' AND seq >= {since} {kind_clause} \
              ORDER BY seq ASC, kind ASC, ts DESC \
              LIMIT {fetch}",
            sid = q.session_id,
            // Versioned inserts mean up to a few rows per key; fetch headroom so
            // the newest-wins dedupe below still fills `limit`.
            fetch = crate::MAX_QUERY_LIMIT * 4,
        );
        let rows: Vec<DigestRow> = self
            .with_client(move |client| {
                let sql = sql.clone();
                async move { client.query_collect::<DigestRow>(sql).await }
            })
            .await?;

        // Newest `ts` wins per (seq, kind) — the ORDER BY put it first; a row that
        // fails to decode (unknown kind) is skipped, not fatal.
        let mut out: Vec<Digest> = Vec::new();
        for row in rows {
            let Some(d) = row.into_digest() else { continue };
            if out
                .iter()
                .any(|prev| prev.seq == d.seq && prev.kind == d.kind)
            {
                continue; // an older version of an already-kept row
            }
            if !keyword_match(&d.keywords, &q.keywords_any) {
                continue;
            }
            out.push(d);
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }
}

fn ch_err(e: klickhouse::KlickhouseError) -> Error {
    Error::Memory(format!("digest clickhouse: {e}"))
}

#[cfg(test)]
mod tests {
    //! Hermetic tests cover construction + the sanitize path (no server in the
    //! sandbox — the sqlite backend carries the behavioral table; the live
    //! ClickHouse path is exercised by the increment's live check, and the
    //! sanitizers are shared `crate::` functions tested in `lib.rs`).
    use super::*;

    #[tokio::test]
    async fn adversarial_hostile_ids_rejected_before_any_connection() {
        // Must fail on sanitize — NOT by attempting a network connect (the addr
        // below would hang/refuse; an error mentioning it would mean we dialed).
        let s = ClickHouseDigests::new("127.0.0.1:1", "agent", "", "");
        let d = Digest {
            session_id: "../../etc".into(),
            user_id: "local".into(),
            seq: 1,
            kind: DigestKind::Summary,
            text: "x".into(),
            keywords: vec![],
            mode: String::new(),
            model: String::new(),
            ts_ms: 1,
            duration_ms: 0,
            tokens: 0,
        };
        let err = s.put(d).await.expect_err("rejected");
        assert!(err.to_string().contains("invalid session_id"));
        let err = s
            .query(&DigestQuery {
                session_id: "a/b".into(),
                ..DigestQuery::default()
            })
            .await
            .expect_err("rejected");
        assert!(err.to_string().contains("invalid session_id"));
    }
}
