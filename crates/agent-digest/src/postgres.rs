//! `PgDigests` — the Postgres digest ledger, behind the non-default
//! `digest-postgres` feature. The same per-session ledger contract as the SQLite
//! tier (`src/sqlite.rs`), over a real server via `sqlx` (pure Rust, the in-tree
//! rustls TLS stack — no `libpq`). It is the OLTP alternative for an operator who
//! already runs one Postgres and does not want a ClickHouse dependency; ClickHouse
//! stays the scale/analytics tier.
//!
//! Unlike SQLite this tier is **not hermetic** — it needs a live server — so its
//! tests are `#[ignore]`-gated and run only via `nix run .#integration`, never
//! inside `nix flake check`. The crate still *compiles* under this feature with no
//! database: every query is runtime-checked (`sqlx::query`, not the compile-time
//! `query!` macro), so no `DATABASE_URL` is needed at build time.
//!
//! Security posture mirrors SQLite and the config-store Postgres tier:
//! **ids/tenants reach SQL only as bound parameters** (`$1`…), `text`/`keywords`
//! are `sanitize`-capped in Rust before storage, the query limit is capped
//! server-side (`MAX_QUERY_LIMIT`), the keyword prefilter runs in Rust after the
//! fetch and before the caller's limit, and a row that fails to decode (unknown
//! kind, corrupt keywords) is skipped, not fatal — fail closed on the row, soft on
//! the read.
//!
//! Schema is applied by a small **versioned** runner ([`PgDigests::run_migrations`])
//! over the embedded [`MIGRATIONS`] set, exactly like the config-store tier. We
//! deliberately do **not** use `sqlx::migrate!`: its `macros` feature pulls in
//! every sqlx driver (`sqlx-mysql`, `sqlx-sqlite`) regardless of the one we use,
//! and `sqlx-mysql` drags in `rsa` — a crate with an unfixable timing-sidechannel
//! advisory (RUSTSEC-2023-0071) that `cargo audit` rejects. A hand-rolled runner
//! over the base `sqlx` API keeps the build to the Postgres driver alone while
//! giving the same exactly-once, versioned guarantee.

use crate::{keyword_match, sanitize, sanitize_query};
use agent_core::{Digest, DigestKind, DigestQuery, DigestStore, Error, Result};
use async_trait::async_trait;
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};
use std::collections::HashSet;

/// The embedded migrations, in apply order: `(version, sql)`. A version is applied
/// exactly once and recorded in the `_digest_migrations` ledger, so a later
/// non-idempotent step (an `ALTER`) runs a single time even across restarts.
/// Adding a migration = drop the next-numbered `.sql` in `migrations/` and append
/// its `(n, include_str!(...))` here (the version is the source of truth, not the
/// filename — no runtime path parsing).
const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("../migrations/0001_digests.sql")),
    // 0002 drops `idx_digests_session_kind_seq`: never chosen by the only read
    // (the `($2='' OR kind=$2)` filter isn't sargable under generic plans) and
    // redundant with the PK, so it was pure write-amplification. See the file.
    (
        2,
        include_str!("../migrations/0002_drop_dead_kind_seq_index.sql"),
    ),
];

/// A fixed, arbitrary key for the transaction-scoped advisory lock that serializes
/// concurrent starters through [`PgDigests::run_migrations`] (so two processes
/// booting at once never race the same non-idempotent step). Any stable constant
/// works; this one is `"agdigest"` folded into an `i64`, distinct from the
/// config-store tier's lock key so the two runners never contend.
const MIGRATION_LOCK_KEY: i64 = 0x6167_6469_6765_7374_u64 as i64;

/// A Postgres-backed digest ledger (a connection pool + the shared schema).
pub struct PgDigests {
    pool: PgPool,
}

fn pg_err(e: sqlx::Error) -> Error {
    Error::Memory(format!("digest postgres: {e}"))
}

