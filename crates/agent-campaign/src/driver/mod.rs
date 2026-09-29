//! The campaign driver tick (`docs/design/campaigns/04-executor.md`, CP-05): per
//! tenant, in rotated order, **reap** expired leases and stale plans, **poll** the
//! open PRs, **plan** a bounded batch of nodes, **claim** leaves for this process's
//! owner token, then **interleave** the claims across tenants and dispatch each one
//! to a worker under a per-tenant and a global semaphore.
//!
//! Everything process-bound is a seam here so the tick runs over `MemCampaigns`
//! with scripted doubles (T11): the store comes from a [`CampaignBackend`], the
//! planner is a [`TickPlanner`] (the shipped [`FactoryPlanner`] builds a
//! [`Planner`] per tenant per tick), the poller a [`PrPoller`] ([`NoopPoller`]
//! until CP-06's forge poller), the worker a [`WorkerExec`]. **With no exec the
//! claim phase is off**: the shipped CP-05 driver reaps, polls and plans, and
//! reports `claimed 0  dispatched 0` honestly rather than burning attempts on
//! leaves nothing can execute (the worker body is CP-06).
//!
//! # Deviations from the design sketch
//!
//! The tick does **not** join its workers: a leaf may legitimately run for
//! `worker_timeout`, and a tick that waited on it would stop reaping and planning
//! for every other tenant. The `JoinSet` and both semaphores persist on the
//! [`Driver`]; a tick first harvests the workers that finished since the last one,
//! and `running(tenant)` is simply the tenant semaphore's permits in use. The claim
//! limit per tenant is `min(per-tenant free permits, remaining global budget)`, so
//! permit acquisition never waits. [`Driver::drain`] joins with a deadline for
//! once-mode, tests and shutdown; whatever it aborts holds a lease that expires and
//! `reap()` returns to `ready`.
//!
//! A worker that returns `Err`, times out or panics is settled by the driver:
//! `claimed → running → failed` (or `running → failed`) under this owner, with the
//! bounded error text and the `error` / `timeout` cause; a worker that already
//! wrote its own terminal state is left alone (`LeaseLost` / `Conflict` are
//! swallowed with a warning).

use agent_core::campaign::{
    truncate_chars, CampaignBackend, CampaignResult, CampaignStore, ClaimRequest, Claimed, Fail,
    FailCause, Owner, Policy, Task, TaskId, TaskState, TokenUsage, DECOMPOSING_MAX_SECS, MAX_ERROR,
};
use agent_core::{safe_segment, SessionKey};
use async_trait::async_trait;
use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::task::{JoinHandle, JoinSet};
use tracing::Instrument;

use crate::planner::{Planned, Planner, TickSummary};

pub mod poller;

/// Default `in_review` leaves handed to the poller per tenant per tick
/// (`04-executor.md` "PR poller"; the `[campaign] poll_batch` key).
pub const POLL_BATCH: usize = 20;

/// The driver's knobs, mapped from `[campaign]` by the runtime (`build_driver`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverConfig {
    /// `false` ⇒ [`Driver::tick`] returns [`TickReport::disabled`] without a store
    /// call (`negative_disabled`).
    pub enabled: bool,
    /// Concurrent workers per tenant (the per-tenant semaphore).
    pub per_tenant_workers: usize,
    /// Concurrent workers across every tenant (the global semaphore).
    pub global_workers: usize,
    /// Nodes the planner phase decomposes per tenant per tick; `0` skips the phase.
    pub plan_per_tick: usize,
    /// Wall clock per worker; past it the leaf is failed with cause `timeout`.
    pub worker_timeout: Duration,
    /// The lease claimed leaves are held under (a claim spans campaigns, so the
    /// per-campaign `policy.lease_secs` is honoured by the worker's heartbeat, CP-06).
    pub lease_secs: i64,
    /// How long a node may sit in `decomposing` before the reaper releases it.
    pub decomposing_max_secs: i64,
    /// `in_review` leaves the poll phase hands the [`PrPoller`] per tenant per
    /// tick.
    pub poll_batch: usize,
}

