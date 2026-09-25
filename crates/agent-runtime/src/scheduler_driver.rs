//! The tenant-fanning scheduler driver (config C2c-2, design of record
//! [`docs/design/config/10-per-tenant-scheduler.md`]).
//!
//! C2c-1 landed the durable, tenant-keyed backend
//! ([`StoreScheduler`](agent_scheduler::StoreScheduler)) as library + tests only —
//! deliberately not selectable in config, so no tenant's jobs could be accepted and
//! then never fired. This is the other half: the driver that actually *fires* them.
//!
//! # Why a driver, not a `PerTenant` wrap
//!
//! The scheduler has two halves (see the design doc). The **registry** half
//! (`schedule`/`list`/`cancel`/`history`) is a passive store, so it routes per
//! tenant with the same [`PerTenant`](crate::tenant::PerTenant) layer every other
//! shared-store seam uses — that is the served `--serve-scheduler` seam. The
//! **driver** half — ticking due jobs and running each as a fresh headless turn —
//! is process-bound (`tick_with` is inherent on the scheduler, not on the
//! `Scheduler` trait, because a job's executor is *this* process). A `PerTenant`
//! wrap of the registry alone would accept a non-`local` tenant's jobs and then
//! tick only the `local` instance, silently never firing them — the "oversold
//! isolation" footgun `CLAUDE.md` warns against. So the driver must **fan out over
//! tenants** itself: enumerate the tenants that own jobs, and tick each tenant's
//! durable scheduler **under that tenant's identity** so the fired turn reads that
//! tenant's registries, prompts, memory, and graph (config C2 / C2b).
//!
//! # Isolation boundary (honest scope)
//!
//! A fired job runs in **this process**, scoped to its tenant's identity but not
//! sandboxed from it. That is the correct Tier-1 shape and matches every other
//! per-tenant seam today. Strong per-tenant *process* isolation of fired jobs is
//! plane-01 (multi-tenancy C23/C24): when that lands, the driver dispatches into a
//! per-tenant sandbox instead of an in-process turn. Called out as a dependency,
//! not implied here.

use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use agent_config_store::Backend;
use agent_scheduler::{wall_clock_ms, RunObserver, StoreScheduler};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::task::JoinSet;

use crate::agent::Agent;

/// A tenant's claimed-but-not-yet-dispatched jobs: the tenant, its per-tenant
/// scheduler, and the queue of claimed `(job_id, goal)` pairs to fire.
type ClaimBatch = (String, Arc<StoreScheduler>, VecDeque<(String, String)>);
/// One claimed job ready to dispatch: `(tenant, its scheduler, job_id, goal)`.
type ClaimedJob = (String, Arc<StoreScheduler>, String, String);

/// Drives the durable, store-backed scheduler by fanning the tick over tenants.
///
/// Holds the shared backend and the same claim/cap/observer configuration the
/// served seam was built with, so a job the seam accepted is fired with identical
/// semantics. A fresh [`StoreScheduler`] is built per tenant per tick — cheap (an
/// `Arc` handle plus a tenant `String`), and all durable state (claims, history,
/// `next_fire`) lives in the store, so nothing is lost between ticks or across a
/// restart.
pub(crate) struct StoreDriver {
    backend: Arc<dyn Backend>,
    /// When true, fire every tenant's jobs (fanning); when false, only the default
    /// `local` tenant (single-tenant durable install).
    per_tenant: bool,
    claim_ttl_ms: u64,
    max_jobs: usize,
    observer: RunObserver,
    /// Global ceiling on jobs firing concurrently across all tenants in one tick
    /// (scheduler S2 fairness). `1` = serial (single-tenant byte-identical); `0` =
    /// unbounded.
    max_concurrent: usize,
    /// Ceiling on one tenant's concurrently-firing jobs, so a single tenant's
    /// backlog cannot consume the whole global ceiling. `0` = bounded only by
    /// `max_concurrent`.
    max_inflight_per_tenant: usize,
    /// Rotates the (sorted) tenant tick order each tick, so no tenant's
    /// lexicographic position starves it (scheduler S2 fairness).
    rr_cursor: AtomicUsize,
    /// Injectable clock, so tests are deterministic; production uses the wall clock.
    now_ms: Option<Arc<dyn Fn() -> u64 + Send + Sync>>,
}