impl PgDigests {
    /// Connect a pool to `dsn` (max `pool_max` connections, clamped to ≥1) and,
    /// when `migrate_on_start`, apply the embedded migrations idempotently.
    ///
    /// The `dsn` is already the *resolved* connection string (from a `dsn_ref`
    /// `env:`/`file:` reference — never an inline literal); it is not echoed on
    /// error, since it carries a password.
    pub async fn connect(dsn: &str, pool_max: u32, migrate_on_start: bool) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(pool_max.max(1))
            .connect(dsn)
            .await
            .map_err(|e| Error::Memory(format!("digest postgres: connect failed: {e}")))?;
        if migrate_on_start {
            Self::run_migrations(&pool).await?;
        }
        Ok(Self { pool })
    }

    /// Build a lazily-connecting pool to `dsn` (max `pool_max`, clamped to ≥1)
    /// **synchronously**: the DSN is validated now, but connections open on first
    /// use. Schema is **not** applied here (there is no connection yet); a lazy
    /// deployment assumes the schema is present or migrated out of band. The DSN is
    /// never echoed on error (it carries a password).
    pub fn connect_lazy(dsn: &str, pool_max: u32) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(pool_max.max(1))
            // Never echo `e`: a DSN parse error can contain the connection string.
            .connect_lazy(dsn)
            .map_err(|_| Error::Memory("digest postgres: invalid DSN (could not parse)".into()))?;
        Ok(Self { pool })
    }

    /// Build a ledger over an already-established pool (tests/embedding).
    pub fn from_pool(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Apply the embedded [`MIGRATIONS`] set exactly once, in order.
    ///
    /// The whole run is one transaction guarded by a transaction-scoped advisory
    /// lock ([`MIGRATION_LOCK_KEY`]), so concurrent starters are serialized — the
    /// second waits, then sees a fully-applied ledger and no-ops. DDL is
    /// transactional in Postgres, so the `_digest_migrations` bookkeeping and each
    /// step's DDL commit (or roll back) together: a crash mid-run never leaves a
    /// half-applied, half-recorded step. On a DB carrying the schema but no ledger,
    /// the empty history re-runs `0001` (`CREATE TABLE IF NOT EXISTS`, inert) and
    /// records it — so upgrading in place is safe.
    pub async fn run_migrations(pool: &PgPool) -> Result<()> {
        let mut tx = pool.begin().await.map_err(pg_err)?;
        // Serialize concurrent starters; the lock releases on commit/rollback.
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(MIGRATION_LOCK_KEY)
            .execute(&mut *tx)
            .await
            .map_err(pg_err)?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS _digest_migrations (
                 version    BIGINT      NOT NULL PRIMARY KEY,
                 applied_at TIMESTAMPTZ NOT NULL DEFAULT now()
             )",
        )
        .execute(&mut *tx)
        .await
        .map_err(pg_err)?;
        let applied: HashSet<i64> =
            sqlx::query_scalar::<_, i64>("SELECT version FROM _digest_migrations")
                .fetch_all(&mut *tx)
                .await
                .map_err(pg_err)?
                .into_iter()
                .collect();
        for (version, sql) in MIGRATIONS {
            if applied.contains(version) {
                continue;
            }
            // `raw_sql` uses the simple-query protocol, so a script with several
            // statements runs as one call — the migration body applies whole.
            sqlx::raw_sql(sql).execute(&mut *tx).await.map_err(|e| {
                Error::Memory(format!("digest postgres: migration {version} failed: {e}"))
            })?;
            sqlx::query("INSERT INTO _digest_migrations (version) VALUES ($1)")
                .bind(version)
                .execute(&mut *tx)
                .await
                .map_err(pg_err)?;
        }
        tx.commit().await.map_err(pg_err)?;
        Ok(())
    }
}

