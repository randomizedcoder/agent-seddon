//! [`PgCampaigns`] — the Postgres `CampaignStore`, behind the non-default
//! `campaign-postgres` feature. The same contract as `agent_testkit::campaign::MemCampaigns`
//! (the conformance suite runs unchanged over both) on a real server via `sqlx` (pure
//! Rust, rustls, no `libpq`): one `tasks` tree table plus `task_events` and
//! `task_attempts` (`docs/design/campaigns/01-schema.md`), every protocol one
//! transaction at READ COMMITTED (`02-transactions.md`).
//!
//! Unlike the memory tier this one is **not hermetic** — it needs a live server — so its
//! tests are `#[ignore]`-gated and run only via `nix run .#pg-integration`, never inside
//! `nix flake check`. The crate still *compiles* under this feature with no database:
//! every statement is runtime-checked (`sqlx::query`, never the compile-time `query!`).
//!
//! Shape, mirrored from the memory tier so the two cannot drift:
//!
//! * a **tenant-bound handle** ([`PgCampaigns::with_tenant`] fails closed on anything
//!   that is not a `safe_segment`, before any statement is issued); every statement
//!   binds the tenant, so a foreign tenant's id is simply `NotFound`;
//! * an **injectable epoch-ms clock** ([`PgCampaigns::with_clock`]) bound as `$now`
//!   wherever the design says `now()`, so lease / reap rows run without sleeping;
//! * **one state write** (`Tx::transition`): `allowed()` → `Denied`, a CAS `UPDATE` on
//!   the version, one `task_events` row carrying the new version — or nothing;
//! * **lock order** (`02-transactions.md`): protocols that roll up lock the ancestors
//!   root → parent *before* the node; subtrees lock in `(depth, path)` order; claim and
//!   reap use `FOR UPDATE SKIP LOCKED` and never block.
//!
//! Every statement lives in `sql.rs`; this file never builds SQL from strings.
//!
//! Schema is applied by a small **versioned** runner ([`PgCampaigns::run_migrations`]) over
//! the embedded [`MIGRATIONS`] set, exactly like the digest and config-store tiers. We
//! deliberately do **not** use `sqlx::migrate!`: its `macros` feature pulls in every sqlx
//! driver, and `sqlx-mysql` drags in `rsa` (RUSTSEC-2023-0071, rejected by `cargo audit`).

mod sql;

use agent_core::campaign::{
    allowed, check_len, check_list, check_max, clamp_lease, rollup, screen, truncate_chars, Actor,
    ActorClass, AttemptId, AttemptKind, AttemptOutcome, BlockReason, CampaignError, CampaignResult,
    CampaignStore, ChildSpec, ClaimRequest, Claimed, Complete, Decomposed, Decomposition, EstSize,
    EventId, Fail, IdemKey, ListFilter, MarkLeaf, NewCampaign, Owner, PlanAttempt, PlanClose,
    PlanCloseOutcome, PlanStart, Policy, Reaped, ReviewOutcome, Task, TaskAttempt, TaskEvent,
    TaskId, TaskKind, TaskPath, TaskState, CLARIFICATION_HEADER, MAX_ACCEPTANCE,
    MAX_ACCEPTANCE_ITEM, MAX_ANSWER, MAX_CHILDREN, MAX_ERROR, MAX_GOAL, MAX_QUESTION, MAX_REASON,
    MAX_SESSION_ID, MAX_TOUCH, MAX_TOUCHES,
};
use agent_core::{safe_segment, scan_for_injection, UserId};
use async_trait::async_trait;
use serde_json::{json, Value};
use sqlx::error::ErrorKind;
use sqlx::postgres::{PgArguments, PgPoolOptions, PgRow};
use sqlx::{PgExecutor, PgPool, Postgres, Row, Transaction};
use std::collections::HashSet;
use std::sync::Arc;

/// The embedded migrations, in apply order: `(version, sql)`. A version is applied
/// exactly once and recorded in the `_campaign_migrations` ledger. Adding a migration =
/// drop the next-numbered `.sql` in `migrations/` and append its `(n, include_str!(...))`
/// here (the version is the source of truth, not the filename).
const MIGRATIONS: &[(i64, &str)] = &[(1, include_str!("../migrations/0001_campaigns.sql"))];

/// The transaction-scoped advisory lock that serializes concurrent starters through
/// [`PgCampaigns::run_migrations`]: `"agcampgn"` folded into an `i64`, distinct from the
/// digest (`"agdigest"`) and config-store (`"agconfgs"`) keys so the runners never contend.
const MIGRATION_LOCK_KEY: i64 = 0x6167_6361_6d70_676e_u64 as i64;

/// A Postgres-backed campaign store: a pool, the tenant it is bound to, and the clock.
#[derive(Clone)]
pub struct PgCampaigns {
    pool: PgPool,
    tenant: String,
    now_ms: Arc<dyn Fn() -> u64 + Send + Sync>,
}

