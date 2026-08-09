//! The SQLite digest ledger — server-less environments (dev shells, CI, hermetic
//! tests). Own DB file, never the session store (the codex add-then-drop migration
//! lesson). `Connection` is `Send + !Sync`, so it sits behind a `Mutex` and every
//! method does one short synchronous transaction (the `agent-prompt` sqlite
//! pattern); callers are the background distiller and compaction — never the
//! turn's hot path.

use crate::{keyword_match, sanitize, sanitize_query};
use agent_core::{Digest, DigestKind, DigestQuery, DigestStore, Error, Result};
use async_trait::async_trait;
use rusqlite::{params, Connection};
use std::path::Path;
use std::sync::Mutex;

/// Matches docs/design/cognition-graph/02-background-distiller.md — the
/// `(session_id, seq, kind)` PK is the replace key; the `(session_id, kind, seq)`
/// index is the "all summaries for this session, in order" read (opencode's
/// `(session, type, seq)` shape).
const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS digests (
  session_id  TEXT    NOT NULL,
  user_id     TEXT    NOT NULL DEFAULT 'local',
  seq         INTEGER NOT NULL,
  kind        TEXT    NOT NULL CHECK (kind IN ('summary','facts','objective','alternatives')),
  text        TEXT    NOT NULL,
  keywords    TEXT    NOT NULL DEFAULT '[]',
  mode        TEXT    NOT NULL DEFAULT '',
  model       TEXT    NOT NULL DEFAULT '',
  ts_ms       INTEGER NOT NULL,
  duration_ms INTEGER NOT NULL DEFAULT 0,
  tokens      INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (session_id, seq, kind)
);
CREATE INDEX IF NOT EXISTS idx_digests_session_kind_seq
  ON digests(session_id, kind, seq);
";

pub struct SqliteDigests {
    conn: Mutex<Connection>,
}

impl SqliteDigests {
    /// Open (creating file + schema as needed).
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path).map_err(sql_err)?;
        Self::from_conn(conn)
    }

    /// An in-memory ledger (tests).
    pub fn in_memory() -> Result<Self> {
        Self::from_conn(Connection::open_in_memory().map_err(sql_err)?)
    }

    fn from_conn(conn: Connection) -> Result<Self> {
        conn.execute_batch(SCHEMA).map_err(sql_err)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }
}

