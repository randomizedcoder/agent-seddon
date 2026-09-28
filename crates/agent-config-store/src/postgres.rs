//! `PgBackend` — the Postgres [`Backend`], behind the non-default
//! `config-store-postgres` feature. The same opaque-blob model as the SQLite
//! tier (`src/sqlite.rs`), over a real server via `sqlx` (pure Rust, the
//! in-tree rustls TLS stack — no `libpq`).
//!
//! Unlike SQLite this tier is **not hermetic** — it needs a live server — so it
//! is exercised only via `nix run .#integration` (the `pg-integration` harness
//! spins up a container), never inside `nix flake check`. The crate still
//! *compiles* under this feature with no database: every query is
//! runtime-checked (`sqlx::query`, not the compile-time `query!` macro), so no
//! `DATABASE_URL` is needed at build time.
//!
//! Security posture mirrors SQLite: **ids/tenants reach SQL only as bound
//! parameters** (`$1`…), a card field is inside the opaque `blob`, and a whole
//! batch commits or rolls back as one Postgres (MVCC) transaction. The
//! fail-closed [`check_batch`] runs inside that transaction against a snapshot
//! of `tenants`, so the tenant foreign-key check and the writes are atomic.
//!
//! Schema is applied by a small **versioned** runner ([`PgBackend::run_migrations`])
//! over the embedded [`MIGRATIONS`] set (each `migrations/*.sql` pulled in with
//! `include_str!`): each numbered step is applied **exactly once**, recorded in a
//! `_schema_migrations` ledger, so a later non-idempotent step (an `ALTER`) is
//! safe on restart. We deliberately do **not** use `sqlx::migrate!`: its `macros`
//! feature pulls in every sqlx driver (`sqlx-mysql`, `sqlx-sqlite`) regardless of
//! the one we use, and `sqlx-mysql` drags in `rsa` — a crate with an unfixable
//! timing-sidechannel advisory (RUSTSEC-2023-0071) that `cargo audit` rejects. A
//! hand-rolled runner over the base `sqlx` API keeps the build to the Postgres
//! driver alone while giving the same exactly-once, versioned guarantee.

use std::collections::HashSet;

use agent_core::{Error, Result};
use async_trait::async_trait;
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};

use crate::{check_batch, conflict, Backend, Write};

/// The embedded migrations, in apply order: `(version, sql)`. A version is
/// applied exactly once and recorded in the `_schema_migrations` ledger, so a
/// non-idempotent step (a later `ALTER`) runs a single time even across restarts.
/// Adding a migration = drop the next-numbered `.sql` in `migrations/` and append
/// its `(n, include_str!(...))` here (the version is the source of truth, not the
/// filename — no runtime path parsing).
pub(crate) const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("../migrations/0001_config_store.sql")),
    (
        2,
        include_str!("../migrations/0002_cards_pos_identity_and_list_index.sql"),
    ),
];

/// A fixed, arbitrary key for the transaction-scoped advisory lock that
/// serializes concurrent starters through [`PgBackend::run_migrations`] (so two
/// processes booting at once never race the same non-idempotent step). Any stable
/// constant works; this one is `"agent-config-store"` folded into an `i64`.
const MIGRATION_LOCK_KEY: i64 = 0x6167_636f_6e66_6773_u64 as i64;

/// A Postgres-backed config store (a connection pool + the shared schema).
pub struct PgBackend {
    pool: PgPool,
    /// `Some` ⇒ apply the embedded migrations before the first query
    /// ([`Self::migrate_lazily`]): a lazily-connected backend has no connection
    /// when it is built, so the schema waits for first use.
    lazy_schema: Option<tokio::sync::OnceCell<()>>,
}

fn pg_err(e: sqlx::Error) -> Error {
    Error::Config(format!("postgres: {e}"))
}