impl std::fmt::Debug for PgCampaigns {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PgCampaigns")
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

/// The typed error for a driver error. Constraint violations are mapped **by constraint
/// name and kind**, never by message text; nothing from the row is echoed.
fn map_db(e: sqlx::Error) -> CampaignError {
    match &e {
        sqlx::Error::Database(db) => {
            let name = db.constraint().unwrap_or("");
            match db.kind() {
                ErrorKind::UniqueViolation if name == "task_attempts_idem_key" => {
                    CampaignError::AlreadyApplied
                }
                ErrorKind::UniqueViolation => CampaignError::Conflict(format!("unique: {name}")),
                ErrorKind::ForeignKeyViolation
                | ErrorKind::NotNullViolation
                | ErrorKind::CheckViolation => {
                    CampaignError::Invalid(format!("constraint: {name}"))
                }
                _ => CampaignError::Backend(format!(
                    "campaign postgres: sqlstate {}",
                    db.code()
                        .map(std::borrow::Cow::into_owned)
                        .unwrap_or_default()
                )),
            }
        }
        sqlx::Error::RowNotFound => CampaignError::NotFound,
        _ => CampaignError::Backend(format!("campaign postgres: {e}")),
    }
}

fn backend(what: &str) -> CampaignError {
    CampaignError::Backend(format!("campaign postgres: {what}"))
}

/// Epoch milliseconds as the `BIGINT` bind.
fn ms(now: u64) -> i64 {
    i64::try_from(now).unwrap_or(i64::MAX)
}

fn ver(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

fn tok(t: u64) -> i64 {
    i64::try_from(t).unwrap_or(i64::MAX)
}

fn unsigned(v: i64) -> u64 {
    u64::try_from(v).unwrap_or(0)
}

fn limit_bind(limit: usize) -> i64 {
    i64::try_from(limit).unwrap_or(i64::MAX)
}

fn col<'r, T>(row: &'r PgRow, name: &str) -> CampaignResult<T>
where
    T: sqlx::Decode<'r, Postgres> + sqlx::Type<Postgres>,
{
    row.try_get(name)
        .map_err(|e| backend(&format!("column {name}: {e}")))
}

fn parse_col<T>(name: &str, text: &str, parse: impl Fn(&str) -> Option<T>) -> CampaignResult<T> {
    parse(text).ok_or_else(|| backend(&format!("column {name}: unknown value")))
}

fn json_list(name: &str, text: &str) -> CampaignResult<Vec<String>> {
    serde_json::from_str(text).map_err(|e| backend(&format!("column {name}: {e}")))
}

/// Decode one `tasks` row, treating the store as untrusted: an unknown enum text, a
/// path that fails the grammar or an owner that fails `safe_segment` is a `Backend`
/// fault, never a panic.
fn row_to_task(row: &PgRow) -> CampaignResult<Task> {
    let kind: String = col(row, "kind")?;
    let state: String = col(row, "state")?;
    let path: String = col(row, "path")?;
    let acceptance: String = col(row, "acceptance")?;
    let touches: String = col(row, "touches")?;
    let depends_on: Vec<i64> = col(row, "depends_on")?;
    let est_size: Option<String> = col(row, "est_size")?;
    let policy: Option<String> = col(row, "policy")?;
    let claimed_by: Option<String> = col(row, "claimed_by")?;
    let depth: i16 = col(row, "depth")?;
    let ordinal: i16 = col(row, "ordinal")?;
    let attempts: i16 = col(row, "attempts")?;
    Ok(Task {
        task_id: TaskId(col(row, "task_id")?),
        campaign_id: TaskId(col(row, "campaign_id")?),
        repo_id: col(row, "repo_id")?,
        parent_id: col::<Option<i64>>(row, "parent_id")?.map(TaskId),
        path: TaskPath::parse(&path).map_err(|e| backend(&format!("column path: {e}")))?,
        depth: u8::try_from(depth).map_err(|_| backend("column depth: out of range"))?,
        ordinal: u8::try_from(ordinal).map_err(|_| backend("column ordinal: out of range"))?,
        kind: parse_col("kind", &kind, TaskKind::parse)?,
        state: parse_col("state", &state, TaskState::parse)?,
        title: col(row, "title")?,
        goal: col(row, "goal")?,
        acceptance: json_list("acceptance", &acceptance)?,
        touches: json_list("touches", &touches)?,
        depends_on: depends_on.into_iter().map(TaskId).collect(),
        est_size: est_size
            .map(|s| parse_col("est_size", &s, EstSize::parse))
            .transpose()?,
        source_ref: col(row, "source_ref")?,
        policy: policy.map(|p| Policy::from_stored(&p)).transpose()?,
        version: unsigned(col(row, "version")?),
        attempts: u16::try_from(attempts).map_err(|_| backend("column attempts: out of range"))?,
        claimed_by: claimed_by
            .map(|o| Owner::parse(&o).map_err(|e| backend(&format!("column claimed_by: {e}"))))
            .transpose()?,
        lease_until_ms: col::<Option<i64>>(row, "lease_until_ms")?.map(unsigned),
        pr_number: col(row, "pr_number")?,
        pr_url: col(row, "pr_url")?,
        branch: col(row, "branch")?,
        superseded_by: col::<Option<i64>>(row, "superseded_by")?.map(TaskId),
        created_by: col(row, "created_by")?,
        created_at_ms: unsigned(col(row, "created_at_ms")?),
        updated_at_ms: unsigned(col(row, "updated_at_ms")?),
    })
}

fn row_to_event(row: &PgRow) -> CampaignResult<TaskEvent> {
    let from: Option<String> = col(row, "from_state")?;
    let to: String = col(row, "to_state")?;
    let detail: String = col(row, "detail")?;
    Ok(TaskEvent {
        event_id: EventId(col(row, "event_id")?),
        task_id: TaskId(col(row, "task_id")?),
        from_state: from
            .map(|s| parse_col("from_state", &s, TaskState::parse))
            .transpose()?,
        to_state: parse_col("to_state", &to, TaskState::parse)?,
        actor: col(row, "actor")?,
        version: unsigned(col(row, "version")?),
        detail: serde_json::from_str(&detail)
            .map_err(|e| backend(&format!("column detail: {e}")))?,
        at_ms: unsigned(col(row, "at_ms")?),
    })
}

fn row_to_attempt(row: &PgRow) -> CampaignResult<TaskAttempt> {
    let kind: String = col(row, "kind")?;
    let idem: String = col(row, "idem_key")?;
    let owner: Option<String> = col(row, "owner")?;
    let outcome: String = col(row, "outcome")?;
    Ok(TaskAttempt {
        attempt_id: AttemptId(col(row, "attempt_id")?),
        task_id: TaskId(col(row, "task_id")?),
        kind: parse_col("kind", &kind, AttemptKind::parse)?,
        idem_key: IdemKey::parse(&idem).map_err(|e| backend(&format!("column idem_key: {e}")))?,
        prompt_hash: col(row, "prompt_hash")?,
        model: col(row, "model")?,
        tokens_in: unsigned(col(row, "tokens_in")?),
        tokens_out: unsigned(col(row, "tokens_out")?),
        session_id: col(row, "session_id")?,
        owner: owner
            .map(|o| Owner::parse(&o).map_err(|e| backend(&format!("column owner: {e}"))))
            .transpose()?,
        outcome: parse_col("outcome", &outcome, AttemptOutcome::parse)?,
        pr_url: col(row, "pr_url")?,
        error: col(row, "error")?,
        started_at_ms: unsigned(col(row, "started_at_ms")?),
        ended_at_ms: col::<Option<i64>>(row, "ended_at_ms")?.map(unsigned),
    })
}

type Query<'q> = sqlx::query::Query<'q, Postgres, PgArguments>;

async fn fetch_tasks<'e, E: PgExecutor<'e>>(ex: E, q: Query<'_>) -> CampaignResult<Vec<Task>> {
    q.fetch_all(ex)
        .await
        .map_err(map_db)?
        .iter()
        .map(row_to_task)
        .collect()
}

async fn fetch_task<'e, E: PgExecutor<'e>>(ex: E, q: Query<'_>) -> CampaignResult<Option<Task>> {
    q.fetch_optional(ex)
        .await
        .map_err(map_db)?
        .as_ref()
        .map(row_to_task)
        .transpose()
}

/// Re-read one row after a column patch (`RETURNING` the task columns).
async fn patched<'e, E: PgExecutor<'e>>(ex: E, q: Query<'_>) -> CampaignResult<Task> {
    fetch_task(ex, q).await?.ok_or(CampaignError::NotFound)
}

impl PgCampaigns {
    /// Connect a pool to `dsn` (max `pool_max` connections, clamped to ≥1) bound to the
    /// `local` tenant and, when `migrate_on_start`, apply the embedded migrations. The
    /// DSN is never echoed on error (it carries a password).
    pub async fn connect(dsn: &str, pool_max: u32, migrate_on_start: bool) -> CampaignResult<Self> {
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

    /// A lazily-connecting pool (the DSN is validated now, connections open on first
    /// use); the schema is **not** applied here. The DSN is never echoed on error.
    pub fn connect_lazy(dsn: &str, pool_max: u32) -> CampaignResult<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(pool_max.max(1))
            .connect_lazy(dsn)
            .map_err(|_| backend("invalid DSN (could not parse)"))?;
        Ok(Self::from_pool(pool))
    }

    /// A store over an already-established pool, bound to the `local` tenant, on the
    /// wall clock.
    pub fn from_pool(pool: PgPool) -> Self {
        Self {
            pool,
            tenant: UserId::LOCAL.to_string(),
            now_ms: Arc::new(wall_clock_ms),
        }
    }

    /// The same pool and clock under `tenant`; refuses anything that is not a
    /// [`safe_segment`] (traversal, empty, over-length) **before any statement is issued**.
    pub fn with_tenant(&self, tenant: &str) -> CampaignResult<Self> {
        if !safe_segment(tenant) {
            return Err(CampaignError::Invalid(
                "tenant: must be a non-empty path-safe segment".to_string(),
            ));
        }
        Ok(Self {
            pool: self.pool.clone(),
            tenant: tenant.to_string(),
            now_ms: Arc::clone(&self.now_ms),
        })
    }

    /// Replace the clock (epoch milliseconds). Tests drive leases and reaps with it.
    #[doc(hidden)]
    pub fn with_clock(mut self, now_ms: Arc<dyn Fn() -> u64 + Send + Sync>) -> Self {
        self.now_ms = now_ms;
        self
    }