#[async_trait]
impl DigestStore for PgDigests {
    async fn put(&self, mut digest: Digest) -> Result<()> {
        sanitize(&mut digest)?;
        let keywords = serde_json::to_string(&digest.keywords)?;
        // `INSERT … ON CONFLICT DO UPDATE` on the `(session_id, seq, kind)` PK is
        // the "replace in place" the SQLite tier gets from `INSERT OR REPLACE`: a
        // re-distilled row overwrites every non-key column. Counters bind as `i64`
        // (Postgres has no unsigned types); the column is BIGINT so the full
        // `u32`/`u64` range the SQLite tier accepts is preserved.
        sqlx::query(
            "INSERT INTO digests
               (session_id, user_id, seq, kind, text, keywords, mode, model, ts_ms, duration_ms, tokens)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
             ON CONFLICT (session_id, seq, kind) DO UPDATE SET
               user_id     = EXCLUDED.user_id,
               text        = EXCLUDED.text,
               keywords    = EXCLUDED.keywords,
               mode        = EXCLUDED.mode,
               model       = EXCLUDED.model,
               ts_ms       = EXCLUDED.ts_ms,
               duration_ms = EXCLUDED.duration_ms,
               tokens      = EXCLUDED.tokens",
        )
        .bind(&digest.session_id)
        .bind(&digest.user_id)
        .bind(i64::try_from(digest.seq).unwrap_or(i64::MAX))
        .bind(digest.kind.as_str())
        .bind(&digest.text)
        .bind(&keywords)
        .bind(&digest.mode)
        .bind(&digest.model)
        .bind(i64::try_from(digest.ts_ms).unwrap_or(i64::MAX))
        .bind(i64::from(digest.duration_ms))
        .bind(i64::from(digest.tokens))
        .execute(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(())
    }

    async fn query(&self, q: &DigestQuery) -> Result<Vec<Digest>> {
        let (q, limit) = sanitize_query(q)?;
        // kind/user_id/since narrow in SQL; the keyword prefilter runs in Rust
        // AFTER the fetch and BEFORE the caller's limit (filtering after a SQL
        // LIMIT would starve matching rows), so the SQL fetch is capped at the
        // server ceiling only. `$2 = '' OR kind = $2` / `$3 = '' OR user_id = $3`
        // scope the read: an empty `user_id` is an unscoped/single-tenant read, a
        // non-empty one confines the read to the owning tenant so a shared/colliding
        // `session_id` cannot cross-read another user's ledger.
        let kind = q.kind.map(DigestKind::as_str).unwrap_or("");
        let since = i64::try_from(q.since_seq.unwrap_or(0)).unwrap_or(i64::MAX);
        let rows = sqlx::query(
            "SELECT session_id, user_id, seq, kind, text, keywords, mode, model,
                    ts_ms, duration_ms, tokens
               FROM digests
              WHERE session_id = $1
                AND ($2 = '' OR kind = $2)
                AND ($3 = '' OR user_id = $3)
                AND seq >= $4
              ORDER BY seq ASC
              LIMIT $5",
        )
        .bind(&q.session_id)
        .bind(kind)
        .bind(&q.user_id)
        .bind(since)
        .bind(crate::MAX_QUERY_LIMIT as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(pg_err)?;

        let mut out = Vec::new();
        for row in &rows {
            // A row that fails to decode (unknown kind, corrupt keywords) is
            // skipped, not fatal — fail closed on the row, soft on the read.
            let Some(d) = row_to_raw(row).into_digest() else {
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
    duration_ms: i64,
    tokens: i64,
}

impl RawRow {
    /// Decode, treating the store as untrusted: unknown kind ⇒ `None`, corrupt
    /// keywords ⇒ empty list, out-of-range counters ⇒ 0.
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
            duration_ms: u32::try_from(self.duration_ms).unwrap_or(0),
            tokens: u32::try_from(self.tokens).unwrap_or(0),
        })
    }
}

fn row_to_raw(row: &sqlx::postgres::PgRow) -> RawRow {
    RawRow {
        session_id: row.get::<String, _>("session_id"),
        user_id: row.get::<String, _>("user_id"),
        seq: row.get::<i64, _>("seq"),
        kind: row.get::<String, _>("kind"),
        text: row.get::<String, _>("text"),
        keywords: row.get::<String, _>("keywords"),
        mode: row.get::<String, _>("mode"),
        model: row.get::<String, _>("model"),
        ts_ms: row.get::<i64, _>("ts_ms"),
        duration_ms: row.get::<i64, _>("duration_ms"),
        tokens: row.get::<i64, _>("tokens"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Every test needs a live Postgres, so the whole module is `#[ignore]`-gated
    // (run only via `nix run .#integration`, which sets `AGENT_DIGEST_TEST_DSN`);
    // it still *compiles* in-gate under `clippy --all-features`.
    const REQUIRES_PG: &str =
        "requires a live Postgres (AGENT_DIGEST_TEST_DSN); run via nix run .#integration";

    /// Connect, migrate, and TRUNCATE to a clean slate so every scenario starts
    /// from an empty ledger regardless of run order.
    async fn pg_digests() -> PgDigests {
        let pool = test_pool().await;
        sqlx::query("TRUNCATE digests")
            .execute(&pool)
            .await
            .expect("truncate digests");
        PgDigests::from_pool(pool)
    }

    /// A migrated pool on the test DSN (no reset) — for the tests that need the raw
    /// pool alongside the ledger (planting a corrupt row, reconnect durability).
    async fn test_pool() -> PgPool {
        let dsn = std::env::var("AGENT_DIGEST_TEST_DSN")
            .expect("AGENT_DIGEST_TEST_DSN must be set by the pg-integration harness");
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect(&dsn)
            .await
            .expect("connect postgres");
        PgDigests::run_migrations(&pool).await.expect("migrate");
        pool
    }

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

    /// A digest owned by a specific tenant (`user_id`).
    fn du(session: &str, user: &str, seq: u64, text: &str) -> Digest {
        Digest {
            user_id: user.into(),
            ..d(session, seq, DigestKind::Summary, text, &[])
        }
    }

    #[tokio::test]
    #[ignore = "requires a live Postgres (AGENT_DIGEST_TEST_DSN); run via nix run .#integration"]
    async fn positive_put_then_query_ordered_by_seq() {
        let s = pg_digests().await;
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
        assert_eq!(rows[0].model, "kimi");
        assert_eq!(rows[0].tokens, 7);
    }

    #[tokio::test]
    #[ignore = "requires a live Postgres (AGENT_DIGEST_TEST_DSN); run via nix run .#integration"]
    async fn positive_kind_and_since_filters() {
        let s = pg_digests().await;
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
    #[ignore = "requires a live Postgres (AGENT_DIGEST_TEST_DSN); run via nix run .#integration"]
    async fn positive_keyword_prefilter_case_insensitive() {
        let s = pg_digests().await;
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
    #[ignore = "requires a live Postgres (AGENT_DIGEST_TEST_DSN); run via nix run .#integration"]
    async fn corner_replace_same_key_keeps_latest() {
        let s = pg_digests().await;
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
    #[ignore = "requires a live Postgres (AGENT_DIGEST_TEST_DSN); run via nix run .#integration"]
    async fn corner_sessions_are_isolated() {
        let s = pg_digests().await;
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
    #[ignore = "requires a live Postgres (AGENT_DIGEST_TEST_DSN); run via nix run .#integration"]
    async fn negative_unknown_session_is_empty_not_error() {
        let s = pg_digests().await;
        assert!(s.query(&q("nope")).await.unwrap().is_empty());
    }

    #[tokio::test]
    #[ignore = "requires a live Postgres (AGENT_DIGEST_TEST_DSN); run via nix run .#integration"]
    async fn boundary_limit_caps_rows() {
        let s = pg_digests().await;
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

    // R2: two tenants sharing a `session_id` must never cross-read each other's
    // ledger — the read is scoped to `user_id`.
    #[tokio::test]
    #[ignore = "requires a live Postgres (AGENT_DIGEST_TEST_DSN); run via nix run .#integration"]
    async fn adversarial_read_scoped_by_user_id_isolates_tenants() {
        let s = pg_digests().await;
        s.put(du("shared", "alice", 1, "alice-secret"))
            .await
            .unwrap();
        s.put(du("shared", "bob", 2, "bob-secret")).await.unwrap();
        let alice = s
            .query(&DigestQuery {
                session_id: "shared".into(),
                user_id: "alice".into(),
                ..DigestQuery::default()
            })
            .await
            .unwrap();
        assert_eq!(alice.len(), 1, "alice sees only her own row");
        assert_eq!(alice[0].text, "alice-secret");
        assert!(
            alice.iter().all(|d| d.user_id == "alice"),
            "no bob rows leaked into alice's scoped read"
        );
    }

    // Backward-compat: an empty `user_id` is an unscoped read (single-tenant),
    // returning every user's rows for the session.
    #[tokio::test]
    #[ignore = "requires a live Postgres (AGENT_DIGEST_TEST_DSN); run via nix run .#integration"]
    async fn corner_empty_user_id_reads_all_users() {
        let s = pg_digests().await;
        s.put(du("shared", "alice", 1, "a")).await.unwrap();
        s.put(du("shared", "bob", 2, "b")).await.unwrap();
        let all = s.query(&q("shared")).await.unwrap();
        assert_eq!(all.len(), 2, "unscoped read returns both tenants' rows");
    }

    // ids reach SQL only as bound params, but they are rejected before binding by
    // `sanitize`/`sanitize_query` — a traversal/injection id fails closed on both
    // the write and the read path.
    #[tokio::test]
    #[ignore = "requires a live Postgres (AGENT_DIGEST_TEST_DSN); run via nix run .#integration"]
    async fn adversarial_traversal_ids_rejected_both_paths() {
        let s = pg_digests().await;
        assert!(s
            .put(d("../../etc", 1, DigestKind::Summary, "x", &[]))
            .await
            .is_err());
        assert!(s.query(&q("../..")).await.is_err());
    }

    // A hostile/updated store row whose `kind` is outside the closed set (or whose
    // keywords are corrupt) is skipped on read, not fatal. The `kind` CHECK
    // normally forbids such a row, so we drop the constraint for this throwaway-DB
    // test to plant one; TRUNCATE in the next test's `pg_digests()` clears the row.
    #[tokio::test]
    #[ignore = "requires a live Postgres (AGENT_DIGEST_TEST_DSN); run via nix run .#integration"]
    async fn adversarial_corrupt_row_is_skipped_not_fatal() {
        let pool = test_pool().await;
        sqlx::query("TRUNCATE digests")
            .execute(&pool)
            .await
            .unwrap();
        let s = PgDigests::from_pool(pool.clone());
        s.put(d("s1", 1, DigestKind::Summary, "good", &[]))
            .await
            .unwrap();
        // Plant a row with an unknown kind and corrupt keywords JSON (a hostile /
        // schema-drifted store). Dropping the CHECK is confined to this test's
        // throwaway DB.
        sqlx::query("ALTER TABLE digests DROP CONSTRAINT IF EXISTS digests_kind_check")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO digests (session_id, user_id, seq, kind, text, keywords, ts_ms)
             VALUES ('s1', 'local', 2, 'weaponized', 'evil', 'not-json', 2)",
        )
        .execute(&pool)
        .await
        .unwrap();
        let rows = s.query(&q("s1")).await.unwrap();
        assert_eq!(rows.len(), 1, "unknown-kind row skipped");
        assert_eq!(rows[0].text, "good");
    }

    #[tokio::test]
    #[ignore = "requires a live Postgres (AGENT_DIGEST_TEST_DSN); run via nix run .#integration"]
    async fn adversarial_oversize_text_capped_before_store() {
        let s = pg_digests().await;
        let mut big = d("s1", 1, DigestKind::Facts, "", &[]);
        big.text = "x".repeat(10 * crate::MAX_TEXT_BYTES);
        s.put(big).await.unwrap();
        let rows = s.query(&q("s1")).await.unwrap();
        assert!(rows[0].text.len() <= crate::MAX_TEXT_BYTES);
    }

    // Durability: a row written through one pool is visible to a fresh pool on the
    // same server — the ledger survives a process restart.
    #[tokio::test]
    #[ignore = "requires a live Postgres (AGENT_DIGEST_TEST_DSN); run via nix run .#integration"]
    async fn positive_reconnect_reads_persisted() {
        let dsn = std::env::var("AGENT_DIGEST_TEST_DSN").expect(REQUIRES_PG);
        {
            let s = pg_digests().await;
            s.put(d("s1", 1, DigestKind::Summary, "persisted", &[]))
                .await
                .unwrap();
        }
        // A brand-new pool (a "restart") on the same DSN.
        let s = PgDigests::connect(&dsn, 2, true).await.unwrap();
        assert_eq!(s.query(&q("s1")).await.unwrap()[0].text, "persisted");
    }

    // The versioned runner is idempotent: a second `run_migrations` on an
    // already-migrated DB sees a full ledger and no-ops (not an error, not a
    // double-apply).
    #[tokio::test]
    #[ignore = "requires a live Postgres (AGENT_DIGEST_TEST_DSN); run via nix run .#integration"]
    async fn positive_migrate_is_idempotent_on_reconnect() {
        let pool = test_pool().await; // first migrate
        PgDigests::run_migrations(&pool)
            .await
            .expect("second migrate no-ops");
    }

    // corner: after the full migration set, the dead `(session_id, kind, seq)`
    // index is gone (0002 dropped it) even though 0001 still creates it — the
    // versioned runner applies both in order. A fresh DB ends with no such index.
    #[tokio::test]
    #[ignore = "requires a live Postgres (AGENT_DIGEST_TEST_DSN); run via nix run .#integration"]
    async fn corner_migrate_drops_dead_kind_seq_index() {
        let pool = test_pool().await; // runs 0001 then 0002
        let present: bool = sqlx::query_scalar(
            "SELECT EXISTS (
                 SELECT 1 FROM pg_indexes
                  WHERE tablename = 'digests'
                    AND indexname = 'idx_digests_session_kind_seq'
             )",
        )
        .fetch_one(&pool)
        .await
        .expect("query pg_indexes");
        assert!(
            !present,
            "0002 must leave no idx_digests_session_kind_seq on a fully-migrated DB"
        );
    }
}