impl Default for DriverConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            per_tenant_workers: 2,
            global_workers: 8,
            plan_per_tick: 4,
            worker_timeout: Duration::from_secs(3_600),
            lease_secs: Policy::default().lease_secs,
            decomposing_max_secs: DECOMPOSING_MAX_SECS,
            poll_batch: POLL_BATCH,
        }
    }
}

/// Which tenants a tick serves: a fixed list (`--tenant T`, or `["local"]` for a
/// single-tenant install) or every tenant the backend reports live work for
/// (`[tenancy] per_tenant`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tenants {
    Fixed(Vec<String>),
    Discover,
}

/// Builds the planner for one tenant's store (the provider, brief and touch
/// resolver are captured once by the CLI).
pub type PlannerFactory = Arc<dyn Fn(Arc<dyn CampaignStore>) -> Planner + Send + Sync>;

/// One tenant's claims awaiting dispatch: the tenant with its opened store, and
/// the leaves claimed for it this tick.
type ClaimBatch = ((String, Arc<dyn CampaignStore>), Vec<Claimed>);

/// What the plan phase did for one tenant: the tally plus one line's worth per
/// node, so the CLI can print what CP-04's `run --once` printed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlanReport {
    pub summary: TickSummary,
    /// The queue in `plannable` order with each node's outcome; an `Err` is a store
    /// failure counted in `summary.failures`.
    pub nodes: Vec<(Task, CampaignResult<Planned>)>,
}

/// The plan phase seam: plan up to `limit` nodes of `store`. Never fails.
#[async_trait]
pub trait TickPlanner: Send + Sync {
    async fn tick(&self, store: Arc<dyn CampaignStore>, limit: usize) -> PlanReport;
}

/// The shipped [`TickPlanner`]: one [`Planner`] per tenant per tick from the
/// factory, then the CLI's own loop (`plannable` read once, `plan_node` per node,
/// a store error counted and the tick continued).
pub struct FactoryPlanner(pub PlannerFactory);

#[async_trait]
impl TickPlanner for FactoryPlanner {
    async fn tick(&self, store: Arc<dyn CampaignStore>, limit: usize) -> PlanReport {
        let mut report = PlanReport::default();
        let queue = match store.plannable(limit).await {
            Ok(q) => q,
            Err(e) => {
                tracing::warn!(error = %e, "campaign.tick: plannable failed");
                report.summary.failures += 1;
                return report;
            }
        };
        report.summary.selected = queue.len();
        let planner = (self.0)(store);
        for task in queue {
            let planned = planner.plan_node(task.task_id).await;
            match &planned {
                Ok(p) => report.summary.add(p),
                Err(e) => {
                    tracing::warn!(task = %task.task_id, error = %e, "campaign.tick: node failed");
                    report.summary.failures += 1;
                }
            }
            report.nodes.push((task, planned));
        }
        report
    }
}

/// What the poll phase did for one tenant.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PollReport {
    pub polled: usize,
    pub merged: usize,
    pub closed: usize,
    /// Merged on the forge but waiting for a human `approve`
    /// (`require_pr_approval`); the leaf stays `in_review`.
    pub awaiting: usize,
    /// Forge failures, timeouts, unknown states, store errors; each is logged.
    pub errors: usize,
}

/// The poll phase seam: resolve up to `batch` `in_review` leaves of `store`.
/// The shipped implementation is [`poller::ForgePoller`].
#[async_trait]
pub trait PrPoller: Send + Sync {
    async fn poll(&self, store: Arc<dyn CampaignStore>, batch: usize) -> PollReport;
}

/// No `[forge]` backend configured: reports zeros and never touches the store,
/// so `in_review` leaves are never resolved (the runtime warns once at build).
pub struct NoopPoller;

#[async_trait]
impl PrPoller for NoopPoller {
    async fn poll(&self, _store: Arc<dyn CampaignStore>, _batch: usize) -> PollReport {
        PollReport::default()
    }
}

