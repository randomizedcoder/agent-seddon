//! [`PgRepoGraph`] — the Postgres [`RepoGraphStore`], behind the non-default
//! `repo-graph-postgres` feature. The same contract as `agent_testkit::repo_graph::MemRepoGraph`
//! (the conformance suite runs unchanged over both) on a real server via `sqlx` (pure Rust,
//! rustls, no `libpq`): the tables of `docs/design/repo-knowledge/01-schema.md`, the recursive-CTE
//! reads of `03-queries.md`, every write one all-or-nothing transaction.
//!
//! Unlike the memory tier this one is **not hermetic** — it needs a live server — so its
//! conformance tests are `#[ignore]`-gated and run only via `nix run .#pg-integration`, never
//! inside `nix flake check`. The crate still *compiles* under this feature with no database: every
//! statement is runtime-checked (`sqlx::query`, never the compile-time `query!`), so the gate
//! type-checks and lints this code (`clippy --all-features`) and the pure helpers' unit table runs
//! (`nix/checks/repo-graph.nix`) without a server.
//!
//! Shape, mirrored from the memory tier so the two cannot drift:
//!
//! * a **tenant-bound handle** ([`PgRepoGraph::with_tenant`] fails closed on anything that is not a
//!   [`safe_segment`], before any statement is issued); every statement binds the tenant, so a
//!   foreign tenant's row is simply `NotFound`;
//! * an **injectable epoch-ms clock** ([`PgRepoGraph::with_clock`]) wherever the design says
//!   `now()`, so the conformance rows drive `built_at` / `created_at` deterministically;
//! * **reads clamp** their hops / caps / list lengths in Rust and return empty for an unknown or
//!   hostile key, never an error that echoes it;
//! * **writes** are one transaction: `snapshot_write` bulk-inserts via `UNNEST` with the node-id
//!   collision check, all-or-nothing.
//!
//! Every statement lives in [`sql`]; this file never builds SQL from strings.
//!
//! Schema is applied by a small **versioned** runner ([`PgRepoGraph::run_migrations`]) over the
//! embedded [`MIGRATIONS`] set, exactly like the digest / campaign / config-store tiers. We
//! deliberately do **not** use `sqlx::migrate!`: its `macros` feature pulls in every sqlx driver,
//! and `sqlx-mysql` drags in `rsa` (RUSTSEC-2023-0071, rejected by `cargo audit`).

mod sql;

use agent_core::repo_graph::{
    Direction, EdgeKind, ExtractReport, GraphDiff, NodeId, NodeKey, NodeKind, NodeRow, Repo,
    RepoGraph, RepoGraphError, RepoGraphResult, RepoGraphStore, RepoId, RepoSpec, Scope, Shape,
    Snapshot, SnapshotBegin, SnapshotId, SnapshotStatus, TestHit, Neighbor,
};
use agent_core::safe_segment;
use async_trait::async_trait;
use sqlx::error::ErrorKind;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::collections::HashSet;
use std::sync::Arc;

/// The embedded migrations, in apply order: `(version, sql)`. A version is applied exactly once and
/// recorded in the `_repo_graph_migrations` ledger. Adding a migration = drop the next-numbered
/// `.sql` in `migrations/` and append its `(n, include_str!(...))` here (the version is the source
/// of truth, not the filename).
const MIGRATIONS: &[(i64, &str)] = &[(1, include_str!("../migrations/0001_repo_graph.sql"))];

/// The transaction-scoped advisory lock that serializes concurrent starters through
/// [`PgRepoGraph::run_migrations`]: `"agrepogr"` folded into an `i64`, distinct from the digest
/// (`"agdigest"`), campaign (`"agcampgn"`) and config-store (`"agconfgs"`) keys so the runners
/// never contend.
const MIGRATION_LOCK_KEY: i64 = 0x6167_7265_706f_6772_u64 as i64;

/// A Postgres-backed repo-graph store: a pool, the tenant it is bound to, and the clock.
#[derive(Clone)]
pub struct PgRepoGraph {
    pool: PgPool,
    tenant: String,
    now_ms: Arc<dyn Fn() -> u64 + Send + Sync>,
}

impl std::fmt::Debug for PgRepoGraph {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never the pool or the DSN it carries — only the tenant.
        f.debug_struct("PgRepoGraph")
            .field("tenant", &self.tenant)
            .finish_non_exhaustive()
    }
}