impl StoreDriver {
    pub(crate) fn new(
        backend: Arc<dyn Backend>,
        per_tenant: bool,
        claim_ttl_ms: u64,
        max_jobs: usize,
        observer: RunObserver,
    ) -> Self {
        Self {
            backend,
            per_tenant,
            claim_ttl_ms,
            max_jobs,
            observer,
            // Serial by default → today's behaviour (single-tenant byte-identical);
            // an operator raises these to fire independent tenants' jobs in parallel.
            max_concurrent: 1,
            max_inflight_per_tenant: 1,
            rr_cursor: AtomicUsize::new(0),
            now_ms: None,
        }
    }

    /// Set the fairness caps (config C2c-2, scheduler S2). `max_concurrent` is the
    /// global ceiling on concurrently-firing jobs; `max_inflight_per_tenant` bounds
    /// any one tenant's share of it. A `0` cap means unbounded.
    pub(crate) fn with_fairness(
        mut self,
        max_concurrent: usize,
        max_inflight_per_tenant: usize,
    ) -> Self {
        self.max_concurrent = max_concurrent;
        self.max_inflight_per_tenant = max_inflight_per_tenant;
        self
    }

    /// The tick's `now`, from the injected clock (tests) or the wall clock.
    fn now(&self) -> u64 {
        self.now_ms.as_ref().map_or_else(wall_clock_ms, |f| f())
    }

    /// Override the clock (tests only).
    #[cfg(test)]
    fn with_clock(mut self, f: Arc<dyn Fn() -> u64 + Send + Sync>) -> Self {
        self.now_ms = Some(f);
        self
    }

    /// Build the per-tenant durable scheduler with this driver's configuration. A
    /// hostile tenant segment (should never reach here — tenants come from the
    /// store, where `schedule` gated them — but fail closed anyway) yields `None`,
    /// so the caller skips it rather than firing it against the base view.
    fn scheduler_for(&self, tenant: &str) -> Option<StoreScheduler> {
        let mut s = StoreScheduler::with_tenant(self.backend.clone(), tenant)
            .ok()?
            .with_claim_ttl_ms(self.claim_ttl_ms)
            .with_max_jobs(self.max_jobs)
            .with_observer(self.observer.clone());
        if let Some(clock) = &self.now_ms {
            s = s.with_clock(clock.clone());
        }
        Some(s)
    }

    /// The tenants to drive this tick. With `per_tenant`, every tenant that owns a
    /// job card (a cheap distinct-tenant query); otherwise just the default `local`
    /// tenant. A discovery failure fires nothing this tick (fail closed) — logged,
    /// not panicked, since the backend is attacker-reachable state.
    async fn tenants(&self) -> Vec<String> {
        if !self.per_tenant {
            return vec![agent_scheduler::store::DEFAULT_TENANT.to_string()];
        }
        match self
            .backend
            .tenants(agent_scheduler::store::COLLECTION)
            .await
        {
            Ok(ts) => ts,
            Err(e) => {
                tracing::warn!(error = %e, "scheduler: tenant discovery failed; firing nothing this tick");
                Vec::new()
            }
        }
    }

    /// The tenants to drive this tick, rotated round-robin so no tenant's
    /// lexicographic position (the sorted order [`Backend::tenants`] returns)
    /// starves it across ticks.
    async fn rotated_tenants(&self) -> Vec<String> {
        let mut tenants = self.tenants().await;
        if tenants.len() > 1 {
            let start = self.rr_cursor.fetch_add(1, Ordering::Relaxed) % tenants.len();
            tenants.rotate_left(start);
        }
        tenants
    }

    /// Tick every driven tenant once, running each due job's goal through `exec`
    /// (given the owning tenant and the goal). Returns the total jobs claimed and
    /// dispatched this tick (a job whose `exec` fails is still counted — its run is
    /// recorded `Failed`).
    ///
    /// Factored out of [`tick`](Self::tick) so the fanning + per-tenant claim logic
    /// is testable without a whole [`Agent`]: a test passes a fake `exec` that
    /// records `(tenant, goal)` pairs.
    ///
    /// **Fairness (scheduler S2).** Due jobs are first *claimed* per tenant (S1's
    /// CAS claim, so overlap/cross-driver races stay excluded), then *dispatched*
    /// round-robin-interleaved across tenants under a global concurrency ceiling
    /// (`max_concurrent`) and a per-tenant in-flight cap (`max_inflight_per_tenant`)
    /// — so a busy tenant's backlog can neither starve nor drown the others. With
    /// both caps at their `1` default, dispatch is serial and interleaved (today's
    /// behaviour for a single tenant, fair ordering for many).
    pub(crate) async fn tick_with_exec<F, Fut>(&self, exec: F) -> usize
    where
        F: Fn(String, String) -> Fut + Clone + Send + 'static,
        Fut: Future<Output = agent_core::Result<String>> + Send,
    {
        let now = self.now();

        // 1. Claim every tenant's due jobs, in rotated (round-robin) tenant order.
        let mut batches: Vec<ClaimBatch> = Vec::new();
        for tenant in self.rotated_tenants().await {
            let Some(sched) = self.scheduler_for(&tenant) else {
                tracing::warn!(tenant = %tenant, "scheduler: skipping tenant (unsafe segment)");
                continue;
            };
            let sched = Arc::new(sched);
            match sched.claim_due(now).await {
                Ok(due) if !due.is_empty() => batches.push((tenant, sched, due.into())),
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!(tenant = %tenant, error = %e, "scheduler: tenant claim failed");
                }
            }
        }