/// The worker seam: run one claimed leaf to its own terminal state. `Ok(())`
/// means the worker wrote that state itself (`complete` / `fail`); `Err(text)`
/// makes the driver fail the leaf with `text` (bounded) as the error.
#[async_trait]
pub trait WorkerExec: Send + Sync {
    async fn run(
        &self,
        tenant: &str,
        store: Arc<dyn CampaignStore>,
        claimed: &Claimed,
        owner: &Owner,
    ) -> Result<(), String>;
}

/// A [`WorkerExec`] from a closure (tests and the in-process sandbox).
pub struct ClosureExec<F>(pub F);

#[async_trait]
impl<F, Fut> WorkerExec for ClosureExec<F>
where
    F: Fn(String, Arc<dyn CampaignStore>, Claimed, Owner) -> Fut + Send + Sync,
    Fut: Future<Output = Result<(), String>> + Send,
{
    async fn run(
        &self,
        tenant: &str,
        store: Arc<dyn CampaignStore>,
        claimed: &Claimed,
        owner: &Owner,
    ) -> Result<(), String> {
        (self.0)(tenant.to_string(), store, claimed.clone(), owner.clone()).await
    }
}

/// A fresh owner token for one driver process: a random 128-bit value as 32
/// lowercase hex chars (`04-executor.md`), which is a path-safe segment by
/// construction.
pub fn mint_owner() -> Owner {
    Owner::parse(&uuid::Uuid::new_v4().simple().to_string())
        .expect("32 lowercase hex chars are a path-safe segment")
}

/// How one dispatched worker ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerOutcome {
    /// The worker returned `Ok`: it wrote its own terminal state.
    Ok,
    /// The worker returned `Err`; the driver failed the leaf (`error`).
    Error,
    /// `worker_timeout` elapsed; the driver failed the leaf (`timeout`).
    Timeout,
    /// The worker task panicked; the driver failed the leaf with the bounded text.
    Panic,
}

/// One finished worker, harvested by the next tick or by [`Driver::drain`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settled {
    pub tenant: String,
    pub task: TaskId,
    pub outcome: WorkerOutcome,
}

/// One tenant's phases in one tick.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TenantReport {
    pub tenant: String,
    /// Expired leases returned to `ready`.
    pub reaped: usize,
    /// Stale `decomposing` nodes returned to `ready`.
    pub released: usize,
    pub poll: PollReport,
    /// `None` when `plan_per_tick == 0`.
    pub plan: Option<PlanReport>,
    pub claimed: usize,
    /// Set when the tenant was skipped before any phase (an unsafe segment, an
    /// open failure); the text is fixed or the seam's own, never the tenant.
    pub skipped: Option<String>,
    /// Store / planner failures in this tenant's phases; each is logged.
    pub errors: usize,
}

/// What one [`Driver::tick`] did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TickReport {
    /// `true` ⇒ `enabled = false`: nothing else is set and nothing was called.
    pub disabled: bool,
    /// The tenants served, in the (rotated) order they were served.
    pub tenants: Vec<String>,
    pub per_tenant: Vec<TenantReport>,
    /// `(tenant, leaf)` in dispatch order — interleaved across tenants.
    pub dispatched: Vec<(String, TaskId)>,
    /// Workers that finished since the previous tick.
    pub harvested: Vec<Settled>,
    /// Tenant discovery + every tenant's `errors`.
    pub errors: usize,
}

impl TickReport {
    /// The report of a disabled driver.
    pub fn disabled() -> Self {
        Self {
            disabled: true,
            ..Self::default()
        }
    }

    pub fn reaped(&self) -> usize {
        self.per_tenant.iter().map(|t| t.reaped).sum()
    }

    pub fn released(&self) -> usize {
        self.per_tenant.iter().map(|t| t.released).sum()
    }

    /// Nodes the plan phase handled (every tenant's `PlanReport.nodes`).
    pub fn planned(&self) -> usize {
        self.per_tenant
            .iter()
            .filter_map(|t| t.plan.as_ref())
            .map(|p| p.nodes.len())
            .sum()
    }

    pub fn claimed(&self) -> usize {
        self.per_tenant.iter().map(|t| t.claimed).sum()
    }