impl PgBackend {
    /// Connect a pool to `dsn` (max `pool_max` connections, clamped to ≥1) and,
    /// when `migrate_on_start`, apply the embedded migrations idempotently.
    ///
    /// The `dsn` is already the *resolved* connection string (from a
    /// `dsn_ref` `env:`/`file:` reference — never an inline literal); it is not
    /// echoed on error, since it carries a password.
    pub async fn connect(dsn: &str, pool_max: u32, migrate_on_start: bool) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(pool_max.max(1))
            .connect(dsn)
            .await
            .map_err(|e| Error::Config(format!("postgres: connect failed: {e}")))?;
        if migrate_on_start {
            Self::run_migrations(&pool).await?;
        }
        Ok(Self {
            pool,
            lazy_schema: None,
        })
    }

    /// Apply the embedded [`MIGRATIONS`] set exactly once, in order.
    ///
    /// The whole run is one transaction guarded by a transaction-scoped advisory
    /// lock ([`MIGRATION_LOCK_KEY`]), so concurrent starters are serialized — the
    /// second waits, then sees a fully-applied ledger and no-ops. DDL is
    /// transactional in Postgres, so the `_schema_migrations` bookkeeping and each
    /// step's DDL commit (or roll back) together: a crash mid-run never leaves a
    /// half-applied, half-recorded step. On a DB carrying the pre-runner `0001`
    /// schema but no ledger, the empty history re-runs `0001` (`CREATE TABLE IF
    /// NOT EXISTS`, inert) and records it — so upgrading in place is safe.
    pub(crate) async fn run_migrations(pool: &PgPool) -> Result<()> {
        let mut tx = pool.begin().await.map_err(pg_err)?;
        // Serialize concurrent starters; the lock releases on commit/rollback.
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(MIGRATION_LOCK_KEY)
            .execute(&mut *tx)
            .await
            .map_err(pg_err)?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS _schema_migrations (
                 version    BIGINT      NOT NULL PRIMARY KEY,
                 applied_at TIMESTAMPTZ NOT NULL DEFAULT now()
             )",
        )
        .execute(&mut *tx)
        .await
        .map_err(pg_err)?;
        let applied: HashSet<i64> =
            sqlx::query_scalar::<_, i64>("SELECT version FROM _schema_migrations")
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
            // Run through `Executor::execute` rather than `RawSql::execute`: the
            // latter's lifetime bounds make this future unprovably `Send`, and
            // the lazy schema barrier awaits it inside the `Backend` methods.
            sqlx::Executor::execute(&mut *tx, sqlx::raw_sql(sql))
                .await
                .map_err(|e| Error::Config(format!("postgres: migration {version} failed: {e}")))?;
            sqlx::query("INSERT INTO _schema_migrations (version) VALUES ($1)")
                .bind(version)
                .execute(&mut *tx)
                .await
                .map_err(pg_err)?;
        }
        tx.commit().await.map_err(pg_err)?;
        Ok(())
    }

    /// Build a lazily-connecting pool to `dsn` (max `pool_max`, clamped to ≥1)
    /// **synchronously**: the DSN is validated now, but connections open on first
    /// use — mirroring the lazy-connect discipline the gRPC clients use, so a
    /// sync config resolver (`resolve_provider_registry`) can construct the
    /// backend without an async context. Schema is **not** applied here (there is
    /// no connection yet): chain [`Self::migrate_lazily`] to apply it on first
    /// use (`[config_store] migrate_on_start`), else the shared `cards`/`tenants`
    /// tables must already exist. The DSN is never echoed on error (it carries a
    /// password).
    pub fn connect_lazy(dsn: &str, pool_max: u32) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(pool_max.max(1))
            // Never echo `e`: a DSN parse error can contain the connection string.
            .connect_lazy(dsn)
            .map_err(|_| Error::Config("postgres: invalid DSN (could not parse)".into()))?;
        Ok(Self {
            pool,
            lazy_schema: None,
        })
    }

    /// With `on`, apply the embedded migrations once, before this backend's first
    /// query (security-hardening S15b: a lazily-built session store on a fresh
    /// database otherwise failed every call with `relation "cards" does not
    /// exist`). A failed attempt is not remembered, so a server that comes up
    /// after the agent is migrated on the next call; concurrent first uses, in
    /// this process or another, serialize on the runner's advisory lock.
    pub fn migrate_lazily(mut self, on: bool) -> Self {
        self.lazy_schema = on.then(tokio::sync::OnceCell::new);
        self
    }

    /// The schema barrier every query passes (a no-op unless [`Self::migrate_lazily`]).
    async fn ready(&self) -> Result<()> {
        match &self.lazy_schema {
            None => Ok(()),
            Some(cell) => cell
                .get_or_try_init(|| Self::run_migrations(&self.pool))
                .await
                .map(|&()| ()),
        }
    }

    /// Build a backend over an already-established pool (tests/embedding).
    pub fn from_pool(pool: PgPool) -> Self {
        Self {
            pool,
            lazy_schema: None,
        }
    }
}