fn wall_clock_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// A short `Backend` fault carrying a static description of what failed, never a row or a DSN.
fn backend(what: &str) -> RepoGraphError {
    RepoGraphError::Backend(format!("repo-graph postgres: {what}"))
}

/// The typed error for a driver error. Constraint violations are mapped **by constraint name and
/// kind**, never by message text; nothing from the row or the DSN is echoed.
fn map_db(e: sqlx::Error) -> RepoGraphError {
    match &e {
        sqlx::Error::Database(db) => {
            let name = db.constraint().unwrap_or("");
            match db.kind() {
                ErrorKind::UniqueViolation if name == "graph_snapshots_identity_key" => {
                    RepoGraphError::Conflict("duplicate snapshot identity".to_string())
                }
                ErrorKind::UniqueViolation => {
                    RepoGraphError::Conflict(format!("unique: {name}"))
                }
                ErrorKind::ForeignKeyViolation
                | ErrorKind::NotNullViolation
                | ErrorKind::CheckViolation => {
                    RepoGraphError::Invalid(format!("constraint: {name}"))
                }
                _ => RepoGraphError::Backend(format!(
                    "repo-graph postgres: sqlstate {}",
                    db.code()
                        .map(std::borrow::Cow::into_owned)
                        .unwrap_or_default()
                )),
            }
        }
        sqlx::Error::RowNotFound => RepoGraphError::NotFound,
        // Any other driver error (pool, protocol, decode) is a backend fault; the `Display` of a
        // non-`Database` sqlx error carries no DSN.
        _ => RepoGraphError::Backend(format!("repo-graph postgres: {e}")),
    }
}

impl PgRepoGraph {
    /// Connect a pool to `dsn` (max `pool_max` connections, clamped to ≥1) bound to the `local`
    /// tenant and, when `migrate_on_start`, apply the embedded migrations. The DSN is never echoed
    /// on error (it carries a password).
    pub async fn connect(dsn: &str, pool_max: u32, migrate_on_start: bool) -> RepoGraphResult<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(pool_max.max(1))
            .connect(dsn)
            .await
            .map_err(|_| backend("connect failed"))?;
        if migrate_on_start {
            Self::run_migrations(&pool).await?;
        }
        Ok(Self::from_pool(pool))
    }

    /// A lazily-connecting pool (the DSN is validated now, connections open on first use); the
    /// schema is **not** applied here. The DSN is never echoed on error.
    pub fn connect_lazy(dsn: &str, pool_max: u32) -> RepoGraphResult<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(pool_max.max(1))
            .connect_lazy(dsn)
            .map_err(|_| backend("invalid DSN (could not parse)"))?;
        Ok(Self::from_pool(pool))
    }

    /// A store over an already-established pool, bound to the `local` tenant, on the wall clock.
    pub fn from_pool(pool: PgPool) -> Self {
        Self {
            pool,
            tenant: "local".to_string(),
            now_ms: Arc::new(wall_clock_ms),
        }
    }

    /// The tenant this handle is bound to.
    pub fn tenant(&self) -> &str {
        &self.tenant
    }

    /// The same pool and clock under `tenant`; refuses anything that is not a [`safe_segment`]
    /// (traversal, empty, over-length) **before any statement is issued**.
    pub fn with_tenant(&self, tenant: &str) -> RepoGraphResult<Self> {
        if !safe_segment(tenant) {
            return Err(RepoGraphError::Invalid(
                "tenant: must be a non-empty path-safe segment".to_string(),
            ));
        }
        Ok(Self {
            pool: self.pool.clone(),
            tenant: tenant.to_string(),
            now_ms: Arc::clone(&self.now_ms),
        })
    }

    /// Replace the clock (epoch milliseconds). Tests drive `built_at` / `created_at` with it.
    #[doc(hidden)]
    pub fn with_clock(mut self, now_ms: Arc<dyn Fn() -> u64 + Send + Sync>) -> Self {
        self.now_ms = now_ms;
        self
    }

    /// Apply the embedded migrations to this store's pool. Idempotent.
    pub async fn ensure_migrated(&self) -> RepoGraphResult<()> {
        Self::run_migrations(&self.pool).await
    }

    /// Apply the embedded [`MIGRATIONS`] set exactly once, in order, under a transaction-scoped
    /// advisory lock (concurrent starters serialize; the second sees a full ledger and no-ops). DDL
    /// is transactional in Postgres, so a step and its ledger row commit or roll back together.
    pub async fn run_migrations(pool: &PgPool) -> RepoGraphResult<()> {
        let mut tx = pool.begin().await.map_err(map_db)?;
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(MIGRATION_LOCK_KEY)
            .execute(&mut *tx)
            .await
            .map_err(map_db)?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS _repo_graph_migrations (
                 version    BIGINT      NOT NULL PRIMARY KEY,
                 applied_at TIMESTAMPTZ NOT NULL DEFAULT now()
             )",
        )
        .execute(&mut *tx)
        .await
        .map_err(map_db)?;
        let applied: HashSet<i64> =
            sqlx::query_scalar::<_, i64>("SELECT version FROM _repo_graph_migrations")
                .fetch_all(&mut *tx)
                .await
                .map_err(map_db)?
                .into_iter()
                .collect();
        for (version, body) in MIGRATIONS {
            if applied.contains(version) {
                continue;
            }
            // `raw_sql` uses the simple-query protocol: the whole script is one call.
            sqlx::raw_sql(body)
                .execute(&mut *tx)
                .await
                .map_err(|e| backend(&format!("migration {version} failed: {e}")))?;
            sqlx::query("INSERT INTO _repo_graph_migrations (version) VALUES ($1)")
                .bind(version)
                .execute(&mut *tx)
                .await
                .map_err(map_db)?;
        }
        tx.commit().await.map_err(map_db)
    }
}

