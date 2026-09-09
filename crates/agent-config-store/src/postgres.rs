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
//! The schema is a single idempotent DDL script (`migrations/`, embedded via
//! `include_str!`) applied with the simple-query protocol when
//! `migrate_on_start` is set — enough for one small control-plane table pair
//! without pulling `sqlx`'s macro/migrate machinery into the build.

use std::collections::HashSet;

use agent_core::{Error, Result};
use async_trait::async_trait;
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};

use crate::{check_batch, Backend, Write};

/// A Postgres-backed config store (a connection pool + the shared schema).
pub struct PgBackend {
    pool: PgPool,
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
            // One idempotent DDL script via the simple-query protocol
            // (multi-statement); `CREATE TABLE IF NOT EXISTS` makes re-runs safe.
            sqlx::raw_sql(include_str!("../migrations/0001_config_store.sql"))
                .execute(&pool)
                .await
                .map_err(|e| Error::Config(format!("postgres: migrate failed: {e}")))?;
        }
        Ok(Self { pool })
    }

    /// Build a lazily-connecting pool to `dsn` (max `pool_max`, clamped to ≥1)
    /// **synchronously**: the DSN is validated now, but connections open on first
    /// use — mirroring the lazy-connect discipline the gRPC clients use, so a
    /// sync config resolver (`resolve_provider_registry`) can construct the
    /// backend without an async context. Schema is **not** applied here (there is
    /// no connection yet); a lazy deployment assumes the schema is present (the
    /// shared `cards`/`tenants` tables from the config-store migration), or is
    /// migrated out of band. The DSN is never echoed on error (it carries a
    /// password).
    pub fn connect_lazy(dsn: &str, pool_max: u32) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(pool_max.max(1))
            // Never echo `e`: a DSN parse error can contain the connection string.
            .connect_lazy(dsn)
            .map_err(|_| Error::Config("postgres: invalid DSN (could not parse)".into()))?;
        Ok(Self { pool })
    }

    /// Build a backend over an already-established pool (tests/embedding).
    pub fn from_pool(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl Backend for PgBackend {
    async fn get(&self, collection: &str, tenant: &str, id: &str) -> Result<Option<Vec<u8>>> {
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
        let n: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM cards WHERE collection = $1 AND tenant = $2")
                .bind(collection)
                .bind(tenant)
                .fetch_one(&self.pool)
                .await
                .map_err(pg_err)?;
        Ok(n.max(0) as usize)
    }

    async fn apply(&self, writes: &[Write]) -> Result<()> {
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
                    // New rows take the next global `pos` (insertion order,
                    // visible within this txn); an existing row keeps its pos
                    // and only swaps the blob.
                    sqlx::query(
                        "INSERT INTO cards (collection, tenant, id, pos, blob)
                         VALUES ($1, $2, $3, (SELECT COALESCE(MAX(pos), -1) + 1 FROM cards), $4)
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
            }
        }
        tx.commit().await.map_err(pg_err)?;
        Ok(())
    }
}