#[async_trait]
impl Backend for PgBackend {
    async fn get(&self, collection: &str, tenant: &str, id: &str) -> Result<Option<Vec<u8>>> {
        self.ready().await?;
        let row =
            sqlx::query("SELECT blob FROM cards WHERE collection = $1 AND tenant = $2 AND id = $3")
                .bind(collection)
                .bind(tenant)
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .map_err(pg_err)?;
        Ok(row.map(|r| r.get::<Vec<u8>, _>("blob")))
    }

    async fn list(&self, collection: &str, tenant: &str) -> Result<Vec<Vec<u8>>> {
        self.ready().await?;
        let rows = sqlx::query(
            "SELECT blob FROM cards WHERE collection = $1 AND tenant = $2 ORDER BY pos, id",
        )
        .bind(collection)
        .bind(tenant)
        .fetch_all(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(rows
            .into_iter()
            .map(|r| r.get::<Vec<u8>, _>("blob"))
            .collect())
    }

    async fn count(&self, collection: &str, tenant: &str) -> Result<usize> {
        self.ready().await?;
        let n: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM cards WHERE collection = $1 AND tenant = $2")
                .bind(collection)
                .bind(tenant)
                .fetch_one(&self.pool)
                .await
                .map_err(pg_err)?;
        Ok(n.max(0) as usize)
    }

    async fn tenants(&self, collection: &str) -> Result<Vec<String>> {
        self.ready().await?;
        // `SELECT DISTINCT tenant … WHERE collection = $1` reads EVERY card in the
        // collection to recover its distinct tenants — O(rows), even as an
        // index-only scan. That is a scaling hazard on the scheduler's hot path:
        // the tenant-fair driver calls this once PER TICK to enumerate tenants
        // owning a job card (`scheduler_driver::rotated_tenants`), so the per-tick
        // cost grows with the total job-card count, not the tenant count.
        //
        // Do a loose index scan (a "skip scan") instead: seed with the first
        // tenant, then repeatedly jump to the next tenant strictly greater than
        // the last via the `cards` PK's `(collection, tenant, …)` prefix — one
        // index seek per DISTINCT tenant, O(tenants), not O(rows). The recursion
        // ends when the look-ahead subquery finds no greater tenant (a trailing
        // NULL row, filtered out). Results come back sorted ascending, identical
        // to the old query. `collection` is a bound parameter (never interpolated).
        let rows: Vec<String> = sqlx::query_scalar(
            "WITH RECURSIVE t AS (
                 (SELECT tenant FROM cards WHERE collection = $1 ORDER BY tenant LIMIT 1)
                 UNION ALL
                 SELECT (SELECT c.tenant FROM cards c
                          WHERE c.collection = $1 AND c.tenant > t.tenant
                          ORDER BY c.tenant LIMIT 1)
                   FROM t WHERE t.tenant IS NOT NULL
             )
             SELECT tenant FROM t WHERE tenant IS NOT NULL ORDER BY tenant",
        )
        .bind(collection)
        .fetch_all(&self.pool)
        .await
        .map_err(pg_err)?;
        Ok(rows)
    }

