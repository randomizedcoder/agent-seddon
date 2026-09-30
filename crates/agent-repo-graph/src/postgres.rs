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
    Direction, EdgeKind, EdgeRef, ExtractReport, GraphDiff, Lang, Neighbor, Node, NodeId, NodeKey,
    NodeKind, NodeRow, NodeVersion, Repo, RepoGraph, RepoGraphError, RepoGraphResult,
    RepoGraphStore, RepoId, RepoSpec, Scope, Shape, Snapshot, SnapshotBegin, SnapshotId,
    SnapshotStatus, TestHit, MAX_DIFF, MAX_KEYS, MAX_PATHS, MAX_REASON_LEN, MAX_SNAPSHOT_EDGES,
    MAX_SNAPSHOT_LIST, MAX_SNAPSHOT_NODES,
};
use agent_core::safe_segment;
use async_trait::async_trait;
use sqlx::error::ErrorKind;
use sqlx::postgres::{PgPoolOptions, PgRow};
use sqlx::{PgPool, Postgres, Row};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
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
                ErrorKind::UniqueViolation => RepoGraphError::Conflict(format!("unique: {name}")),
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

/// A typed column read; a decode failure is a `Backend` fault naming the column, never a panic.
fn col<'r, T>(row: &'r PgRow, name: &str) -> RepoGraphResult<T>
where
    T: sqlx::Decode<'r, Postgres> + sqlx::Type<Postgres>,
{
    row.try_get(name)
        .map_err(|e| backend(&format!("column {name}: {e}")))
}