    /// Apply the embedded [`MIGRATIONS`] set exactly once, in order, under a
    /// transaction-scoped advisory lock (concurrent starters serialize; the second sees
    /// a full ledger and no-ops). DDL is transactional, so a step and its ledger row
    /// commit or roll back together.
    pub async fn run_migrations(pool: &PgPool) -> CampaignResult<()> {
        let mut tx = pool.begin().await.map_err(map_db)?;
        sqlx::query(sql::ADVISORY_LOCK)
            .bind(MIGRATION_LOCK_KEY)
            .execute(&mut *tx)
            .await
            .map_err(map_db)?;
        sqlx::query(sql::CREATE_LEDGER)
            .execute(&mut *tx)
            .await
            .map_err(map_db)?;
        let applied: HashSet<i64> = sqlx::query_scalar::<_, i64>(sql::SELECT_APPLIED)
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
            sqlx::query(sql::INSERT_APPLIED)
                .bind(version)
                .execute(&mut *tx)
                .await
                .map_err(map_db)?;
        }
        tx.commit().await.map_err(map_db)
    }

    async fn begin(&self) -> CampaignResult<Tx> {
        let conn = self.pool.begin().await.map_err(map_db)?;
        Ok(Tx {
            conn,
            tenant: self.tenant.clone(),
            now: (self.now_ms)(),
        })
    }

    async fn peek(&self, id: TaskId) -> CampaignResult<Task> {
        fetch_task(
            &self.pool,
            sqlx::query(sql::GET).bind(&self.tenant).bind(id.0),
        )
        .await?
        .ok_or(CampaignError::NotFound)
    }
}

/// Fields a closing `work` attempt receives; `None` keeps the stored value.
#[derive(Default)]
struct WorkPatch {
    pr_url: Option<String>,
    error: Option<String>,
    tokens: Option<(u64, u64)>,
    session_id: Option<String>,
}

/// One protocol's transaction: the connection, the tenant and the transaction's `now`.
struct Tx {
    conn: Transaction<'static, Postgres>,
    tenant: String,
    now: u64,
}

impl Tx {
    async fn commit(self) -> CampaignResult<()> {
        self.conn.commit().await.map_err(map_db)
    }

    /// The row without a lock (a `NotFound` probe before the lock order starts).
    async fn peek(&mut self, id: TaskId) -> CampaignResult<Task> {
        fetch_task(
            &mut *self.conn,
            sqlx::query(sql::GET).bind(&self.tenant).bind(id.0),
        )
        .await?
        .ok_or(CampaignError::NotFound)
    }

    /// The row under `FOR UPDATE` (re-locking a row this transaction holds is a no-op).
    async fn lock(&mut self, id: TaskId) -> CampaignResult<Task> {
        fetch_task(
            &mut *self.conn,
            sqlx::query(sql::LOCK).bind(&self.tenant).bind(id.0),
        )
        .await?
        .ok_or(CampaignError::NotFound)
    }

    /// Lock every proper ancestor of `t`, root first (the fixed order every rolling-up
    /// protocol takes before it touches the node).
    async fn lock_ancestors(&mut self, t: &Task) -> CampaignResult<()> {
        sqlx::query(sql::LOCK_ANCESTORS)
            .bind(&self.tenant)
            .bind(t.campaign_id.0)
            .bind(t.path.as_str())
            .execute(&mut *self.conn)
            .await
            .map_err(map_db)?;
        Ok(())
    }

    /// Direct children of `parent` by ordinal (every state, superseded included).
    async fn children_of(&mut self, parent: TaskId) -> CampaignResult<Vec<Task>> {
        fetch_tasks(
            &mut *self.conn,
            sqlx::query(sql::CHILDREN).bind(&self.tenant).bind(parent.0),
        )
        .await
    }

    /// `node` and every descendant, `(depth, path)` order, locked when `lock`.
    async fn subtree_of(&mut self, node: &Task, lock: bool) -> CampaignResult<Vec<Task>> {
        let q = if lock {
            sql::LOCK_SUBTREE
        } else {
            sql::SUBTREE
        };
        fetch_tasks(
            &mut *self.conn,
            sqlx::query(q)
                .bind(&self.tenant)
                .bind(node.campaign_id.0)
                .bind(node.task_id.0)
                .bind(node.path.subtree_like()),
        )
        .await
    }

    async fn policy_of(&mut self, campaign_id: TaskId) -> CampaignResult<Policy> {
        let row = sqlx::query(sql::POLICY)
            .bind(&self.tenant)
            .bind(campaign_id.0)
            .fetch_optional(&mut *self.conn)
            .await
            .map_err(map_db)?
            .ok_or(CampaignError::NotFound)?;
        let text: Option<String> = col(&row, "policy")?;
        text.map(|p| Policy::from_stored(&p))
            .transpose()
            .map(Option::unwrap_or_default)
    }

    async fn campaign_nodes(&mut self, campaign_id: TaskId) -> CampaignResult<i64> {
        sqlx::query_scalar::<_, i64>(sql::CAMPAIGN_NODES)
            .bind(&self.tenant)
            .bind(campaign_id.0)
            .fetch_one(&mut *self.conn)
            .await
            .map_err(map_db)
    }

    async fn campaign_plan_tokens(&mut self, campaign_id: TaskId) -> CampaignResult<u64> {
        sqlx::query_scalar::<_, i64>(sql::PLAN_TOKENS)
            .bind(&self.tenant)
            .bind(campaign_id.0)
            .fetch_one(&mut *self.conn)
            .await
            .map(unsigned)
            .map_err(map_db)
    }

    async fn idem_exists(&mut self, key: &IdemKey) -> CampaignResult<bool> {
        sqlx::query(sql::IDEM_EXISTS)
            .bind(&self.tenant)
            .bind(key.as_str())
            .fetch_optional(&mut *self.conn)
            .await
            .map(|r| r.is_some())
            .map_err(map_db)
    }

    async fn event(
        &mut self,
        task_id: TaskId,
        from: Option<TaskState>,
        to: TaskState,
        actor: &Actor,
        version: u64,
        detail: Value,
    ) -> CampaignResult<()> {
        let now = ms(self.now);
        sqlx::query(sql::INSERT_EVENT)
            .bind(&self.tenant)
            .bind(task_id.0)
            .bind(from.map(TaskState::as_str))
            .bind(to.as_str())
            .bind(actor.render())
            .bind(ver(version))
            .bind(detail.to_string())
            .bind(now)
            .execute(&mut *self.conn)
            .await
            .map_err(map_db)?;
        Ok(())
    }

    /// The one state write: `allowed()` → `Denied`; CAS on the version; the lease
    /// cleared unless the new state is leased; one event carrying the new version.
    async fn transition(
        &mut self,
        id: TaskId,
        to: TaskState,
        actor: &Actor,
        detail: Value,
    ) -> CampaignResult<Task> {
        let t = self.lock(id).await?;
        if !allowed(t.state, to, t.kind, actor.class()) {
            return Err(CampaignError::Denied(format!(
                "{} → {} on a {} by {}",
                t.state.as_str(),
                to.as_str(),
                t.kind.as_str(),
                actor.class().as_str()
            )));
        }
        let now = ms(self.now);
        let task = fetch_task(
            &mut *self.conn,
            sqlx::query(sql::TRANSITION)
                .bind(&self.tenant)
                .bind(id.0)
                .bind(ver(t.version))
                .bind(to.as_str())
                .bind(now)
                .bind(to.is_leased()),
        )
        .await?
        .ok_or_else(|| CampaignError::Conflict("version: concurrent write".to_string()))?;
        self.event(id, Some(t.state), to, actor, task.version, detail)
            .await?;
        Ok(task)
    }