    /// Harvested workers that did not end `Ok`.
    pub fn failed(&self) -> usize {
        self.harvested
            .iter()
            .filter(|s| s.outcome != WorkerOutcome::Ok)
            .count()
    }
}

/// What [`Driver::drain`] collected.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DrainReport {
    pub settled: Vec<Settled>,
    /// Workers still running at the deadline, aborted; their leases expire.
    pub aborted: usize,
}

/// The driver: one owner token, the seams, the persistent worker set and the two
/// semaphores. One per process (`agent campaign run`); `run --once` builds one for
/// a single tick and drains it.
pub struct Driver {
    backend: Arc<dyn CampaignBackend>,
    tenants: Tenants,
    cfg: DriverConfig,
    owner: Owner,
    planner: Arc<dyn TickPlanner>,
    poller: Arc<dyn PrPoller>,
    exec: Option<Arc<dyn WorkerExec>>,
    global: Arc<Semaphore>,
    per_tenant: Mutex<HashMap<String, Arc<Semaphore>>>,
    workers: tokio::sync::Mutex<JoinSet<Settled>>,
    /// Rotates the tenant order each tick so no tenant's position starves it.
    rr_cursor: AtomicUsize,
}

impl std::fmt::Debug for Driver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Driver")
            .field("tenants", &self.tenants)
            .field("cfg", &self.cfg)
            .field("owner", &self.owner)
            .field("exec", &self.exec.is_some())
            .finish_non_exhaustive()
    }
}

impl Driver {
    /// A driver over `backend` serving `tenants`, with a fresh owner token, the
    /// [`NoopPoller`] and **no exec** (claims off) until [`Driver::with_exec`].
    pub fn new(
        backend: Arc<dyn CampaignBackend>,
        tenants: Tenants,
        cfg: DriverConfig,
        planner: Arc<dyn TickPlanner>,
    ) -> Self {
        let global = Arc::new(Semaphore::new(cfg.global_workers.max(1)));
        Self {
            backend,
            tenants,
            cfg,
            owner: mint_owner(),
            planner,
            poller: Arc::new(NoopPoller),
            exec: None,
            global,
            per_tenant: Mutex::new(HashMap::new()),
            workers: tokio::sync::Mutex::new(JoinSet::new()),
            rr_cursor: AtomicUsize::new(0),
        }
    }

    #[must_use]
    pub fn with_poller(mut self, poller: Arc<dyn PrPoller>) -> Self {
        self.poller = poller;
        self
    }

    /// `None` turns the claim phase off (the shipped CP-05 driver).
    #[must_use]
    pub fn with_exec(mut self, exec: Option<Arc<dyn WorkerExec>>) -> Self {
        self.exec = exec;
        self
    }

    pub fn owner(&self) -> &Owner {
        &self.owner
    }

    pub fn config(&self) -> &DriverConfig {
        &self.cfg
    }

    pub fn tenants(&self) -> &Tenants {
        &self.tenants
    }

    /// Whether the claim phase runs (an exec is wired).
    pub fn has_exec(&self) -> bool {
        self.exec.is_some()
    }

    /// Free permits on the global semaphore (T11's probe; `global_workers` when
    /// nothing runs).
    #[doc(hidden)]
    pub fn global_available(&self) -> usize {
        self.global.available_permits()
    }

    /// One tick: harvest, tenants, per-tenant phases, interleaved dispatch. Never
    /// fails; every store error is logged and counted in the report.
    pub async fn tick(&self) -> TickReport {
        if !self.cfg.enabled {
            return TickReport::disabled();
        }
        self.tick_inner()
            .instrument(tracing::info_span!("campaign.tick"))
            .await
    }