#[async_trait]
impl DigestStore for SqliteDigests {
    async fn put(&self, mut digest: Digest) -> Result<()> {
        sanitize(&mut digest)?;
        let keywords = serde_json::to_string(&digest.keywords)?;
        let conn = self.conn.lock().expect("digest sqlite lock");
        conn.execute(
            "INSERT OR REPLACE INTO digests
               (session_id, user_id, seq, kind, text, keywords, mode, model, ts_ms, duration_ms, tokens)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                digest.session_id,
                digest.user_id,
                i64::try_from(digest.seq).unwrap_or(i64::MAX),
                digest.kind.as_str(),
                digest.text,
                keywords,
                digest.mode,
                digest.model,
                i64::try_from(digest.ts_ms).unwrap_or(i64::MAX),
                digest.duration_ms,
                digest.tokens,
            ],
        )
        .map_err(sql_err)?;
        Ok(())
    }

    async fn query(&self, q: &DigestQuery) -> Result<Vec<Digest>> {
        let (q, limit) = sanitize_query(q)?;
        let conn = self.conn.lock().expect("digest sqlite lock");
        // kind/since narrow in SQL; the keyword prefilter runs in Rust AFTER the
        // fetch and BEFORE the limit (filtering after a SQL LIMIT would starve
        // matching rows), so the SQL fetch is capped at the server ceiling only.
        let mut stmt = conn
            .prepare(
                "SELECT session_id, user_id, seq, kind, text, keywords, mode, model,
                        ts_ms, duration_ms, tokens
                   FROM digests
                  WHERE session_id = ?1
                    AND (?2 = '' OR kind = ?2)
                    AND seq >= ?3
                  ORDER BY seq ASC
                  LIMIT ?4",
            )
            .map_err(sql_err)?;
        let kind = q.kind.map(DigestKind::as_str).unwrap_or("");
        let since = i64::try_from(q.since_seq.unwrap_or(0)).unwrap_or(i64::MAX);
        let rows = stmt
            .query_map(
                params![q.session_id, kind, since, crate::MAX_QUERY_LIMIT as i64],
                row_to_digest,
            )
            .map_err(sql_err)?;
        let mut out = Vec::new();
        for row in rows {
            // A row that fails to decode (unknown kind, corrupt keywords) is
            // skipped, not fatal — fail closed on the row, soft on the read.
            let Some(d) = row.map_err(sql_err)?.into_digest() else {
                continue;
            };
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

/// One fetched row, pre-decode (kind/keywords still raw strings from the store).
struct RawRow {
    session_id: String,
    user_id: String,
    seq: i64,
    kind: String,
    text: String,
    keywords: String,
    mode: String,
    model: String,
    ts_ms: i64,
    duration_ms: u32,
    tokens: u32,
}

impl RawRow {
    /// Decode, treating the store as untrusted: unknown kind ⇒ `None`, corrupt
    /// keywords ⇒ empty list, negative counters ⇒ 0.
    fn into_digest(self) -> Option<Digest> {
        Some(Digest {
            kind: DigestKind::parse(&self.kind)?,
            keywords: serde_json::from_str(&self.keywords).unwrap_or_default(),
            session_id: self.session_id,
            user_id: self.user_id,
            seq: u64::try_from(self.seq).unwrap_or(0),
            text: self.text,
            mode: self.mode,
            model: self.model,
            ts_ms: u64::try_from(self.ts_ms).unwrap_or(0),
            duration_ms: self.duration_ms,
            tokens: self.tokens,
        })
    }
}

fn row_to_digest(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawRow> {
    Ok(RawRow {
        session_id: row.get(0)?,
        user_id: row.get(1)?,
        seq: row.get(2)?,
        kind: row.get(3)?,
        text: row.get(4)?,
        keywords: row.get(5)?,
        mode: row.get(6)?,
        model: row.get(7)?,
        ts_ms: row.get(8)?,
        duration_ms: row.get(9)?,
        tokens: row.get(10)?,
    })
}

fn sql_err(e: rusqlite::Error) -> Error {
    Error::Memory(format!("digest sqlite: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(session: &str, seq: u64, kind: DigestKind, text: &str, keywords: &[&str]) -> Digest {
        Digest {
            session_id: session.into(),
            user_id: "local".into(),
            seq,
            kind,
            text: text.into(),
            keywords: keywords.iter().map(|s| (*s).to_string()).collect(),
            mode: "implement".into(),
            model: "kimi".into(),
            ts_ms: 1_000 + seq,
            duration_ms: 42,
            tokens: 7,
        }
    }

    fn q(session: &str) -> DigestQuery {
        DigestQuery {
            session_id: session.into(),
            ..DigestQuery::default()
        }
    }

    #[tokio::test]
    async fn positive_put_then_query_ordered_by_seq() {
        let s = SqliteDigests::in_memory().unwrap();
        for seq in [3u64, 1, 2] {
            s.put(d("s1", seq, DigestKind::Summary, &format!("sum{seq}"), &[]))
                .await
                .unwrap();
        }
        let rows = s.query(&q("s1")).await.unwrap();
        assert_eq!(
            rows.iter().map(|r| r.seq).collect::<Vec<_>>(),
            vec![1, 2, 3],
            "seq ascending"
        );
        assert_eq!(rows[0].text, "sum1");
        assert_eq!(rows[0].mode, "implement");
    }

    #[tokio::test]
    async fn positive_kind_and_since_filters() {
        let s = SqliteDigests::in_memory().unwrap();
        s.put(d("s1", 1, DigestKind::Summary, "sum", &[]))
            .await
            .unwrap();
        s.put(d("s1", 1, DigestKind::Facts, "facts", &[]))
            .await
            .unwrap();
        s.put(d("s1", 2, DigestKind::Summary, "sum2", &[]))
            .await
            .unwrap();
        let mut query = q("s1");
        query.kind = Some(DigestKind::Facts);
        assert_eq!(s.query(&query).await.unwrap().len(), 1);
        let mut query = q("s1");
        query.since_seq = Some(2);
        assert_eq!(s.query(&query).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn positive_keyword_prefilter_case_insensitive() {
        let s = SqliteDigests::in_memory().unwrap();
        s.put(d(
            "s1",
            1,
            DigestKind::Summary,
            "a",
            &["Compaction", "SQLite"],
        ))
        .await
        .unwrap();
        s.put(d("s1", 2, DigestKind::Summary, "b", &["routing"]))
            .await
            .unwrap();
        let mut query = q("s1");
        query.keywords_any = vec!["sqlite".into()];
        let rows = s.query(&query).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].seq, 1);
    }

    #[tokio::test]
    async fn corner_replace_same_key_keeps_latest() {
        let s = SqliteDigests::in_memory().unwrap();
        s.put(d("s1", 1, DigestKind::Summary, "v1", &[]))
            .await
            .unwrap();
        s.put(d("s1", 1, DigestKind::Summary, "v2 re-distilled", &[]))
            .await
            .unwrap();
        let rows = s.query(&q("s1")).await.unwrap();
        assert_eq!(rows.len(), 1, "replace, not duplicate");
        assert_eq!(rows[0].text, "v2 re-distilled");
    }

    #[tokio::test]
    async fn corner_sessions_are_isolated() {
        let s = SqliteDigests::in_memory().unwrap();
        s.put(d("s1", 1, DigestKind::Summary, "one", &[]))
            .await
            .unwrap();
        s.put(d("s2", 1, DigestKind::Summary, "two", &[]))
            .await
            .unwrap();
        assert_eq!(s.query(&q("s1")).await.unwrap().len(), 1);
        assert_eq!(s.query(&q("s2")).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn negative_unknown_session_is_empty_not_error() {
        let s = SqliteDigests::in_memory().unwrap();
        assert!(s.query(&q("nope")).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn boundary_limit_caps_rows() {
        let s = SqliteDigests::in_memory().unwrap();
        for seq in 1..=10u64 {
            s.put(d("s1", seq, DigestKind::Summary, "x", &[]))
                .await
                .unwrap();
        }
        let mut query = q("s1");
        query.limit = 3;
        let rows = s.query(&query).await.unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[2].seq, 3, "first three in seq order");
    }

    #[tokio::test]
    async fn adversarial_traversal_ids_rejected_both_paths() {
        let s = SqliteDigests::in_memory().unwrap();
        assert!(s
            .put(d("../../etc", 1, DigestKind::Summary, "x", &[]))
            .await
            .is_err());
        assert!(s.query(&q("../..")).await.is_err());
    }

    #[tokio::test]
    async fn adversarial_corrupt_row_is_skipped_not_fatal() {
        let s = SqliteDigests::in_memory().unwrap();
        s.put(d("s1", 1, DigestKind::Summary, "good", &[]))
            .await
            .unwrap();
        // Bypass the API to plant a row with an unknown kind (a hostile/updated
        // store) and corrupt keywords JSON.
        {
            let conn = s.conn.lock().unwrap();
            conn.execute_batch(
                "PRAGMA ignore_check_constraints = ON;
                 INSERT INTO digests (session_id, user_id, seq, kind, text, keywords, ts_ms)
                 VALUES ('s1', 'local', 2, 'weaponized', 'evil', 'not-json', 2);",
            )
            .unwrap();
        }
        let rows = s.query(&q("s1")).await.unwrap();
        assert_eq!(rows.len(), 1, "unknown-kind row skipped");
        assert_eq!(rows[0].text, "good");
    }

    #[tokio::test]
    async fn adversarial_oversize_text_capped_before_store() {
        let s = SqliteDigests::in_memory().unwrap();
        let mut big = d("s1", 1, DigestKind::Facts, "", &[]);
        big.text = "x".repeat(10 * crate::MAX_TEXT_BYTES);
        s.put(big).await.unwrap();
        let rows = s.query(&q("s1")).await.unwrap();
        assert!(rows[0].text.len() <= crate::MAX_TEXT_BYTES);
    }

    #[tokio::test]
    async fn positive_open_persists_across_reopen() {
        let dir = agent_testkit::tempdir();
        let path = dir.join("digests.sqlite3");
        {
            let s = SqliteDigests::open(&path).unwrap();
            s.put(d("s1", 1, DigestKind::Summary, "persisted", &[]))
                .await
                .unwrap();
        }
        let s = SqliteDigests::open(&path).unwrap();
        assert_eq!(s.query(&q("s1")).await.unwrap()[0].text, "persisted");
    }
}