    #[allow(clippy::too_many_arguments)]
    async fn insert_attempt(
        &mut self,
        task_id: TaskId,
        kind: AttemptKind,
        idem_key: &IdemKey,
        owner: Option<&Owner>,
        outcome: AttemptOutcome,
        plan: Option<&PlanAttempt>,
        error: Option<&str>,
    ) -> CampaignResult<AttemptId> {
        let now = ms(self.now);
        let (tin, tout) = plan.map(|p| p.tokens.clamped()).unwrap_or_default();
        let closed = plan.is_some();
        let id = sqlx::query_scalar::<_, i64>(sql::INSERT_ATTEMPT)
            .bind(&self.tenant)
            .bind(task_id.0)
            .bind(kind.as_str())
            .bind(idem_key.as_str())
            .bind(plan.map_or("", |p| p.prompt_hash.as_str()))
            .bind(plan.map_or("", |p| p.model.as_str()))
            .bind(tok(tin))
            .bind(tok(tout))
            .bind(owner.map(Owner::as_str))
            .bind(outcome.as_str())
            .bind(error)
            .bind(now)
            .bind(closed)
            .fetch_one(&mut *self.conn)
            .await
            .map_err(map_db)?;
        Ok(AttemptId(id))
    }

    /// Insert the planner's finished attempt, closed with its outcome (`03-decomposition.md`
    /// step 1 as amended: inside the finishing transaction).
    async fn plan_attempt(
        &mut self,
        task_id: TaskId,
        attempt: &PlanAttempt,
        outcome: AttemptOutcome,
        error: Option<&str>,
    ) -> CampaignResult<AttemptId> {
        attempt.validate()?;
        self.insert_attempt(
            task_id,
            AttemptKind::Decompose,
            &attempt.idem_key,
            None,
            outcome,
            Some(attempt),
            error,
        )
        .await
    }

    /// Close every pending `work` attempt on `task_id` (optionally only `owner`'s).
    async fn close_work(
        &mut self,
        task_id: TaskId,
        owner: Option<&Owner>,
        outcome: AttemptOutcome,
        patch: WorkPatch,
    ) -> CampaignResult<()> {
        let now = ms(self.now);
        sqlx::query(sql::CLOSE_WORK)
            .bind(&self.tenant)
            .bind(task_id.0)
            .bind(outcome.as_str())
            .bind(now)
            .bind(patch.pr_url)
            .bind(patch.error)
            .bind(patch.tokens.map(|(i, _)| tok(i)))
            .bind(patch.tokens.map(|(_, o)| tok(o)))
            .bind(patch.session_id)
            .bind(owner.map(Owner::as_str))
            .execute(&mut *self.conn)
            .await
            .map_err(map_db)?;
        Ok(())
    }

    /// Block `ready` siblings that depend on `leaf` (`02-transactions.md` (d) step 4).
    async fn block_dependents(&mut self, leaf: &Task) -> CampaignResult<()> {
        let Some(parent) = leaf.parent_id else {
            return Ok(());
        };
        for sib in self.children_of(parent).await? {
            if sib.state == TaskState::Ready && sib.depends_on.contains(&leaf.task_id) {
                self.transition(
                    sib.task_id,
                    TaskState::Blocked,
                    &Actor::Rollup,
                    json!({"reason": BlockReason::DependencyFailed.as_str(), "dependency": leaf.task_id}),
                )
                .await?;
            }
        }
        Ok(())
    }

    /// The rollup pass, parent upward (the ancestors are already locked), stopping at
    /// the first unchanged ancestor.
    async fn rollup_from(&mut self, mut parent: Option<TaskId>) -> CampaignResult<()> {
        while let Some(id) = parent {
            let p = self.lock(id).await?;
            let states: Vec<TaskState> = self
                .children_of(id)
                .await?
                .iter()
                .map(|c| c.state)
                .collect();
            match rollup(p.state, &states) {
                Some(next) => {
                    self.transition(id, next, &Actor::Rollup, json!({})).await?;
                    parent = p.parent_id;
                }
                None => break,
            }
        }
        Ok(())
    }

    async fn set_attempts(&mut self, id: TaskId, attempts: u16) -> CampaignResult<()> {
        sqlx::query(sql::SET_ATTEMPTS)
            .bind(&self.tenant)
            .bind(id.0)
            .bind(i16::try_from(attempts).unwrap_or(i16::MAX))
            .execute(&mut *self.conn)
            .await
            .map_err(map_db)?;
        Ok(())
    }
}

fn cas(t: &Task, expected_version: u64, state: TaskState) -> CampaignResult<()> {
    if t.version != expected_version {
        return Err(CampaignError::Conflict(format!(
            "version: expected {expected_version}, found {}",
            t.version
        )));
    }
    require_state(t, state)
}

fn require_state(t: &Task, state: TaskState) -> CampaignResult<()> {
    if t.state != state {
        return Err(CampaignError::Conflict(format!(
            "state: expected {}, found {}",
            state.as_str(),
            t.state.as_str()
        )));
    }
    Ok(())
}

/// `children[i].depends_on` are batch ordinals: each in `1..=n`, not self, acyclic.
fn check_deps(children: &[ChildSpec]) -> CampaignResult<()> {
    let n = children.len();
    for (i, c) in children.iter().enumerate() {
        let me = i + 1;
        for d in &c.depends_on {
            let d = usize::from(*d);
            if d < 1 || d > n {
                return Err(CampaignError::Invalid(format!(
                    "children[{i}].depends_on: ordinal {d} is not in this batch"
                )));
            }
            if d == me {
                return Err(CampaignError::Invalid(format!(
                    "children[{i}].depends_on: depends on itself"
                )));
            }
        }
    }
    // Kahn: every node must drain.
    let mut indeg: Vec<usize> = children.iter().map(|c| c.depends_on.len()).collect();
    let mut ready: Vec<usize> = (0..n).filter(|i| indeg[*i] == 0).collect();
    let mut drained = 0;
    while let Some(i) = ready.pop() {
        drained += 1;
        for (j, c) in children.iter().enumerate() {
            if c.depends_on.contains(&(i as u8 + 1)) {
                indeg[j] -= 1;
                if indeg[j] == 0 {
                    ready.push(j);
                }
            }
        }
    }
    if drained != n {
        return Err(CampaignError::Invalid(
            "children: depends_on has a cycle".to_string(),
        ));
    }
    Ok(())
}

fn session_ok(s: &Option<String>) -> CampaignResult<()> {
    match s {
        Some(v) => check_len("session_id", v, MAX_SESSION_ID),
        None => Ok(()),
    }
}

fn json_text(items: &[String]) -> String {
    serde_json::to_string(items).unwrap_or_else(|_| "[]".to_string())
}

#[async_trait]
impl CampaignStore for PgCampaigns {
    fn tenant(&self) -> &str {
        &self.tenant
    }