    async fn apply(&self, writes: &[Write]) -> Result<()> {
        self.ready().await?;
        let mut tx = self.pool.begin().await.map_err(pg_err)?;

        // Snapshot existing tenants inside the transaction so the fail-closed
        // FK check and the writes commit as one MVCC unit.
        let existing: HashSet<String> =
            sqlx::query_scalar::<_, String>("SELECT tenant FROM tenants")
                .fetch_all(&mut *tx)
                .await
                .map_err(pg_err)?
                .into_iter()
                .collect();
        check_batch(writes, |t| existing.contains(t))?;

        for w in writes {
            match w {
                Write::EnsureTenant { tenant } => {
                    sqlx::query(
                        "INSERT INTO tenants (tenant) VALUES ($1) ON CONFLICT (tenant) DO NOTHING",
                    )
                    .bind(tenant)
                    .execute(&mut *tx)
                    .await
                    .map_err(pg_err)?;
                }
                Write::Put {
                    collection,
                    tenant,
                    id,
                    blob,
                } => {
                    // New rows get the next `pos` from the identity sequence
                    // (insertion order) — omitting the column lets the DEFAULT
                    // fill it, so there is no `MAX(pos)` full-table aggregate on
                    // the write path (PG-02). An existing row keeps its pos and
                    // only swaps the blob.
                    sqlx::query(
                        "INSERT INTO cards (collection, tenant, id, blob)
                         VALUES ($1, $2, $3, $4)
                         ON CONFLICT (collection, tenant, id) DO UPDATE SET blob = EXCLUDED.blob",
                    )
                    .bind(collection)
                    .bind(tenant)
                    .bind(id)
                    .bind(blob.as_slice())
                    .execute(&mut *tx)
                    .await
                    .map_err(pg_err)?;
                }
                Write::Delete {
                    collection,
                    tenant,
                    id,
                } => {
                    sqlx::query(
                        "DELETE FROM cards WHERE collection = $1 AND tenant = $2 AND id = $3",
                    )
                    .bind(collection)
                    .bind(tenant)
                    .bind(id)
                    .execute(&mut *tx)
                    .await
                    .map_err(pg_err)?;
                }
                Write::CompareAndSwap {
                    collection,
                    tenant,
                    id,
                    expected,
                    blob,
                } => {
                    match expected.as_deref() {
                        // Insert-if-absent (e.g. the review-fleet post-lease
                        // claim). `SELECT … FOR UPDATE` cannot lock a row that
                        // does not exist yet, so a SELECT-then-INSERT lets two
                        // connections both observe "absent", both pass the
                        // `expected = None` check, and both land the row (the
                        // second via `ON CONFLICT DO UPDATE`) — silently
                        // breaking cross-connection mutual exclusion (two
                        // racers each see `Acquired`). Claim atomically with
                        // `ON CONFLICT DO NOTHING` and read the command tag
                        // instead: exactly one racer inserts (`rows_affected
                        // == 1`), and every loser is arbitrated by the unique
                        // index (`rows_affected == 0`) → the CAS conflict. The
                        // unique-index contention IS cross-connection atomic,
                        // unlike a phantom-row `FOR UPDATE`.
                        None => {
                            let res = sqlx::query(
                                "INSERT INTO cards (collection, tenant, id, blob)
                                 VALUES ($1, $2, $3, $4)
                                 ON CONFLICT (collection, tenant, id) DO NOTHING",
                            )
                            .bind(collection)
                            .bind(tenant)
                            .bind(id)
                            .bind(blob.as_slice())
                            .execute(&mut *tx)
                            .await
                            .map_err(pg_err)?;
                            if res.rows_affected() == 0 {
                                return Err(conflict(collection, id));
                            }
                        }
                        // Replace-if-equals: the row must already exist, so
                        // `SELECT … FOR UPDATE` locks it and a second connection
                        // racing the same claim blocks until this txn
                        // commits/rolls back and then sees the updated value —
                        // the row lock is what makes this true cross-connection
                        // mutual exclusion. On mismatch (or an absent row) the
                        // `Err` drops `tx` before commit, rolling back the whole
                        // batch. A plain `UPDATE` of the matched row keeps its
                        // identity `pos`.
                        Some(exp) => {
                            let cur: Option<Vec<u8>> = sqlx::query_scalar(
                                "SELECT blob FROM cards WHERE collection = $1 AND tenant = $2 AND id = $3 FOR UPDATE",
                            )
                            .bind(collection)
                            .bind(tenant)
                            .bind(id)
                            .fetch_optional(&mut *tx)
                            .await
                            .map_err(pg_err)?;
                            if cur.as_deref() != Some(exp) {
                                return Err(conflict(collection, id));
                            }
                            sqlx::query(
                                "UPDATE cards SET blob = $4 WHERE collection = $1 AND tenant = $2 AND id = $3",
                            )
                            .bind(collection)
                            .bind(tenant)
                            .bind(id)
                            .bind(blob.as_slice())
                            .execute(&mut *tx)
                            .await
                            .map_err(pg_err)?;
                        }
                    }
                }
            }
        }
        tx.commit().await.map_err(pg_err)?;
        Ok(())
    }
}