    async fn tick_inner(&self) -> TickReport {
        let mut report = TickReport {
            harvested: self.harvest().await,
            ..TickReport::default()
        };
        let tenants = self.tenants_for_tick(&mut report).await;
        report.tenants.clone_from(&tenants);

        // 1–2. Per tenant, under that tenant's identity: reap, release, poll, plan,
        //      claim within the remaining global budget.
        let mut budget = self.global.available_permits();
        let mut claims: Vec<ClaimBatch> = Vec::new();
        for tenant in &tenants {
            let key = SessionKey::parse(tenant, "campaign")
                .unwrap_or_else(|_| SessionKey::local("campaign"));
            let (tr, claimed) = agent_core::scope(key, self.tick_tenant(tenant, &mut budget)).await;
            report.errors += tr.errors;
            report.per_tenant.push(tr);
            if let Some((store, claimed)) = claimed {
                if !claimed.is_empty() {
                    claims.push(((tenant.clone(), store), claimed));
                }
            }
        }

        // 3. Interleave (A, B, C, A, B, C …) and dispatch: per-tenant permit first,
        //    then the global one — a fixed order, so the two cannot deadlock — and
        //    neither waits, since the claims were sized to the free permits.
        for ((tenant, store), claim) in interleave(claims) {
            let Some(exec) = self.exec.clone() else {
                break;
            };
            let tenant_permit = acquire(self.tenant_semaphore(&tenant)).await;
            let global_permit = acquire(Arc::clone(&self.global)).await;
            let task = claim.task.task_id;
            report.dispatched.push((tenant.clone(), task));
            self.workers.lock().await.spawn(run_worker(
                tenant,
                store,
                claim,
                self.owner.clone(),
                exec,
                self.cfg.worker_timeout,
                tenant_permit,
                global_permit,
            ));
        }

        tracing::info!(
            tenants = report.tenants.len(),
            reaped = report.reaped(),
            released = report.released(),
            planned = report.planned(),
            claimed = report.claimed(),
            dispatched = report.dispatched.len(),
            harvested = report.harvested.len(),
            failed = report.failed(),
            errors = report.errors,
            "campaign.tick"
        );
        report
    }