    async fn create(&self, req: NewCampaign, actor: &Actor) -> CampaignResult<Task> {
        let principal = Actor::User(actor.human()?.clone());
        req.validate()?;
        let policy = req.policy.clone().unwrap_or_default();
        let state = if req.draft {
            TaskState::Draft
        } else {
            TaskState::Ready
        };
        let injection = scan_for_injection(&req.goal).is_some();
        let mut tx = self.begin().await?;
        let now = ms(tx.now);
        sqlx::query(sql::ENSURE_TENANT)
            .bind(&tx.tenant)
            .execute(&mut *tx.conn)
            .await
            .map_err(map_db)?;
        let task = fetch_task(
            &mut *tx.conn,
            sqlx::query(sql::INSERT_ROOT)
                .bind(&tx.tenant)
                .bind(req.repo_id)
                .bind(state.as_str())
                .bind(&req.title)
                .bind(&req.goal)
                .bind(req.source_ref.as_deref())
                .bind(policy.to_json())
                .bind(principal.render())
                .bind(now),
        )
        .await?
        .ok_or_else(|| backend("create returned no row"))?;
        let mut detail = json!({"source_ref": req.source_ref});
        if injection {
            detail["injection"] = json!(true);
        }
        tx.event(task.task_id, None, state, &principal, 1, detail)
            .await?;
        tx.commit().await?;
        Ok(task)
    }

    async fn plan_start(&self, task: TaskId) -> CampaignResult<PlanStart> {
        let mut tx = self.begin().await?;
        let probe = tx.peek(task).await?;
        if probe.kind == TaskKind::Leaf {
            return Err(CampaignError::Denied(
                "plan_start: a leaf is never planned".to_string(),
            ));
        }
        require_state(&probe, TaskState::Ready)?;
        // A cap may block the node, which rolls up: ancestors first, then the node.
        tx.lock_ancestors(&probe).await?;
        let t = tx.lock(task).await?;
        require_state(&t, TaskState::Ready)?;
        let policy = tx.policy_of(t.campaign_id).await?;
        let blocked = if i64::from(t.attempts) >= policy.max_plan_attempts {
            Some(BlockReason::AttemptsExhausted)
        } else if tx.campaign_plan_tokens(t.campaign_id).await?
            >= u64::try_from(policy.max_plan_tokens).unwrap_or(u64::MAX)
        {
            Some(BlockReason::TokenCap)
        } else {
            None
        };
        let out = match blocked {
            Some(reason) => {
                let task = tx
                    .transition(
                        task,
                        TaskState::Blocked,
                        &Actor::Planner,
                        json!({"reason": reason.as_str()}),
                    )
                    .await?;
                // A `blocked` child is a failure state: the parent is recomputed.
                tx.rollup_from(t.parent_id).await?;
                PlanStart::Blocked { task, reason }
            }
            None => {
                let task = tx
                    .transition(task, TaskState::Decomposing, &Actor::Planner, json!({}))
                    .await?;
                let expected_version = task.version;
                PlanStart::Started {
                    task,
                    expected_version,
                }
            }
        };
        tx.commit().await?;
        Ok(out)
    }

    async fn decompose(&self, req: Decomposition) -> CampaignResult<Decomposed> {
        let mut tx = self.begin().await?;
        // 1. Idempotency first: a replayed key is a no-op before any lookup.
        if tx.idem_exists(&req.attempt.idem_key).await? {
            return Err(CampaignError::AlreadyApplied);
        }
        // 2. The parent, locked; a concurrent replay that committed while we waited is
        //    visible now (READ COMMITTED), so the key is checked once more under the lock.
        let parent = tx.lock(req.parent).await?;
        if tx.idem_exists(&req.attempt.idem_key).await? {
            return Err(CampaignError::AlreadyApplied);
        }
        if parent.kind == TaskKind::Leaf {
            return Err(CampaignError::Denied(
                "decompose: a leaf has no children".to_string(),
            ));
        }
        cas(&parent, req.expected_version, TaskState::Decomposing)?;
        // 3. The batch.
        let n = req.children.len();
        if n == 0 || n > MAX_CHILDREN {
            return Err(CampaignError::Invalid(format!(
                "children: must be 1..={MAX_CHILDREN}"
            )));
        }
        for (i, c) in req.children.iter().enumerate() {
            c.validate(&format!("children[{i}]"))?;
        }
        check_deps(&req.children)?;
        check_max("reason", &req.reason, MAX_REASON)?;
        // 4. Caps, under the lock.
        let policy = tx.policy_of(parent.campaign_id).await?;
        let depth = parent.depth + 1;
        if depth > policy.depth_cap() {
            return Err(CampaignError::Invalid(format!(
                "depth: children would be at {depth}, max_depth is {}",
                policy.max_depth
            )));
        }
        let existing = tx.children_of(parent.task_id).await?;
        let cap = usize::from(policy.children_cap());
        if existing.len() + n > cap {
            return Err(CampaignError::Invalid(format!(
                "children: {} existing + {n} new exceeds {cap}",
                existing.len()
            )));
        }
        let nodes = usize::try_from(tx.campaign_nodes(parent.campaign_id).await?).unwrap_or(0);
        if nodes + n > usize::try_from(policy.max_nodes).unwrap_or(usize::MAX) {
            return Err(CampaignError::Invalid(format!(
                "max_nodes: {nodes} + {n} exceeds {}",
                policy.max_nodes
            )));
        }
        let max_ord = existing.iter().map(|c| c.ordinal).max().unwrap_or(0);
        // 5. The attempt row, closed `split`.
        let attempt_id = tx
            .plan_attempt(parent.task_id, &req.attempt, AttemptOutcome::Split, None)
            .await?;
        let by = Actor::Attempt(attempt_id);
        // 6. Children; ordinals continue from `max_ord`.
        let state = if policy.gated(depth) {
            TaskState::AwaitingApproval
        } else {
            TaskState::Ready
        };
        let now = ms(tx.now);
        let mut ids = Vec::with_capacity(n);
        for (i, c) in req.children.iter().enumerate() {
            let ordinal = max_ord + i as u8 + 1;
            let path = parent.path.child_of(ordinal)?;
            let child = fetch_task(
                &mut *tx.conn,
                sqlx::query(sql::INSERT_CHILD)
                    .bind(&tx.tenant)
                    .bind(parent.campaign_id.0)
                    .bind(parent.repo_id)
                    .bind(parent.task_id.0)
                    .bind(path.as_str())
                    .bind(i16::from(depth))
                    .bind(i16::from(ordinal))
                    .bind(state.as_str())
                    .bind(&c.title)
                    .bind(&c.goal)
                    .bind(json_text(&c.acceptance))
                    .bind(json_text(&c.touches))
                    .bind(c.est_size.map(EstSize::as_str))
                    .bind(by.render())
                    .bind(now),
            )
            .await?
            .ok_or_else(|| backend("child insert returned no row"))?;
            tx.event(child.task_id, None, state, &by, 1, json!({}))
                .await?;
            ids.push(child.task_id);
        }
        // 7. `depends_on` ordinals → sibling ids.
        for (i, c) in req.children.iter().enumerate() {
            if c.depends_on.is_empty() {
                continue;
            }
            let deps: Vec<i64> = c
                .depends_on
                .iter()
                .map(|d| ids[usize::from(*d) - 1].0)
                .collect();
            sqlx::query(sql::SET_DEPENDS_ON)
                .bind(&tx.tenant)
                .bind(ids[i].0)
                .bind(deps)
                .execute(&mut *tx.conn)
                .await
                .map_err(map_db)?;
        }
        // 8. Parent forward.
        let parent = tx
            .transition(
                parent.task_id,
                TaskState::Decomposed,
                &by,
                json!({"children": n, "reason": req.reason, "confidence": req.confidence}),
            )
            .await?;
        let mut children = Vec::with_capacity(n);
        for id in &ids {
            children.push(tx.peek(*id).await?);
        }
        tx.commit().await?;
        Ok(Decomposed {
            parent,
            children,
            attempt_id,
        })
    }