// The trait impl is built verb-group by verb-group in the steps that follow (RK-02 steps 2–4);
// until a group lands its methods return a `Backend` fault so the crate compiles and the pure
// helpers can be exercised in the gate. Each group replaces its stubs with the real SQL.
#[async_trait]
impl RepoGraphStore for PgRepoGraph {
    // -- Repos (step 2) ----------------------------------------------------

    async fn repo_put(&self, _spec: &RepoSpec) -> RepoGraphResult<RepoId> {
        Err(backend("repo_put: unimplemented"))
    }

    async fn repo_get(&self, _slug: &str) -> RepoGraphResult<Option<Repo>> {
        Err(backend("repo_get: unimplemented"))
    }

    async fn repos(&self) -> RepoGraphResult<Vec<Repo>> {
        Err(backend("repos: unimplemented"))
    }

    // -- Snapshots (step 3) ------------------------------------------------

    async fn snapshot_begin(&self, _begin: &SnapshotBegin) -> RepoGraphResult<SnapshotId> {
        Err(backend("snapshot_begin: unimplemented"))
    }

    async fn snapshot_write(&self, _id: SnapshotId, _graph: &RepoGraph) -> RepoGraphResult<()> {
        Err(backend("snapshot_write: unimplemented"))
    }

    async fn snapshot_finish(
        &self,
        _id: SnapshotId,
        _status: SnapshotStatus,
        _reason: &str,
        _report: &ExtractReport,
    ) -> RepoGraphResult<()> {
        Err(backend("snapshot_finish: unimplemented"))
    }

    async fn snapshot_find(
        &self,
        _repo: RepoId,
        _commit_sha: &str,
    ) -> RepoGraphResult<Option<Snapshot>> {
        Err(backend("snapshot_find: unimplemented"))
    }

    async fn snapshot_latest(&self, _repo: RepoId) -> RepoGraphResult<Option<Snapshot>> {
        Err(backend("snapshot_latest: unimplemented"))
    }

    async fn snapshots(&self, _repo: RepoId, _limit: usize) -> RepoGraphResult<Vec<Snapshot>> {
        Err(backend("snapshots: unimplemented"))
    }

    async fn snapshot_delete_older_than(
        &self,
        _repo: RepoId,
        _keep: usize,
    ) -> RepoGraphResult<usize> {
        Err(backend("snapshot_delete_older_than: unimplemented"))
    }

    async fn snapshot_diff(&self, _a: SnapshotId, _b: SnapshotId) -> RepoGraphResult<GraphDiff> {
        Err(backend("snapshot_diff: unimplemented"))
    }

    // -- Reads (step 4) ----------------------------------------------------

    async fn nodes_by_key(
        &self,
        _scope: Scope,
        _keys: &[NodeKey],
    ) -> RepoGraphResult<Vec<NodeRow>> {
        Err(backend("nodes_by_key: unimplemented"))
    }

