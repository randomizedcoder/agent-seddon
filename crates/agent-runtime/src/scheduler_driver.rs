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

use std::future::Future;
use std::sync::Arc;

use agent_config_store::Backend;
use agent_scheduler::{RunObserver, StoreScheduler};

use crate::agent::Agent;

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
    /// Injectable clock, so tests are deterministic; production uses the default
    /// wall clock the built scheduler already carries.
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
            now_ms: None,
        }
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

    /// Tick every driven tenant once, running each due job's goal through `exec`
    /// (given the owning tenant and the goal). Returns the total jobs fired.
    ///
    /// Factored out of [`tick`](Self::tick) so the fanning + per-tenant claim logic
    /// is testable without a whole [`Agent`]: a test passes a fake `exec` that
    /// records `(tenant, goal)` pairs.
    pub(crate) async fn tick_with_exec<F, Fut>(&self, exec: F) -> usize
    where
        F: Fn(String, String) -> Fut + Clone,
        Fut: Future<Output = agent_core::Result<String>>,
    {
        let mut fired = 0usize;
        for tenant in self.tenants().await {
            let Some(sched) = self.scheduler_for(&tenant) else {
                tracing::warn!(tenant = %tenant, "scheduler: skipping tenant (unsafe segment)");
                continue;
            };
            let exec = exec.clone();
            let t = tenant.clone();
            let n = sched
                .tick_with(move |goal| {
                    let exec = exec.clone();
                    let t = t.clone();
                    async move { exec(t, goal).await }
                })
                .await;
            match n {
                Ok(k) => fired += k,
                Err(e) => {
                    tracing::warn!(tenant = %tenant, error = %e, "scheduler: tenant tick failed")
                }
            }
        }
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

#[cfg(test)]
mod tests;