    async fn mark_leaf(&self, req: MarkLeaf) -> CampaignResult<Task> {
        let mut tx = self.begin().await?;
        if tx.idem_exists(&req.attempt.idem_key).await? {
            return Err(CampaignError::AlreadyApplied);
        }
        let t = tx.lock(req.task).await?;
        if tx.idem_exists(&req.attempt.idem_key).await? {
            return Err(CampaignError::AlreadyApplied);
        }
        if t.kind == TaskKind::Objective {
            return Err(CampaignError::Denied(
                "mark_leaf: the root is never executed".to_string(),
            ));
        }
        cas(&t, req.expected_version, TaskState::Decomposing)?;
        if !tx.children_of(t.task_id).await?.is_empty() {
            return Err(CampaignError::Conflict(
                "mark_leaf: the node has children".to_string(),
            ));
        }
        check_list(
            "acceptance",
            &req.acceptance,
            MAX_ACCEPTANCE,
            MAX_ACCEPTANCE_ITEM,
        )?;
        check_list("touches", &req.touches, MAX_TOUCHES, MAX_TOUCH)?;
        if !req.est_size.is_leaf_size() {
            return Err(CampaignError::Invalid(format!(
                "est_size: a leaf is xs or s, not {}",
                req.est_size.as_str()
            )));
        }
        check_max("reason", &req.reason, MAX_REASON)?;
        let policy = tx.policy_of(t.campaign_id).await?;
        let attempt_id = tx
            .plan_attempt(t.task_id, &req.attempt, AttemptOutcome::Execute, None)
            .await?;
        let by = Actor::Attempt(attempt_id);
        let to = if policy.gated(t.depth) {
            TaskState::AwaitingApproval
        } else {
            TaskState::Ready
        };
        // The transition checks the pre-write kind (`task`); the kind flips after.
        tx.transition(
            t.task_id,
            to,
            &by,
            json!({"execute": true, "reason": req.reason, "confidence": req.confidence}),
        )
        .await?;
        let task = patched(
            &mut *tx.conn,
            sqlx::query(sql::SET_LEAF)
                .bind(&tx.tenant)
                .bind(t.task_id.0)
                .bind(json_text(&req.acceptance))
                .bind(json_text(&req.touches))
                .bind(req.est_size.as_str()),
        )
        .await?;
        tx.commit().await?;
        Ok(task)
    }

    async fn plan_close(&self, req: PlanClose) -> CampaignResult<Task> {
        let mut tx = self.begin().await?;
        if tx.idem_exists(&req.attempt.idem_key).await? {
            return Err(CampaignError::AlreadyApplied);
        }
        let probe = tx.peek(req.task).await?;
        if probe.kind == TaskKind::Leaf {
            return Err(CampaignError::Denied(
                "plan_close: a leaf is never planned".to_string(),
            ));
        }
        // `reject` and an exhausted `error` block the node and roll up: ancestors first.
        tx.lock_ancestors(&probe).await?;
        let t = tx.lock(req.task).await?;
        if tx.idem_exists(&req.attempt.idem_key).await? {
            return Err(CampaignError::AlreadyApplied);
        }
        cas(&t, req.expected_version, TaskState::Decomposing)?;
        let task = match &req.outcome {
            PlanCloseOutcome::NeedsInfo { question } => {
                check_len("question", question, MAX_QUESTION)?;
                screen("question", question)?;
                let id = tx
                    .plan_attempt(t.task_id, &req.attempt, AttemptOutcome::NeedsInfo, None)
                    .await?;
                tx.transition(
                    t.task_id,
                    TaskState::AwaitingApproval,
                    &Actor::Attempt(id),
                    json!({"question": question}),
                )
                .await?
            }
            PlanCloseOutcome::Reject { reason } => {
                check_max("reason", reason, MAX_REASON)?;
                let id = tx
                    .plan_attempt(t.task_id, &req.attempt, AttemptOutcome::Reject, None)
                    .await?;
                let task = tx
                    .transition(
                        t.task_id,
                        TaskState::Blocked,
                        &Actor::Attempt(id),
                        json!({"reason": BlockReason::Reject.as_str(), "message": reason}),
                    )
                    .await?;
                // A `blocked` child is a failure state: the parent is recomputed
                // (`02-transactions.md` "Rollup rule").
                tx.rollup_from(t.parent_id).await?;
                task
            }
            PlanCloseOutcome::Error { error } => {
                let error = truncate_chars(error, MAX_ERROR);
                let id = tx
                    .plan_attempt(
                        t.task_id,
                        &req.attempt,
                        AttemptOutcome::Error,
                        Some(error.as_str()),
                    )
                    .await?;
                let policy = tx.policy_of(t.campaign_id).await?;
                let attempts = t.attempts.saturating_add(1);
                tx.set_attempts(t.task_id, attempts).await?;
                if i64::from(attempts) >= policy.max_plan_attempts {
                    let task = tx
                        .transition(
                            t.task_id,
                            TaskState::Blocked,
                            &Actor::Attempt(id),
                            json!({"reason": BlockReason::AttemptsExhausted.as_str(), "attempts": attempts}),
                        )
                        .await?;
                    tx.rollup_from(t.parent_id).await?;
                    task
                } else {
                    tx.transition(
                        t.task_id,
                        TaskState::Ready,
                        &Actor::Attempt(id),
                        json!({"error": true, "attempts": attempts}),
                    )
                    .await?
                }
            }
        };
        tx.commit().await?;
        Ok(task)
    }

    async fn claim(&self, req: ClaimRequest) -> CampaignResult<Vec<Claimed>> {
        if req.limit == 0 {
            return Ok(vec![]);
        }
        // The static pair the one-statement claim performs, asserted like every write.
        if !allowed(
            TaskState::Ready,
            TaskState::Claimed,
            TaskKind::Leaf,
            ActorClass::Driver,
        ) {
            return Err(backend("transition table: ready → claimed by driver"));
        }
        let lease = clamp_lease(req.lease_secs);
        let mut tx = self.begin().await?;
        let now = ms(tx.now);
        let mut tasks = fetch_tasks(
            &mut *tx.conn,
            sqlx::query(sql::CLAIM)
                .bind(&tx.tenant)
                .bind(limit_bind(req.limit))
                .bind(req.owner.as_str())
                .bind(now)
                .bind(i64::from(lease)),
        )
        .await?;
        // `RETURNING` carries no order: restore the queue order the candidates were
        // picked in.
        tasks.sort_by(|a, b| (a.campaign_id, &a.path).cmp(&(b.campaign_id, &b.path)));
        let by = Actor::Driver(req.owner.clone());
        let mut out = Vec::with_capacity(tasks.len());
        for (seq, task) in tasks.into_iter().enumerate() {
            tx.event(
                task.task_id,
                Some(TaskState::Ready),
                TaskState::Claimed,
                &by,
                task.version,
                json!({"lease_secs": lease}),
            )
            .await?;
            let idem =
                IdemKey::synthetic([unsigned(task.task_id.0), task.version, tx.now, seq as u64]);
            let attempt_id = tx
                .insert_attempt(
                    task.task_id,
                    AttemptKind::Work,
                    &idem,
                    Some(&req.owner),
                    AttemptOutcome::Pending,
                    None,
                    None,
                )
                .await?;
            out.push(Claimed { task, attempt_id });
        }
        tx.commit().await?;
        Ok(out)
    }

    async fn heartbeat(&self, task: TaskId, owner: &Owner, lease_secs: i64) -> CampaignResult<()> {
        let lease = clamp_lease(lease_secs);
        let now = ms((self.now_ms)());
        let done = sqlx::query(sql::HEARTBEAT)
            .bind(&self.tenant)
            .bind(task.0)
            .bind(owner.as_str())
            .bind(now)
            .bind(i64::from(lease))
            .execute(&self.pool)
            .await
            .map_err(map_db)?;
        if done.rows_affected() == 0 {
            return Err(CampaignError::LeaseLost);
        }
        Ok(())
    }