    async fn nodes_by_file(
        &self,
        _scope: Scope,
        _files: &[String],
    ) -> RepoGraphResult<Vec<NodeRow>> {
        Err(backend("nodes_by_file: unimplemented"))
    }

    async fn nodes_by_name(
        &self,
        _scope: Scope,
        _name: &str,
        _kind: Option<NodeKind>,
        _limit: usize,
    ) -> RepoGraphResult<Vec<NodeRow>> {
        Err(backend("nodes_by_name: unimplemented"))
    }

    async fn neighbors(
        &self,
        _scope: Scope,
        _seeds: &[NodeId],
        _kind: EdgeKind,
        _dir: Direction,
        _hops: u32,
        _cap: usize,
    ) -> RepoGraphResult<Vec<Neighbor>> {
        Err(backend("neighbors: unimplemented"))
    }

    async fn blast_radius(
        &self,
        _scope: Scope,
        _files: &[String],
        _hops: u32,
        _cap: usize,
    ) -> RepoGraphResult<Vec<String>> {
        Err(backend("blast_radius: unimplemented"))
    }

    async fn tests_covering(
        &self,
        _scope: Scope,
        _seeds: &[NodeId],
        _hops: u32,
        _cap: usize,
    ) -> RepoGraphResult<Vec<TestHit>> {
        Err(backend("tests_covering: unimplemented"))
    }

    async fn path_between(
        &self,
        _scope: Scope,
        _src: NodeId,
        _dst: NodeId,
        _max_hops: u32,
        _max_paths: usize,
    ) -> RepoGraphResult<Vec<Vec<NodeKey>>> {
        Err(backend("path_between: unimplemented"))
    }

    async fn shape(&self, _scope: Scope) -> RepoGraphResult<Shape> {
        Err(backend("shape: unimplemented"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    /// A handle over a lazily-connecting pool: the DSN parses but no connection is ever opened, so
    /// the `with_tenant` refusals are provably synchronous (no database needed). `connect_lazy`
    /// itself needs a Tokio context (the pool spawns a reaper), so the callers are `#[tokio::test]`
    /// — but no statement is ever issued, so no server is required.
    fn lazy() -> PgRepoGraph {
        PgRepoGraph::connect_lazy("postgres://u:p@localhost:5432/db", 1).expect("valid DSN parses")
    }

    #[tokio::test]
    async fn positive_with_tenant_valid() {
        let s = lazy().with_tenant("ta").expect("safe tenant");
        assert_eq!(s.tenant(), "ta");
    }

    #[tokio::test]
    async fn positive_with_tenant_clone_shares_pool() {
        let s = lazy().with_tenant("ta").unwrap();
        let c = s.clone();
        assert_eq!(c.tenant(), "ta");
    }

    #[rstest]
    #[case::traversal("../x")]
    #[case::space("a b")]
    #[case::slash("a/b")]
    #[case::dash("-x")]
    #[case::dotdot("..")]
    #[case::empty("")]
    #[tokio::test]
    async fn adversarial_tenant_refused_before_any_statement(#[case] tenant: &str) {
        // The lazy pool never connected; a refusal here is therefore synchronous, before SQL.
        let err = lazy().with_tenant(tenant).expect_err("unsafe tenant refused");
        assert!(matches!(err, RepoGraphError::Invalid(_)));
    }

    #[tokio::test]
    async fn boundary_tenant_128_ok_129_refused() {
        assert!(lazy().with_tenant(&"a".repeat(128)).is_ok());
        assert!(lazy().with_tenant(&"a".repeat(129)).is_err());
    }

    #[test]
    fn positive_map_db_row_not_found() {
        assert_eq!(map_db(sqlx::Error::RowNotFound), RepoGraphError::NotFound);
    }

    #[test]
    fn negative_map_db_other_is_backend() {
        // A non-`Database` error maps to `Backend` and never carries a DSN/password.
        let err = map_db(sqlx::Error::PoolClosed);
        match err {
            RepoGraphError::Backend(msg) => {
                assert!(msg.starts_with("repo-graph postgres:"));
                assert!(!msg.contains("localhost"));
                assert!(!msg.contains("password"));
            }
            other => panic!("expected Backend, got {other:?}"),
        }
    }
}