/// A `usize` bound as the `LIMIT` / count parameter, saturating.
fn lim(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

/// Truncate on a char boundary (the `reason` cap), matching `MemRepoGraph`.
fn truncate_chars(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut cut = max;
    while !s.is_char_boundary(cut) {
        cut -= 1;
    }
    s[..cut].to_string()
}

/// Decode a stored `status` (the store is untrusted: an unknown value is a `Backend` fault).
fn parse_status(s: &str) -> RepoGraphResult<SnapshotStatus> {
    match s {
        "building" => Ok(SnapshotStatus::Building),
        "ready" => Ok(SnapshotStatus::Ready),
        "failed" => Ok(SnapshotStatus::Failed),
        _ => Err(backend("column status: unknown value")),
    }
}

/// Decode a stored `lang` (`""` is [`Lang::None`]); unknown ⇒ `Backend`.
fn parse_lang(s: &str) -> RepoGraphResult<Lang> {
    Ok(match s {
        "rust" => Lang::Rust,
        "go" => Lang::Go,
        "md" => Lang::Md,
        "proto" => Lang::Proto,
        "sql" => Lang::Sql,
        "toml" => Lang::Toml,
        "" => Lang::None,
        _ => return Err(backend("column lang: unknown value")),
    })
}

fn row_to_repo(row: &PgRow) -> RepoGraphResult<Repo> {
    let profile_text: String = col(row, "profile")?;
    let profile = serde_json::from_str(&profile_text)
        .map_err(|e| backend(&format!("column profile: {e}")))?;
    Ok(Repo {
        id: RepoId(col(row, "repo_id")?),
        slug: col(row, "slug")?,
        forge: col(row, "forge")?,
        remote_url: col(row, "remote_url")?,
        default_branch: col(row, "default_branch")?,
        profile,
        created_at_ms: sql::unsigned(col(row, "created_at_ms")?),
    })
}

fn row_to_snapshot(row: &PgRow) -> RepoGraphResult<Snapshot> {
    let extractors_text: String = col(row, "extractors")?;
    let extractors = serde_json::from_str(&extractors_text)
        .map_err(|e| backend(&format!("column extractors: {e}")))?;
    let status: String = col(row, "status")?;
    let node_count: i32 = col(row, "node_count")?;
    let edge_count: i32 = col(row, "edge_count")?;
    Ok(Snapshot {
        id: SnapshotId(col(row, "snapshot_id")?),
        repo: RepoId(col(row, "repo_id")?),
        commit_sha: col(row, "commit_sha")?,
        extractors,
        extractor_version: col(row, "extractor_version")?,
        graph_hash: col(row, "graph_hash")?,
        node_count: usize::try_from(node_count).unwrap_or(0),
        edge_count: usize::try_from(edge_count).unwrap_or(0),
        status: parse_status(&status)?,
        reason: col(row, "reason")?,
        built_at_ms: sql::unsigned(col(row, "built_at_ms")?),
        duration_ms: sql::unsigned(col(row, "duration_ms")?),
    })
}

/// Decode one node row (the shared `node_row_cols!` projection). Every store value is untrusted: a
/// bad key / kind / lang / attrs is a `Backend` fault, never a panic.
fn row_to_noderow(row: &PgRow) -> RepoGraphResult<NodeRow> {
    let key_text: String = col(row, "node_key")?;
    let key = NodeKey::parse(&key_text).map_err(|_| backend("column node_key: invalid"))?;
    let kind_text: String = col(row, "kind")?;
    let lang_text: String = col(row, "lang")?;
    let attrs_text: String = col(row, "attrs")?;
    let attrs =
        serde_json::from_str(&attrs_text).map_err(|e| backend(&format!("column attrs: {e}")))?;
    let line_start: i32 = col(row, "line_start")?;
    let line_end: i32 = col(row, "line_end")?;
    Ok(NodeRow {
        node: Node {
            key,
            id: NodeId(col(row, "node_id")?),
            kind: NodeKind::parse(&kind_text)
                .ok_or_else(|| backend("column kind: unknown value"))?,
            lang: parse_lang(&lang_text)?,
            name: col(row, "name")?,
            name_tokens: col(row, "name_tokens")?,
            qualifier: col(row, "qualifier")?,
        },
        version: NodeVersion {
            file: col(row, "file")?,
            line_start,
            line_end,
            sig_hash: col(row, "sig_hash")?,
            body_hash: col(row, "body_hash")?,
            exported: col(row, "exported")?,
            attrs,
        },
    })
}

impl PgRepoGraph {
    /// Connect a pool to `dsn` (max `pool_max` connections, clamped to ≥1) bound to the `local`
    /// tenant and, when `migrate_on_start`, apply the embedded migrations. The DSN is never echoed
    /// on error (it carries a password).
    pub async fn connect(
        dsn: &str,
        pool_max: u32,
        migrate_on_start: bool,
    ) -> RepoGraphResult<Self> {
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

    /// The read scope preflight: a snapshot must exist under `(tenant, repo, snapshot)`, else
    /// `NotFound`. This is how every cross-repo / cross-tenant read returns `NotFound` rather than
    /// leaking whether the snapshot exists elsewhere.
    async fn scope_ok(&self, scope: Scope) -> RepoGraphResult<()> {
        let found = sqlx::query(sql::SCOPE_EXISTS)
            .bind(&self.tenant)
            .bind(scope.repo.0)
            .bind(scope.snapshot.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(map_db)?;
        found.map(|_| ()).ok_or(RepoGraphError::NotFound)
    }
}

#[async_trait]
impl RepoGraphStore for PgRepoGraph {
    // -- Repos -------------------------------------------------------------

    async fn repo_put(&self, spec: &RepoSpec) -> RepoGraphResult<RepoId> {
        spec.validate()?;
        let now = sql::ms((self.now_ms)());
        let profile =
            serde_json::to_string(&spec.profile).map_err(|e| backend(&format!("profile: {e}")))?;
        let mut tx = self.pool.begin().await.map_err(map_db)?;
        sqlx::query(sql::ENSURE_TENANT)
            .bind(&self.tenant)
            .execute(&mut *tx)
            .await
            .map_err(map_db)?;
        let id: i64 = sqlx::query_scalar(sql::REPO_UPSERT)
            .bind(&self.tenant)
            .bind(&spec.slug)
            .bind(&spec.forge)
            .bind(&spec.remote_url)
            .bind(&spec.default_branch)
            .bind(&profile)
            .bind(now)
            .fetch_one(&mut *tx)
            .await
            .map_err(map_db)?;
        tx.commit().await.map_err(map_db)?;
        Ok(RepoId(id))
    }

    async fn repo_get(&self, slug: &str) -> RepoGraphResult<Option<Repo>> {
        let row = sqlx::query(sql::REPO_GET)
            .bind(&self.tenant)
            .bind(slug)
            .fetch_optional(&self.pool)
            .await
            .map_err(map_db)?;
        row.as_ref().map(row_to_repo).transpose()
    }

    async fn repos(&self) -> RepoGraphResult<Vec<Repo>> {
        sqlx::query(sql::REPOS)
            .bind(&self.tenant)
            .fetch_all(&self.pool)
            .await
            .map_err(map_db)?
            .iter()
            .map(row_to_repo)
            .collect()
    }

    // -- Snapshots ---------------------------------------------------------

    async fn snapshot_begin(&self, begin: &SnapshotBegin) -> RepoGraphResult<SnapshotId> {
        begin.validate()?;
        let now = sql::ms((self.now_ms)());
        let extractors = serde_json::to_string(&begin.extractors)
            .map_err(|e| backend(&format!("extractors: {e}")))?;
        let mut tx = self.pool.begin().await.map_err(map_db)?;
        if sqlx::query(sql::REPO_EXISTS)
            .bind(&self.tenant)
            .bind(begin.repo.0)
            .fetch_optional(&mut *tx)
            .await
            .map_err(map_db)?
            .is_none()
        {
            return Err(RepoGraphError::NotFound);
        }
        // The identity is unique; at most one row. A live one conflicts, a failed one is replaced.
        if let Some(row) = sqlx::query(sql::BEGIN_FIND)
            .bind(&self.tenant)
            .bind(begin.repo.0)
            .bind(&begin.commit_sha)
            .bind(&begin.extractor_version)
            .fetch_optional(&mut *tx)
            .await
            .map_err(map_db)?
        {
            let status: String = col(&row, "status")?;
            match status.as_str() {
                "building" | "ready" => {
                    return Err(RepoGraphError::Conflict("snapshot identity".to_string()));
                }
                "failed" => {
                    let sid: i64 = col(&row, "snapshot_id")?;
                    sqlx::query(sql::SNAPSHOT_DELETE_ONE)
                        .bind(&self.tenant)
                        .bind(sid)
                        .execute(&mut *tx)
                        .await
                        .map_err(map_db)?;
                }
                _ => return Err(backend("snapshot status: unknown value")),
            }
        }
        let id: i64 = sqlx::query_scalar(sql::BEGIN_INSERT)
            .bind(&self.tenant)
            .bind(begin.repo.0)
            .bind(&begin.commit_sha)
            .bind(&extractors)
            .bind(&begin.extractor_version)
            .bind(now)
            .fetch_one(&mut *tx)
            .await
            .map_err(map_db)?;
        tx.commit().await.map_err(map_db)?;
        Ok(SnapshotId(id))
    }

    async fn snapshot_write(&self, id: SnapshotId, graph: &RepoGraph) -> RepoGraphResult<()> {
        if graph.nodes().len() > MAX_SNAPSHOT_NODES {
            return Err(RepoGraphError::TooLong("nodes".to_string()));
        }
        if graph.edges().len() > MAX_SNAPSHOT_EDGES {
            return Err(RepoGraphError::TooLong("edges".to_string()));
        }
        let na = sql::node_arrays(graph);
        let ea = sql::edge_arrays(graph);
        // Per-row `name_tokens` bound as a comma-joined `text[]` (tokens are `[a-z0-9]`, no comma),
        // split back with `string_to_array` — avoids the `text[][]` UNNEST flattening trap.
        let toks: Vec<String> = na.tokens.iter().map(|t| t.join(",")).collect();

        let mut tx = self.pool.begin().await.map_err(map_db)?;
        let row = sqlx::query(sql::SNAPSHOT_LOCK)
            .bind(&self.tenant)
            .bind(id.0)
            .fetch_optional(&mut *tx)
            .await
            .map_err(map_db)?
            .ok_or(RepoGraphError::NotFound)?;
        let repo_id: i64 = col(&row, "repo_id")?;
        let status: String = col(&row, "status")?;
        if status != "building" {
            return Err(RepoGraphError::Conflict(
                "snapshot not building".to_string(),
            ));
        }

        for r in sql::chunk_ranges(na.ids.len(), sql::WRITE_CHUNK) {
            // Bodies: insert new, keep existing (shared across snapshots).
            sqlx::query(sql::NODES_INSERT)
                .bind(&self.tenant)
                .bind(repo_id)
                .bind(&na.ids[r.clone()])
                .bind(&na.keys[r.clone()])
                .bind(&na.kinds[r.clone()])
                .bind(&na.langs[r.clone()])
                .bind(&na.names[r.clone()])
                .bind(&toks[r.clone()])
                .bind(&na.qualifiers[r.clone()])
                .execute(&mut *tx)
                .await
                .map_err(map_db)?;
            // A distinct key mapped onto a stored id is a collision: the tx rolls back untouched.
            let collisions: i64 = sqlx::query_scalar(sql::COLLISION_CHECK)
                .bind(&self.tenant)
                .bind(repo_id)
                .bind(&na.ids[r.clone()])
                .bind(&na.keys[r.clone()])
                .fetch_one(&mut *tx)
                .await
                .map_err(map_db)?;
            if collisions > 0 {
                return Err(RepoGraphError::Conflict("node id collision".to_string()));
            }
        }
        for r in sql::chunk_ranges(na.ids.len(), sql::WRITE_CHUNK) {
            sqlx::query(sql::VERSIONS_INSERT)
                .bind(&self.tenant)
                .bind(repo_id)
                .bind(id.0)
                .bind(&na.ids[r.clone()])
                .bind(&na.files[r.clone()])
                .bind(&na.line_starts[r.clone()])
                .bind(&na.line_ends[r.clone()])
                .bind(&na.sig_hashes[r.clone()])
                .bind(&na.body_hashes[r.clone()])
                .bind(&na.exported[r.clone()])
                .bind(&na.attrs[r.clone()])
                .execute(&mut *tx)
                .await
                .map_err(map_db)?;
        }
        for r in sql::chunk_ranges(ea.kinds.len(), sql::WRITE_CHUNK) {
            sqlx::query(sql::EDGES_INSERT)
                .bind(&self.tenant)
                .bind(repo_id)
                .bind(id.0)
                .bind(&ea.kinds[r.clone()])
                .bind(&ea.src_ids[r.clone()])
                .bind(&ea.dst_ids[r.clone()])
                .bind(&ea.weights[r.clone()])
                .bind(&ea.attrs[r.clone()])
                .execute(&mut *tx)
                .await
                .map_err(map_db)?;
        }
        sqlx::query(sql::WRITE_META)
            .bind(&self.tenant)
            .bind(id.0)
            .bind(graph.graph_hash())
            .bind(sql::i32_of(graph.nodes().len()))
            .bind(sql::i32_of(graph.edges().len()))
            .execute(&mut *tx)
            .await
            .map_err(map_db)?;
        tx.commit().await.map_err(map_db)?;
        Ok(())
    }

    async fn snapshot_finish(
        &self,
        id: SnapshotId,
        status: SnapshotStatus,
        reason: &str,
        _report: &ExtractReport,
    ) -> RepoGraphResult<()> {
        let now = (self.now_ms)();
        let mut tx = self.pool.begin().await.map_err(map_db)?;
        let row = sqlx::query(sql::SNAPSHOT_LOCK)
            .bind(&self.tenant)
            .bind(id.0)
            .fetch_optional(&mut *tx)
            .await
            .map_err(map_db)?
            .ok_or(RepoGraphError::NotFound)?;
        let cur: String = col(&row, "status")?;
        if cur != "building" {
            return Err(RepoGraphError::Conflict(
                "snapshot not building".to_string(),
            ));
        }
        let built_at_ms: i64 = col(&row, "built_at_ms")?;
        let duration = now.saturating_sub(sql::unsigned(built_at_ms));
        sqlx::query(sql::FINISH_UPDATE)
            .bind(&self.tenant)
            .bind(id.0)
            .bind(status.as_str())
            .bind(truncate_chars(reason, MAX_REASON_LEN))
            .bind(sql::ms(duration))
            .execute(&mut *tx)
            .await
            .map_err(map_db)?;
        tx.commit().await.map_err(map_db)?;
        Ok(())
    }

    async fn snapshot_find(
        &self,
        repo: RepoId,
        commit_sha: &str,
    ) -> RepoGraphResult<Option<Snapshot>> {
        let row = sqlx::query(sql::SNAPSHOT_FIND)
            .bind(&self.tenant)
            .bind(repo.0)
            .bind(commit_sha)
            .fetch_optional(&self.pool)
            .await
            .map_err(map_db)?;
        row.as_ref().map(row_to_snapshot).transpose()
    }

    async fn snapshot_latest(&self, repo: RepoId) -> RepoGraphResult<Option<Snapshot>> {
        let row = sqlx::query(sql::SNAPSHOT_LATEST)
            .bind(&self.tenant)
            .bind(repo.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(map_db)?;
        row.as_ref().map(row_to_snapshot).transpose()
    }

    async fn snapshots(&self, repo: RepoId, limit: usize) -> RepoGraphResult<Vec<Snapshot>> {
        let limit = limit.clamp(1, MAX_SNAPSHOT_LIST);
        sqlx::query(sql::SNAPSHOTS)
            .bind(&self.tenant)
            .bind(repo.0)
            .bind(lim(limit))
            .fetch_all(&self.pool)
            .await
            .map_err(map_db)?
            .iter()
            .map(row_to_snapshot)
            .collect()
    }

    async fn snapshot_delete_older_than(
        &self,
        repo: RepoId,
        keep: usize,
    ) -> RepoGraphResult<usize> {
        let keep = sql::clamp_keep(keep);
        let mut tx = self.pool.begin().await.map_err(map_db)?;
        let retained: Vec<i64> = sqlx::query_scalar(sql::RETENTION_READY)
            .bind(&self.tenant)
            .bind(repo.0)
            .bind(lim(keep))
            .fetch_all(&mut *tx)
            .await
            .map_err(map_db)?;
        let deleted = sqlx::query(sql::RETENTION_DELETE)
            .bind(&self.tenant)
            .bind(repo.0)
            .bind(&retained)
            .fetch_all(&mut *tx)
            .await
            .map_err(map_db)?
            .len();
        sqlx::query(sql::SWEEP_BODIES)
            .bind(&self.tenant)
            .bind(repo.0)
            .execute(&mut *tx)
            .await
            .map_err(map_db)?;
        tx.commit().await.map_err(map_db)?;
        Ok(deleted)
    }

    async fn snapshot_diff(&self, a: SnapshotId, b: SnapshotId) -> RepoGraphResult<GraphDiff> {
        // Both snapshots must exist under one repo of this tenant, else `NotFound`.
        let repo_a: Option<i64> = sqlx::query_scalar(sql::SNAPSHOT_REPO)
            .bind(&self.tenant)
            .bind(a.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(map_db)?;
        let repo_b: Option<i64> = sqlx::query_scalar(sql::SNAPSHOT_REPO)
            .bind(&self.tenant)
            .bind(b.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(map_db)?;
        let (repo_a, repo_b) = match (repo_a, repo_b) {
            (Some(x), Some(y)) => (x, y),
            _ => return Err(RepoGraphError::NotFound),
        };
        if repo_a != repo_b {
            return Err(RepoGraphError::NotFound);
        }
        let repo = repo_a;
        let ha = self.diff_versions(repo, a).await?;
        let hb = self.diff_versions(repo, b).await?;
        let ea = self.diff_edges(repo, a).await?;
        let eb = self.diff_edges(repo, b).await?;

        let mut diff = GraphDiff::default();
        for (key, (sig_b, body_b)) in &hb {
            match ha.get(key) {
                None => diff.added.push(key.clone()),
                Some((sig_a, body_a)) => {
                    if sig_a != sig_b {
                        diff.sig_changed.push(key.clone());
                    }
                    if body_a != body_b {
                        diff.body_changed.push(key.clone());
                    }
                }
            }
        }
        for key in ha.keys() {
            if !hb.contains_key(key) {
                diff.removed.push(key.clone());
            }
        }
        for e in &eb {
            if !ea.contains(e) {
                diff.edges_added.push(e.clone());
            }
        }
        for e in &ea {
            if !eb.contains(e) {
                diff.edges_removed.push(e.clone());
            }
        }
        diff.added.sort();
        diff.removed.sort();
        diff.sig_changed.sort();
        diff.body_changed.sort();
        sort_edge_refs(&mut diff.edges_added);
        sort_edge_refs(&mut diff.edges_removed);
        cap_vec(&mut diff.added, &mut diff.truncated);
        cap_vec(&mut diff.removed, &mut diff.truncated);
        cap_vec(&mut diff.sig_changed, &mut diff.truncated);
        cap_vec(&mut diff.body_changed, &mut diff.truncated);
        cap_vec(&mut diff.edges_added, &mut diff.truncated);
        cap_vec(&mut diff.edges_removed, &mut diff.truncated);
        Ok(diff)
    }

    // -- Reads -------------------------------------------------------------

    async fn nodes_by_key(&self, scope: Scope, keys: &[NodeKey]) -> RepoGraphResult<Vec<NodeRow>> {
        self.scope_ok(scope).await?;
        let keys: Vec<String> = keys
            .iter()
            .take(MAX_KEYS)
            .map(|k| k.as_str().to_string())
            .collect();
        sqlx::query(sql::NODES_BY_KEY)
            .bind(&self.tenant)
            .bind(scope.repo.0)
            .bind(scope.snapshot.0)
            .bind(&keys)
            .fetch_all(&self.pool)
            .await
            .map_err(map_db)?
            .iter()
            .map(row_to_noderow)
            .collect()
    }

    async fn nodes_by_file(&self, scope: Scope, files: &[String]) -> RepoGraphResult<Vec<NodeRow>> {
        self.scope_ok(scope).await?;
        let files: Vec<String> = files.iter().take(MAX_KEYS).cloned().collect();
        sqlx::query(sql::NODES_BY_FILE)
            .bind(&self.tenant)
            .bind(scope.repo.0)
            .bind(scope.snapshot.0)
            .bind(&files)
            .fetch_all(&self.pool)
            .await
            .map_err(map_db)?
            .iter()
            .map(row_to_noderow)
            .collect()
    }

    async fn nodes_by_name(
        &self,
        scope: Scope,
        name: &str,
        kind: Option<NodeKind>,
        limit: usize,
    ) -> RepoGraphResult<Vec<NodeRow>> {
        self.scope_ok(scope).await?;
        // A name that cannot be a symbol returns empty, not an error, and never reaches SQL.
        if name.len() > 256 || name.bytes().any(|b| b.is_ascii_whitespace()) {
            return Ok(Vec::new());
        }
        let limit = sql::clamp_limit(limit);
        sqlx::query(sql::NODES_BY_NAME)
            .bind(&self.tenant)
            .bind(scope.repo.0)
            .bind(scope.snapshot.0)
            .bind(name)
            .bind(kind.map(NodeKind::as_str))
            .bind(lim(limit))
            .fetch_all(&self.pool)
            .await
            .map_err(map_db)?
            .iter()
            .map(row_to_noderow)
            .collect()
    }

    async fn neighbors(
        &self,
        scope: Scope,
        seeds: &[NodeId],
        kind: EdgeKind,
        dir: Direction,
        hops: u32,
        cap: usize,
    ) -> RepoGraphResult<Vec<Neighbor>> {
        self.scope_ok(scope).await?;
        let hops = sql::clamp_hops(hops, sql::NEIGHBOR_HOPS);
        let cap = sql::clamp_cap(cap);
        let seeds: Vec<i64> = seeds.iter().map(|n| n.0).collect();
        let stmt = match dir {
            Direction::In => sql::NEIGHBORS_IN,
            Direction::Out => sql::NEIGHBORS_OUT,
        };
        sqlx::query(stmt)
            .bind(&self.tenant)
            .bind(scope.repo.0)
            .bind(scope.snapshot.0)
            .bind(&seeds)
            .bind(kind.as_str())
            .bind(hops as i32)
            .bind(lim(cap))
            .fetch_all(&self.pool)
            .await
            .map_err(map_db)?
            .iter()
            .map(|row| {
                let depth: i32 = col(row, "depth")?;
                Ok(Neighbor {
                    row: row_to_noderow(row)?,
                    depth: u8::try_from(depth).unwrap_or(u8::MAX),
                })
            })
            .collect()
    }

    async fn blast_radius(
        &self,
        scope: Scope,
        files: &[String],
        hops: u32,
        cap: usize,
    ) -> RepoGraphResult<Vec<String>> {
        self.scope_ok(scope).await?;
        let hops = sql::clamp_hops(hops, sql::RADIUS_HOPS);
        let cap = sql::clamp_cap(cap);
        let files: Vec<String> = files.iter().take(MAX_KEYS).cloned().collect();
        sqlx::query(sql::BLAST_RADIUS)
            .bind(&self.tenant)
            .bind(scope.repo.0)
            .bind(scope.snapshot.0)
            .bind(&files)
            .bind(hops as i32)
            .bind(lim(cap))
            .fetch_all(&self.pool)
            .await
            .map_err(map_db)?
            .iter()
            .map(|row| col::<String>(row, "file"))
            .collect()
    }

    async fn tests_covering(
        &self,
        scope: Scope,
        seeds: &[NodeId],
        hops: u32,
        cap: usize,
    ) -> RepoGraphResult<Vec<TestHit>> {
        self.scope_ok(scope).await?;
        let hops = sql::clamp_hops(hops, sql::RADIUS_HOPS);
        let cap = sql::clamp_cap(cap);
        let seeds: Vec<i64> = seeds.iter().map(|n| n.0).collect();
        let mut hits: Vec<TestHit> = sqlx::query(sql::TESTS_COVERING)
            .bind(&self.tenant)
            .bind(scope.repo.0)
            .bind(scope.snapshot.0)
            .bind(&seeds)
            .bind(hops as i32)
            .fetch_all(&self.pool)
            .await
            .map_err(map_db)?
            .iter()
            .map(|row| {
                let via: Option<String> = col(row, "via")?;
                Ok(TestHit {
                    row: row_to_noderow(row)?,
                    via: via.unwrap_or_default(),
                })
            })
            .collect::<RepoGraphResult<Vec<_>>>()?;
        hits.sort_by(|a, b| a.row.node.key.cmp(&b.row.node.key));
        hits.truncate(cap);
        Ok(hits)
    }

    async fn path_between(
        &self,
        scope: Scope,
        src: NodeId,
        dst: NodeId,
        max_hops: u32,
        max_paths: usize,
    ) -> RepoGraphResult<Vec<Vec<NodeKey>>> {
        self.scope_ok(scope).await?;
        let max_hops = sql::clamp_hops(max_hops, sql::PATH_HOPS);
        let max_paths = max_paths.clamp(1, MAX_PATHS);
        let id_paths: Vec<Vec<i64>> = sqlx::query(sql::PATH_BETWEEN)
            .bind(&self.tenant)
            .bind(scope.repo.0)
            .bind(scope.snapshot.0)
            .bind(src.0)
            .bind(dst.0)
            .bind(max_hops as i32)
            .bind(lim(max_paths))
            .fetch_all(&self.pool)
            .await
            .map_err(map_db)?
            .iter()
            .map(|row| col::<Vec<i64>>(row, "path"))
            .collect::<RepoGraphResult<Vec<_>>>()?;

        // Map the id paths back to keys. An id whose key does not resolve drops its path.
        let ids: Vec<i64> = id_paths
            .iter()
            .flatten()
            .copied()
            .collect::<BTreeSet<i64>>()
            .into_iter()
            .collect();
        let mut by_id: HashMap<i64, NodeKey> = HashMap::new();
        for row in &sqlx::query(sql::KEYS_FOR_IDS)
            .bind(&self.tenant)
            .bind(scope.repo.0)
            .bind(&ids)
            .fetch_all(&self.pool)
            .await
            .map_err(map_db)?
        {
            let id: i64 = col(row, "node_id")?;
            let key_text: String = col(row, "node_key")?;
            if let Ok(key) = NodeKey::parse(&key_text) {
                by_id.insert(id, key);
            }
        }
        let mut out: Vec<Vec<NodeKey>> = Vec::new();
        for path in id_paths {
            let mut keys = Vec::with_capacity(path.len());
            let mut ok = true;
            for id in path {
                match by_id.get(&id) {
                    Some(key) => keys.push(key.clone()),
                    None => {
                        ok = false;
                        break;
                    }
                }
            }
            if ok {
                out.push(keys);
            }
        }
        Ok(out)
    }

    async fn shape(&self, scope: Scope) -> RepoGraphResult<Shape> {
        let meta = sqlx::query(sql::SNAPSHOT_GET)
            .bind(&self.tenant)
            .bind(scope.repo.0)
            .bind(scope.snapshot.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(map_db)?
            .ok_or(RepoGraphError::NotFound)?;
        let snapshot = row_to_snapshot(&meta)?;

        let mut nodes_by_kind: BTreeMap<NodeKind, usize> = BTreeMap::new();
        for row in &sqlx::query(sql::SHAPE_NODES_BY_KIND)
            .bind(&self.tenant)
            .bind(scope.repo.0)
            .bind(scope.snapshot.0)
            .fetch_all(&self.pool)
            .await
            .map_err(map_db)?
        {
            let kind_text: String = col(row, "kind")?;
            let kind =
                NodeKind::parse(&kind_text).ok_or_else(|| backend("column kind: unknown value"))?;
            let c: i64 = col(row, "c")?;
            nodes_by_kind.insert(kind, usize::try_from(c).unwrap_or(0));
        }
        let mut edges_by_kind: BTreeMap<EdgeKind, usize> = BTreeMap::new();
        for row in &sqlx::query(sql::SHAPE_EDGES_BY_KIND)
            .bind(&self.tenant)
            .bind(scope.repo.0)
            .bind(scope.snapshot.0)
            .fetch_all(&self.pool)
            .await
            .map_err(map_db)?
        {
            let kind_text: String = col(row, "kind")?;
            let kind =
                EdgeKind::parse(&kind_text).ok_or_else(|| backend("column kind: unknown value"))?;
            let c: i64 = col(row, "c")?;
            edges_by_kind.insert(kind, usize::try_from(c).unwrap_or(0));
        }
        let files: i64 = sqlx::query_scalar(sql::SHAPE_FILES)
            .bind(&self.tenant)
            .bind(scope.repo.0)
            .bind(scope.snapshot.0)
            .fetch_one(&self.pool)
            .await
            .map_err(map_db)?;
        let crates = nodes_by_kind.get(&NodeKind::Crate).copied().unwrap_or(0);
        Ok(Shape {
            snapshot,
            nodes_by_kind,
            edges_by_kind,
            files: usize::try_from(files).unwrap_or(0),
            crates,
        })
    }
}

impl PgRepoGraph {
    /// `(node_key → (sig_hash, body_hash))` for a snapshot's versions (a `snapshot_diff` half).
    async fn diff_versions(
        &self,
        repo: i64,
        snap: SnapshotId,
    ) -> RepoGraphResult<BTreeMap<NodeKey, (String, String)>> {
        let mut out = BTreeMap::new();
        for row in &sqlx::query(sql::DIFF_VERSIONS)
            .bind(&self.tenant)
            .bind(repo)
            .bind(snap.0)
            .fetch_all(&self.pool)
            .await
            .map_err(map_db)?
        {
            let key_text: String = col(row, "node_key")?;
            let key = NodeKey::parse(&key_text).map_err(|_| backend("column node_key: invalid"))?;
            out.insert(key, (col(row, "sig_hash")?, col(row, "body_hash")?));
        }
        Ok(out)
    }

    /// The edge set (by `(kind, src_key, dst_key)`) of a snapshot (a `snapshot_diff` half).
    async fn diff_edges(&self, repo: i64, snap: SnapshotId) -> RepoGraphResult<BTreeSet<EdgeRef>> {
        let mut out = BTreeSet::new();
        for row in &sqlx::query(sql::DIFF_EDGES)
            .bind(&self.tenant)
            .bind(repo)
            .bind(snap.0)
            .fetch_all(&self.pool)
            .await
            .map_err(map_db)?
        {
            let kind_text: String = col(row, "kind")?;
            let src_text: String = col(row, "src_key")?;
            let dst_text: String = col(row, "dst_key")?;
            out.insert(EdgeRef {
                kind: EdgeKind::parse(&kind_text)
                    .ok_or_else(|| backend("column kind: unknown value"))?,
                src: NodeKey::parse(&src_text).map_err(|_| backend("column src_key: invalid"))?,
                dst: NodeKey::parse(&dst_text).map_err(|_| backend("column dst_key: invalid"))?,
            });
        }
        Ok(out)
    }
}

/// Sort `EdgeRef`s by `(kind, src, dst)` (matching `MemRepoGraph`).
fn sort_edge_refs(v: &mut [EdgeRef]) {
    v.sort_by(|a, b| {
        a.kind
            .as_str()
            .cmp(b.kind.as_str())
            .then(a.src.cmp(&b.src))
            .then(a.dst.cmp(&b.dst))
    });
}

/// Cap a diff list at [`MAX_DIFF`], setting `truncated` when it bit.
fn cap_vec<T>(v: &mut Vec<T>, truncated: &mut bool) {
    if v.len() > MAX_DIFF {
        v.truncate(MAX_DIFF);
        *truncated = true;
    }
}

#[cfg(test)]
mod tests;