    async fn reap(&self) -> CampaignResult<Vec<Reaped>> {
        let mut tx = self.begin().await?;
        let now = ms(tx.now);
        let rows = sqlx::query(sql::REAP)
            .bind(&tx.tenant)
            .bind(now)
            .fetch_all(&mut *tx.conn)
            .await
            .map_err(map_db)?;
        let mut reaped = Vec::with_capacity(rows.len());
        for row in &rows {
            let from: String = col(row, "from_state")?;
            let owner: String = col(row, "lost_owner")?;
            reaped.push((
                TaskId(col(row, "task_id")?),
                parse_col("from_state", &from, TaskState::parse)?,
                Owner::parse(&owner).map_err(|e| backend(&format!("column lost_owner: {e}")))?,
                unsigned(col(row, "version")?),
            ));
        }
        reaped.sort_by_key(|r| r.0);
        let mut out = Vec::with_capacity(reaped.len());
        for (task_id, from_state, lost_owner, version) in reaped {
            if !allowed(
                from_state,
                TaskState::Ready,
                TaskKind::Leaf,
                ActorClass::Reaper,
            ) {
                return Err(CampaignError::Denied(format!(
                    "{} → ready on a leaf by reaper",
                    from_state.as_str()
                )));
            }
            tx.event(
                task_id,
                Some(from_state),
                TaskState::Ready,
                &Actor::Reaper,
                version,
                json!({"lost_owner": lost_owner.as_str()}),
            )
            .await?;
            tx.close_work(
                task_id,
                None,
                AttemptOutcome::LeaseLost,
                WorkPatch::default(),
            )
            .await?;
            out.push(Reaped {
                task_id,
                from_state,
                lost_owner,
            });
        }
        tx.commit().await?;
        Ok(out)
    }

    async fn start(&self, task: TaskId, owner: &Owner) -> CampaignResult<Task> {
        let mut tx = self.begin().await?;
        let t = tx.lock(task).await?;
        if t.claimed_by.as_ref() != Some(owner) {
            return Err(CampaignError::LeaseLost);
        }
        require_state(&t, TaskState::Claimed)?;
        let task = tx
            .transition(
                task,
                TaskState::Running,
                &Actor::Worker(owner.clone()),
                json!({}),
            )
            .await?;
        tx.commit().await?;
        Ok(task)
    }

    async fn complete(&self, req: Complete) -> CampaignResult<Task> {
        let mut tx = self.begin().await?;
        let t = tx.lock(req.task).await?;
        if t.claimed_by.as_ref() != Some(&req.owner) {
            return Err(CampaignError::LeaseLost);
        }
        require_state(&t, TaskState::Running)?;
        req.pr.validate()?;
        session_ok(&req.session_id)?;
        let by = Actor::Worker(req.owner.clone());
        tx.transition(
            t.task_id,
            TaskState::InReview,
            &by,
            json!({"pr_number": req.pr.number, "pr_url": req.pr.url}),
        )
        .await?;
        let task = patched(
            &mut *tx.conn,
            sqlx::query(sql::SET_PR)
                .bind(&tx.tenant)
                .bind(t.task_id.0)
                .bind(req.pr.number)
                .bind(&req.pr.url)
                .bind(&req.pr.branch),
        )
        .await?;
        tx.close_work(
            t.task_id,
            Some(&req.owner),
            AttemptOutcome::Pr,
            WorkPatch {
                pr_url: Some(req.pr.url.clone()),
                error: None,
                tokens: Some(req.tokens.clamped()),
                session_id: req.session_id.clone(),
            },
        )
        .await?;
        tx.commit().await?;
        Ok(task)
    }

    async fn fail(&self, req: Fail) -> CampaignResult<Task> {
        let mut tx = self.begin().await?;
        let probe = tx.peek(req.task).await?;
        tx.lock_ancestors(&probe).await?;
        let t = tx.lock(req.task).await?;
        if t.claimed_by.as_ref() != Some(&req.owner) {
            return Err(CampaignError::LeaseLost);
        }
        require_state(&t, TaskState::Running)?;
        session_ok(&req.session_id)?;
        let error = truncate_chars(&req.error, MAX_ERROR);
        let by = Actor::Worker(req.owner.clone());
        tx.transition(
            t.task_id,
            TaskState::Failed,
            &by,
            json!({"cause": req.cause.outcome().as_str()}),
        )
        .await?;
        tx.close_work(
            t.task_id,
            Some(&req.owner),
            req.cause.outcome(),
            WorkPatch {
                pr_url: None,
                error: Some(error),
                tokens: Some(req.tokens.clamped()),
                session_id: req.session_id.clone(),
            },
        )
        .await?;
        tx.block_dependents(&t).await?;
        tx.rollup_from(t.parent_id).await?;
        let task = tx.peek(t.task_id).await?;
        tx.commit().await?;
        Ok(task)
    }

    async fn resolve_review(&self, task: TaskId, outcome: ReviewOutcome) -> CampaignResult<Task> {
        let mut tx = self.begin().await?;
        let probe = tx.peek(task).await?;
        tx.lock_ancestors(&probe).await?;
        let t = tx.lock(task).await?;
        require_state(&t, TaskState::InReview)?;
        let (to, verdict) = match outcome {
            ReviewOutcome::Merged => (TaskState::Done, "merged"),
            ReviewOutcome::Closed => (TaskState::Failed, "closed"),
        };
        tx.transition(task, to, &Actor::Poller, json!({"review": verdict}))
            .await?;
        if to == TaskState::Failed {
            tx.block_dependents(&t).await?;
        }
        tx.rollup_from(t.parent_id).await?;
        let task = tx.peek(task).await?;
        tx.commit().await?;
        Ok(task)
    }

    async fn approve(
        &self,
        task: TaskId,
        expected_version: u64,
        actor: &Actor,
    ) -> CampaignResult<Task> {
        let principal = Actor::User(actor.human()?.clone());
        let mut tx = self.begin().await?;
        let t = tx.lock(task).await?;
        let out = if t.state == TaskState::InReview {
            // PR approval is an event, not a state change.
            cas(&t, expected_version, TaskState::InReview)?;
            tx.event(
                task,
                Some(TaskState::InReview),
                TaskState::InReview,
                &principal,
                t.version,
                json!({"pr_approved": true}),
            )
            .await?;
            t
        } else {
            cas(&t, expected_version, TaskState::AwaitingApproval)?;
            tx.transition(task, TaskState::Ready, &principal, json!({}))
                .await?
        };
        tx.commit().await?;
        Ok(out)
    }

    async fn approve_children(&self, parent: TaskId, actor: &Actor) -> CampaignResult<Vec<Task>> {
        let principal = Actor::User(actor.human()?.clone());
        let mut tx = self.begin().await?;
        tx.lock(parent).await?;
        let mut out = vec![];
        for c in tx.children_of(parent).await? {
            if c.state == TaskState::AwaitingApproval {
                out.push(
                    tx.transition(c.task_id, TaskState::Ready, &principal, json!({}))
                        .await?,
                );
            }
        }
        tx.commit().await?;
        Ok(out)
    }

    async fn answer(
        &self,
        task: TaskId,
        expected_version: u64,
        text: String,
        actor: &Actor,
    ) -> CampaignResult<Task> {
        let principal = Actor::User(actor.human()?.clone());
        let mut tx = self.begin().await?;
        let t = tx.lock(task).await?;
        cas(&t, expected_version, TaskState::AwaitingApproval)?;
        check_len("answer", &text, MAX_ANSWER)?;
        screen("answer", &text)?;
        let goal = format!("{}{CLARIFICATION_HEADER}{text}", t.goal);
        check_max("goal", &goal, MAX_GOAL)?;
        tx.transition(
            task,
            TaskState::Ready,
            &principal,
            json!({"answered": true}),
        )
        .await?;
        let task = patched(
            &mut *tx.conn,
            sqlx::query(sql::SET_GOAL)
                .bind(&tx.tenant)
                .bind(task.0)
                .bind(goal),
        )
        .await?;
        tx.commit().await?;
        Ok(task)
    }