    /// The phases of one tenant. Returns the report and, when the tenant was
    /// opened, its store with the leaves claimed for dispatch.
    async fn tick_tenant(
        &self,
        tenant: &str,
        budget: &mut usize,
    ) -> (TenantReport, Option<(Arc<dyn CampaignStore>, Vec<Claimed>)>) {
        let mut tr = TenantReport {
            tenant: tenant.to_string(),
            ..TenantReport::default()
        };
        if !safe_segment(tenant) {
            tracing::warn!("campaign.tick: tenant is not a path-safe segment; skipped");
            tr.skipped = Some("tenant is not a path-safe segment".to_string());
            tr.errors += 1;
            return (tr, None);
        }
        let store = match self.backend.with_tenant(tenant) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(tenant, error = %e, "campaign.tick: could not open the tenant; skipped");
                tr.skipped = Some(format!("open failed: {e}"));
                tr.errors += 1;
                return (tr, None);
            }
        };
        match store.reap().await {
            Ok(r) => tr.reaped = r.len(),
            Err(e) => {
                tracing::warn!(tenant, error = %e, "campaign.tick: reap failed");
                tr.errors += 1;
            }
        }
        match store.reap_decomposing(self.cfg.decomposing_max_secs).await {
            Ok(r) => tr.released = r.len(),
            Err(e) => {
                tracing::warn!(tenant, error = %e, "campaign.tick: reap_decomposing failed");
                tr.errors += 1;
            }
        }
        tr.poll = self
            .poller
            .poll(Arc::clone(&store), self.cfg.poll_batch)
            .await;
        tr.errors += tr.poll.errors;
        if self.cfg.plan_per_tick > 0 {
            let plan = self
                .planner
                .tick(Arc::clone(&store), self.cfg.plan_per_tick)
                .await;
            tr.errors += plan.summary.failures;
            tr.plan = Some(plan);
        }
        let mut claimed = Vec::new();
        if self.exec.is_some() {
            let free = self.tenant_semaphore(tenant).available_permits();
            let n = claim_limit(free, *budget);
            if n > 0 {
                match store
                    .claim(ClaimRequest {
                        owner: self.owner.clone(),
                        limit: n,
                        lease_secs: self.cfg.lease_secs,
                    })
                    .await
                {
                    Ok(c) => {
                        *budget = budget.saturating_sub(c.len());
                        tr.claimed = c.len();
                        claimed = c;
                    }
                    Err(e) => {
                        tracing::warn!(tenant, error = %e, "campaign.tick: claim failed");
                        tr.errors += 1;
                    }
                }
            }
        }
        (tr, Some((store, claimed)))
    }

    /// The tenants to serve this tick, rotated round-robin (like the scheduler
    /// driver) so no tenant's position in the list starves it. A discovery failure
    /// serves nothing this tick, logged and counted.
    async fn tenants_for_tick(&self, report: &mut TickReport) -> Vec<String> {
        let mut list = match &self.tenants {
            Tenants::Fixed(v) => v.clone(),
            Tenants::Discover => match self.backend.tenants().await {
                Ok(t) => t,
                Err(e) => {
                    tracing::warn!(error = %e, "campaign.tick: tenant discovery failed; serving nothing this tick");
                    report.errors += 1;
                    Vec::new()
                }
            },
        };
        if list.len() > 1 {
            let start = self.rr_cursor.fetch_add(1, Ordering::Relaxed) % list.len();
            list.rotate_left(start);
        }
        list
    }

    fn tenant_semaphore(&self, tenant: &str) -> Arc<Semaphore> {
        let mut map = self
            .per_tenant
            .lock()
            .expect("per-tenant semaphores poisoned");
        Arc::clone(
            map.entry(tenant.to_string())
                .or_insert_with(|| Arc::new(Semaphore::new(self.cfg.per_tenant_workers.max(1)))),
        )
    }

    /// Collect the workers that finished since the last call (never blocks).
    async fn harvest(&self) -> Vec<Settled> {
        let mut set = self.workers.lock().await;
        let mut out = Vec::new();
        while let Some(r) = set.try_join_next() {
            match r {
                Ok(s) => out.push(s),
                Err(e) => tracing::warn!(error = %e, "campaign.tick: a worker task did not settle"),
            }
        }
        out
    }

    /// Join every running worker, up to `deadline`; whatever is still running is
    /// aborted (its lease expires and `reap()` returns the leaf to `ready`).
    pub async fn drain(&self, deadline: Duration) -> DrainReport {
        let mut set = self.workers.lock().await;
        let mut settled = Vec::new();
        let joined = tokio::time::timeout(deadline, async {
            while let Some(r) = set.join_next().await {
                match r {
                    Ok(s) => settled.push(s),
                    Err(e) => {
                        tracing::warn!(error = %e, "campaign.drain: a worker task did not settle");
                    }
                }
            }
        })
        .await;
        let aborted = if joined.is_err() {
            let n = set.len();
            set.abort_all();
            while set.join_next().await.is_some() {}
            n
        } else {
            0
        };
        DrainReport { settled, aborted }
    }
}

/// What the exec's own task yields: the exec's result under its timeout.
type WorkerRun = Result<Result<(), String>, tokio::time::error::Elapsed>;