        // 2. Interleave the claims round-robin → a flat order that alternates tenants.
        let jobs = interleave(batches);
        let fired = jobs.len();
        if fired == 0 {
            return 0;
        }

        // 3. Dispatch under the global ceiling + per-tenant in-flight cap.
        let global = make_semaphore(self.max_concurrent);
        let mut per_tenant: HashMap<String, Arc<Semaphore>> = HashMap::new();
        let mut set = JoinSet::new();
        for (tenant, sched, id, goal) in jobs {
            let gate = per_tenant
                .entry(tenant.clone())
                .or_insert_with(|| make_semaphore(self.max_inflight_per_tenant))
                .clone();
            let global = global.clone();
            let exec = exec.clone();
            set.spawn(async move {
                // Per-tenant permit first, then the global one — a fixed acquire
                // order across every task, so the two semaphores cannot deadlock.
                let _tpermit = acquire(gate).await;
                let _gpermit = acquire(global).await;
                let t = tenant.clone();
                if let Err(e) = sched.run_claimed(&id, goal, move |g| exec(t, g)).await {
                    tracing::warn!(tenant = %tenant, job = %id, error = %e, "scheduler: run/finish failed");
                }
            });
        }
        while set.join_next().await.is_some() {}
        fired
    }

    /// Fire every due job across every driven tenant, running each as a fresh
    /// headless turn of `agent` **scoped to the job's owning tenant** — so the turn
    /// reads that tenant's per-tenant seams. Returns the total jobs fired.
    pub(crate) async fn tick(&self, agent: &Arc<Agent>) -> usize {
        let agent = Arc::clone(agent);
        self.tick_with_exec(move |tenant, goal| {
            let agent = Arc::clone(&agent);
            async move {
                // Scope the fired turn to the owning tenant. A synthetic session id
                // (`scheduler`) carries the driver's runs; the tenant segment came
                // from the store (already `safe_segment`), but fall back to the
                // trusted `local` key if it somehow is not, never to an escape.
                let key = agent_core::SessionKey::parse(&tenant, "scheduler")
                    .unwrap_or_else(|_| agent_core::SessionKey::local("scheduler"));
                agent_core::scope(key, agent.run(&goal))
                    .await
                    .map_err(|e| agent_core::Error::Scheduler(e.to_string()))
            }
        })
        .await
    }
}

/// Flatten per-tenant claim batches into one dispatch order that **alternates
/// tenants** — one job per tenant per round — so a tenant with a long backlog does
/// not run all its jobs before any other tenant's first (scheduler S2 fairness).
fn interleave(mut batches: Vec<ClaimBatch>) -> Vec<ClaimedJob> {
    let mut out = Vec::new();
    let mut progress = true;
    while progress {
        progress = false;
        for (tenant, sched, queue) in &mut batches {
            if let Some((id, goal)) = queue.pop_front() {
                out.push((tenant.clone(), sched.clone(), id, goal));
                progress = true;
            }
        }
    }
    out
}

/// A semaphore for a fairness cap; a `0` cap means **unbounded** (mirrors the gRPC
/// admission-layer convention).
fn make_semaphore(cap: usize) -> Arc<Semaphore> {
    let permits = if cap == 0 {
        Semaphore::MAX_PERMITS
    } else {
        cap
    };
    Arc::new(Semaphore::new(permits))
}

/// Acquire one owned permit; these semaphores are never closed, so this cannot fail.
async fn acquire(sem: Arc<Semaphore>) -> OwnedSemaphorePermit {
    sem.acquire_owned()
        .await
        .expect("scheduler fairness semaphore is never closed")
}

#[cfg(test)]
mod tests;