    async fn retry(&self, task: TaskId, actor: &Actor) -> CampaignResult<Task> {
        let principal = Actor::User(actor.human()?.clone());
        let mut tx = self.begin().await?;
        let probe = tx.peek(task).await?;
        tx.lock_ancestors(&probe).await?;
        let t = tx.lock(task).await?;
        if !matches!(t.state, TaskState::Failed | TaskState::Blocked) {
            return Err(CampaignError::Conflict(format!(
                "state: retry needs failed or blocked, found {}",
                t.state.as_str()
            )));
        }
        let task = tx
            .transition(
                t.task_id,
                TaskState::Ready,
                &principal,
                json!({"retry": true}),
            )
            .await?;
        tx.rollup_from(t.parent_id).await?;
        tx.commit().await?;
        Ok(task)
    }

    async fn update_policy(
        &self,
        campaign: TaskId,
        policy: Policy,
        actor: &Actor,
    ) -> CampaignResult<Task> {
        let principal = Actor::User(actor.human()?.clone());
        policy.validate()?;
        let mut tx = self.begin().await?;
        let t = tx.lock(campaign).await?;
        if !t.is_root() {
            return Err(CampaignError::Invalid(
                "campaign: policy lives on the root only".to_string(),
            ));
        }
        let now = ms(tx.now);
        let task = patched(
            &mut *tx.conn,
            sqlx::query(sql::SET_POLICY)
                .bind(&tx.tenant)
                .bind(campaign.0)
                .bind(policy.to_json())
                .bind(now),
        )
        .await?;
        tx.event(
            campaign,
            Some(task.state),
            task.state,
            &principal,
            task.version,
            json!({"policy_updated": true}),
        )
        .await?;
        tx.commit().await?;
        Ok(task)
    }

    async fn cancel(&self, task: TaskId, actor: &Actor) -> CampaignResult<Vec<Task>> {
        let principal = Actor::User(actor.human()?.clone());
        let mut tx = self.begin().await?;
        let probe = tx.peek(task).await?;
        if probe.state.is_terminal() {
            return Err(CampaignError::Conflict(format!(
                "state: cannot cancel a {} node",
                probe.state.as_str()
            )));
        }
        tx.lock_ancestors(&probe).await?;
        let t = tx.lock(task).await?;
        if t.state.is_terminal() {
            return Err(CampaignError::Conflict(format!(
                "state: cannot cancel a {} node",
                t.state.as_str()
            )));
        }
        let mut out = vec![];
        for node in tx.subtree_of(&t, true).await? {
            if node.state.is_terminal() {
                continue;
            }
            out.push(
                tx.transition(node.task_id, TaskState::Cancelled, &principal, json!({}))
                    .await?,
            );
            tx.close_work(
                node.task_id,
                None,
                AttemptOutcome::LeaseLost,
                WorkPatch::default(),
            )
            .await?;
        }
        tx.rollup_from(t.parent_id).await?;
        tx.commit().await?;
        Ok(out)
    }

    async fn replan(&self, task: TaskId, actor: &Actor) -> CampaignResult<Task> {
        let principal = Actor::User(actor.human()?.clone());
        let mut tx = self.begin().await?;
        let probe = tx.peek(task).await?;
        if probe.kind == TaskKind::Leaf {
            return Err(CampaignError::Denied(
                "replan: a leaf has no plan".to_string(),
            ));
        }
        tx.lock_ancestors(&probe).await?;
        let t = tx.lock(task).await?;
        if !matches!(t.state, TaskState::Decomposed | TaskState::Blocked) {
            return Err(CampaignError::Conflict(format!(
                "state: replan needs decomposed or blocked, found {}",
                t.state.as_str()
            )));
        }
        let mut superseded = 0usize;
        for node in tx.subtree_of(&t, true).await? {
            if node.task_id == t.task_id || node.state.is_terminal() {
                continue;
            }
            tx.transition(
                node.task_id,
                TaskState::Superseded,
                &principal,
                json!({"superseded_by": t.task_id}),
            )
            .await?;
            sqlx::query(sql::SET_SUPERSEDED_BY)
                .bind(&tx.tenant)
                .bind(node.task_id.0)
                .bind(t.task_id.0)
                .execute(&mut *tx.conn)
                .await
                .map_err(map_db)?;
            tx.close_work(
                node.task_id,
                None,
                AttemptOutcome::LeaseLost,
                WorkPatch::default(),
            )
            .await?;
            superseded += 1;
        }
        tx.transition(
            t.task_id,
            TaskState::Decomposing,
            &principal,
            json!({"replan": true, "superseded": superseded}),
        )
        .await?;
        tx.set_attempts(t.task_id, 0).await?;
        let task = tx.peek(t.task_id).await?;
        tx.commit().await?;
        Ok(task)
    }

    async fn get(&self, task: TaskId) -> CampaignResult<Task> {
        self.peek(task).await
    }

    async fn list_campaigns(&self, filter: ListFilter) -> CampaignResult<Vec<Task>> {
        fetch_tasks(
            &self.pool,
            sqlx::query(sql::LIST_CAMPAIGNS)
                .bind(&self.tenant)
                .bind(filter.repo_id)
                .bind(filter.needs_attention),
        )
        .await
    }

    async fn subtree(&self, node: TaskId) -> CampaignResult<Vec<Task>> {
        let t = self.peek(node).await?;
        fetch_tasks(
            &self.pool,
            sqlx::query(sql::SUBTREE)
                .bind(&self.tenant)
                .bind(t.campaign_id.0)
                .bind(t.task_id.0)
                .bind(t.path.subtree_like()),
        )
        .await
    }

    async fn children(&self, parent: TaskId) -> CampaignResult<Vec<Task>> {
        self.peek(parent).await?;
        fetch_tasks(
            &self.pool,
            sqlx::query(sql::CHILDREN).bind(&self.tenant).bind(parent.0),
        )
        .await
    }

    async fn events(&self, task: TaskId) -> CampaignResult<Vec<TaskEvent>> {
        self.peek(task).await?;
        sqlx::query(sql::EVENTS)
            .bind(&self.tenant)
            .bind(task.0)
            .fetch_all(&self.pool)
            .await
            .map_err(map_db)?
            .iter()
            .map(row_to_event)
            .collect()
    }

    async fn attempts(&self, task: TaskId) -> CampaignResult<Vec<TaskAttempt>> {
        self.peek(task).await?;
        sqlx::query(sql::ATTEMPTS)
            .bind(&self.tenant)
            .bind(task.0)
            .fetch_all(&self.pool)
            .await
            .map_err(map_db)?
            .iter()
            .map(row_to_attempt)
            .collect()
    }

    async fn plannable(&self, limit: usize) -> CampaignResult<Vec<Task>> {
        fetch_tasks(
            &self.pool,
            sqlx::query(sql::PLANNABLE)
                .bind(&self.tenant)
                .bind(limit_bind(limit)),
        )
        .await
    }

    async fn in_review(&self, limit: usize) -> CampaignResult<Vec<Task>> {
        fetch_tasks(
            &self.pool,
            sqlx::query(sql::IN_REVIEW)
                .bind(&self.tenant)
                .bind(limit_bind(limit)),
        )
        .await
    }
}

#[cfg(test)]
mod tests;