/// Aborts the inner worker task when the outer one is dropped or aborted, so a
/// drained driver leaves nothing running.
struct AbortOnDrop(JoinHandle<WorkerRun>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// One dispatched worker: the exec under its timeout on its own task (so a panic
/// is a `JoinError` here, never the driver's), then the driver's settlement of
/// anything the worker did not settle itself. The permits live until the settle
/// write is done.
#[allow(clippy::too_many_arguments)]
async fn run_worker(
    tenant: String,
    store: Arc<dyn CampaignStore>,
    claim: Claimed,
    owner: Owner,
    exec: Arc<dyn WorkerExec>,
    timeout: Duration,
    _tenant_permit: OwnedSemaphorePermit,
    _global_permit: OwnedSemaphorePermit,
) -> Settled {
    let task = claim.task.task_id;
    let mut inner = {
        let (tenant, store, owner) = (tenant.clone(), Arc::clone(&store), owner.clone());
        AbortOnDrop(tokio::spawn(async move {
            tokio::time::timeout(timeout, exec.run(&tenant, store, &claim, &owner)).await
        }))
    };
    // `JoinHandle` is `Unpin`, so the guard keeps ownership while the handle is
    // awaited in place.
    let outcome = match (&mut inner.0).await {
        Ok(Ok(Ok(()))) => WorkerOutcome::Ok,
        Ok(Ok(Err(e))) => {
            settle_failure(&*store, task, &owner, FailCause::Error, &e).await;
            WorkerOutcome::Error
        }
        Ok(Err(_elapsed)) => {
            let text = format!("worker timed out after {}s", timeout.as_secs());
            settle_failure(&*store, task, &owner, FailCause::Timeout, &text).await;
            WorkerOutcome::Timeout
        }
        Err(join) => {
            let text = if join.is_panic() {
                panic_text(join.into_panic())
            } else {
                "worker task was cancelled".to_string()
            };
            settle_failure(&*store, task, &owner, FailCause::Error, &text).await;
            WorkerOutcome::Panic
        }
    };
    tracing::info!(tenant, task = %task, outcome = ?outcome, "campaign.worker: settled");
    Settled {
        tenant,
        task,
        outcome,
    }
}

/// Fail `task` under `owner` unless the worker already moved it: `claimed` is
/// started first (a worker that never began), `running` is failed, anything else
/// is left as it is. Store refusals (`LeaseLost`, `Conflict`) are logged only.
async fn settle_failure(
    store: &dyn CampaignStore,
    task: TaskId,
    owner: &Owner,
    cause: FailCause,
    error: &str,
) {
    let state = match store.get(task).await {
        Ok(t) => t.state,
        Err(e) => {
            tracing::warn!(task = %task, error = %e, "campaign.worker: could not read the leaf to settle it");
            return;
        }
    };
    match state {
        TaskState::Claimed => {
            if let Err(e) = store.start(task, owner).await {
                tracing::warn!(task = %task, error = %e, "campaign.worker: could not start the leaf to settle it");
                return;
            }
        }
        TaskState::Running => {}
        other => {
            tracing::info!(task = %task, state = other.as_str(), "campaign.worker: leaf already settled");
            return;
        }
    }
    let req = Fail {
        task,
        owner: owner.clone(),
        error: truncate_chars(error, MAX_ERROR),
        cause,
        tokens: TokenUsage::default(),
        session_id: None,
    };
    if let Err(e) = store.fail(req).await {
        tracing::warn!(task = %task, error = %e, "campaign.worker: could not fail the leaf");
    }
}

/// The bounded error text for a panicking worker: the payload when it is a
/// string, a fixed phrase otherwise, cut to `MAX_ERROR` chars.
pub(crate) fn panic_text(payload: Box<dyn std::any::Any + Send>) -> String {
    let msg = payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "non-string payload".to_string());
    truncate_chars(&format!("worker panicked: {msg}"), MAX_ERROR)
}

/// Flatten per-tenant claim batches into one dispatch order that alternates
/// tenants — one leaf per tenant per round — so a tenant with a long queue does
/// not run all its leaves before another tenant's first.
pub(crate) fn interleave<K: Clone, V>(batches: Vec<(K, Vec<V>)>) -> Vec<(K, V)> {
    let mut queues: Vec<(K, std::collections::VecDeque<V>)> =
        batches.into_iter().map(|(k, v)| (k, v.into())).collect();
    let mut out = Vec::new();
    let mut progress = true;
    while progress {
        progress = false;
        for (k, q) in &mut queues {
            if let Some(v) = q.pop_front() {
                out.push((k.clone(), v));
                progress = true;
            }
        }
    }
    out
}

/// How many leaves one tenant may claim this tick: its free per-tenant permits,
/// capped by what is left of the global budget.
pub(crate) fn claim_limit(per_tenant_free: usize, global_budget: usize) -> usize {
    per_tenant_free.min(global_budget)
}

/// Acquire one owned permit; these semaphores are never closed.
async fn acquire(sem: Arc<Semaphore>) -> OwnedSemaphorePermit {
    sem.acquire_owned()
        .await
        .expect("campaign driver semaphores are never closed")
}

#[cfg(test)]
mod tests;
