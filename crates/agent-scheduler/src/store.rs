//! `StoreScheduler` — the **durable, tenant-keyed** [`Scheduler`], behind the
//! non-default `scheduler-store` feature (config design C41 / C2c,
//! `docs/design/config/10-per-tenant-scheduler.md`).
//!
//! Where [`LocalScheduler`](crate::LocalScheduler) keeps jobs in an in-memory
//! `HashMap` (Tier-0, one process), `StoreScheduler` persists each job as one
//! card in the shared [`agent_config_store`] backend, keyed `(collection =
//! "scheduler", tenant, job_id)` — exactly the transactional store the other
//! control-plane seams (`agent-registry`, `agent-review-fleet`, `agent-prompt`)
//! converged onto. That durability is what makes per-tenant scheduling possible:
//! a **tenant-fanning driver** (config C2c-2) can enumerate the tenants with due
//! jobs ([`Backend::tenants`]) and tick each one's view, where a `PerTenant`
//! wrap over an in-memory scheduler would accept a tenant's jobs and then never
//! fire them.
//!
//! The behaviour is a faithful port of `LocalScheduler`'s: the overlap guard
//! (a live claim skips the fire, visibly), stale/future-claim reclaim, one-shot
//! spent-disable, bounded history, and the hostile-input clamps — the same
//! [`claim_is_live`](crate::claim_is_live)/[`push_history`](crate::push_history)
//! definitions, so the two tiers cannot drift.
//!
//! Concurrency note (fail-closed, not oversold): `Backend::apply` is an
//! atomic **batch**, not a compare-and-set. So a claim written here gives
//! single-driver overlap-prevention and crash recovery (the TTL), exactly as
//! `LocalScheduler` does — **not** cross-driver mutual exclusion. Two drivers
//! ticking one backend could both claim the same job in the same instant. True
//! multi-driver exclusion needs a CAS primitive the `Backend` does not expose;
//! it is a bounded follow-up (config C2c-2), called out rather than implied.

use std::future::Future;
use std::sync::Arc;

use agent_config_store::{Backend, Write};
use agent_core::{safe_segment, Error, Job, JobId, Result, Run, RunOutcome, Scheduler};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{
    claim_is_live, next_fire, parse, push_history, wall_clock_ms, RunObserver,
    DEFAULT_CLAIM_TTL_MS, MAX_DETAIL_CHARS,
};

/// The collection every scheduler job card lives in.
const COLLECTION: &str = "scheduler";

/// The single-tenant default, matching the other converged store seams: a
/// `StoreScheduler::new` (no verified identity) reads and writes `local`.
pub const DEFAULT_TENANT: &str = "local";

/// The at-rest shape of one job: the public [`Job`] plus the runtime bookkeeping
/// (`claimed_at_ms`, `history`) `LocalScheduler` keeps beside it in memory. Serde
/// JSON, like `StorePrompt` — [`Job`]/[`Run`] already derive `Serialize`, so no
/// new proto (and no `buf` surface).
#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredJob {
    job: Job,
    /// When the in-flight run claimed this job (epoch ms); `None` if idle.
    #[serde(default)]
    claimed_at_ms: Option<u64>,
    #[serde(default)]
    history: Vec<Run>,
}

fn encode_job(sj: &StoredJob) -> Result<Vec<u8>> {
    serde_json::to_vec(sj).map_err(|e| Error::Scheduler(format!("encode job: {e}")))
}

fn decode_job(bytes: &[u8]) -> Result<StoredJob> {
    serde_json::from_slice(bytes).map_err(|e| Error::Scheduler(format!("decode job: {e}")))
}

/// A durable, single-tenant view of the scheduler over a shared [`Backend`].
/// Cheap to clone/build — it is the backend handle plus the tenant key and the
/// same tunables as [`LocalScheduler`](crate::LocalScheduler).
pub struct StoreScheduler {
    backend: Arc<dyn Backend>,
    tenant: String,
    now_ms: Arc<dyn Fn() -> u64 + Send + Sync>,
    observer: Option<RunObserver>,
    claim_ttl_ms: u64,
    /// Cap on jobs per tenant, since the model can create them.
    max_jobs: usize,
}

impl StoreScheduler {
    /// A scheduler over `backend` for the default (`local`) tenant.
    pub fn new(backend: Arc<dyn Backend>) -> Self {
        Self {
            backend,
            tenant: DEFAULT_TENANT.to_string(),
            now_ms: Arc::new(wall_clock_ms),
            observer: None,
            claim_ttl_ms: DEFAULT_CLAIM_TTL_MS,
            max_jobs: 64,
        }
    }

    /// A scheduler scoped to `tenant`. The tenant is `safe_segment`-gated at the
    /// trust boundary (it becomes a store key), fail-closed on a hostile value —
    /// mirroring `StoreRegistry::with_tenant`.
    pub fn with_tenant(backend: Arc<dyn Backend>, tenant: &str) -> Result<Self> {
        if !safe_segment(tenant) {
            return Err(Error::Scheduler(format!("invalid tenant `{tenant}`")));
        }
        Ok(Self {
            backend,
            tenant: tenant.to_string(),
            now_ms: Arc::new(wall_clock_ms),
            observer: None,
            claim_ttl_ms: DEFAULT_CLAIM_TTL_MS,
            max_jobs: 64,
        })
    }

    pub fn with_observer(mut self, o: RunObserver) -> Self {
        self.observer = Some(o);
        self
    }
    pub fn with_claim_ttl_ms(mut self, ms: u64) -> Self {
        self.claim_ttl_ms = ms.max(1);
        self
    }
    pub fn with_max_jobs(mut self, n: usize) -> Self {
        self.max_jobs = n.max(1);
        self
    }
    #[doc(hidden)]
    pub fn with_clock(mut self, f: Arc<dyn Fn() -> u64 + Send + Sync>) -> Self {
        self.now_ms = f;
        self
    }

    fn now(&self) -> u64 {
        (self.now_ms)()
    }

    /// Persist one job card (an idempotent `EnsureTenant` + `Put` batch). The
    /// `EnsureTenant` is required by the store's foreign-key check and is a no-op
    /// once the tenant exists.
    async fn write_job(&self, sj: &StoredJob) -> Result<()> {
        let blob = encode_job(sj)?;
        self.backend
            .apply(&[
                Write::EnsureTenant {
                    tenant: self.tenant.clone(),
                },
                Write::Put {
                    collection: COLLECTION,
                    tenant: self.tenant.clone(),
                    id: sj.job.id.clone(),
                    blob,
                },
            ])
            .await
    }

    /// Load every job card for this tenant (decoded, hostile blobs fail closed).
    async fn load(&self) -> Result<Vec<StoredJob>> {
        self.backend
            .list(COLLECTION, &self.tenant)
            .await?
            .iter()
            .map(|b| decode_job(b))
            .collect()
    }

    /// Jobs whose next fire has arrived, claimed for execution — the durable twin
    /// of `LocalScheduler::claim_due`. Returns `(id, goal)` for jobs that won a
    /// claim; records a visible `Skipped` run for each that lost to a live claim.
    /// All the re-armed/claimed cards are persisted in one atomic batch.
    pub async fn claim_due(&self, now: u64) -> Result<Vec<(JobId, String)>> {
        let mut due = Vec::new();
        let mut skipped: Vec<Run> = Vec::new();
        let mut writes: Vec<Write> = Vec::new();

        for mut sj in self.load().await? {
            if !sj.job.enabled {
                continue;
            }
            let Some(next) = sj.job.next_fire_ms else {
                continue; // spent one-shot
            };
            if next > now {
                continue;
            }
            if let Some(claimed) = sj.claimed_at_ms {
                if claim_is_live(claimed, now, self.claim_ttl_ms) {
                    // Still running — drop this fire rather than stacking, and
                    // re-arm so the job does not spin on the same due instant.
                    let r = Run {
                        job_id: sj.job.id.clone(),
                        started_ms: now,
                        finished_ms: now,
                        outcome: RunOutcome::Skipped,
                        detail: "previous run still in flight".into(),
                    };
                    push_history(&mut sj.history, r.clone());
                    sj.job.next_fire_ms = next_fire(&sj.job.schedule, now);
                    skipped.push(r);
                    writes.push(self.put_write(&sj)?);
                    continue;
                }
            }
            sj.claimed_at_ms = Some(now);
            sj.job.next_fire_ms = next_fire(&sj.job.schedule, now);
            due.push((sj.job.id.clone(), sj.job.goal.clone()));
            writes.push(self.put_write(&sj)?);
        }

        if !writes.is_empty() {
            let mut batch = Vec::with_capacity(writes.len() + 1);
            batch.push(Write::EnsureTenant {
                tenant: self.tenant.clone(),
            });
            batch.extend(writes);
            self.backend.apply(&batch).await?;
        }
        // Observe skips only after the state they describe is committed.
        if let Some(o) = &self.observer {
            for r in &skipped {
                o(r);
            }
        }
        Ok(due)
    }

    /// Record a run's outcome and release its claim — the durable twin of
    /// `LocalScheduler::finish`. A spent one-shot (no next fire) is disabled.
    pub async fn finish(&self, id: &str, run: Run) -> Result<()> {
        // A hostile id cannot name a stored card; nothing to update.
        if safe_segment(id) {
            if let Some(blob) = self.backend.get(COLLECTION, &self.tenant, id).await? {
                let mut sj = decode_job(&blob)?;
                sj.claimed_at_ms = None;
                push_history(&mut sj.history, run.clone());
                if sj.job.next_fire_ms.is_none() {
                    sj.job.enabled = false;
                }
                self.write_job(&sj).await?;
            }
        }
        if let Some(o) = &self.observer {
            o(&run);
        }
        Ok(())
    }

    /// Fire every due job once, running each through `exec`. The single-tenant
    /// driver step; the multi-tenant driver (config C2c-2) fans this out over
    /// [`Backend::tenants`]. Returns how many ran.
    pub async fn tick_with<F, Fut>(&self, exec: F) -> Result<usize>
    where
        F: Fn(String) -> Fut,
        Fut: Future<Output = Result<String>>,
    {
        let now = self.now();
        let due = self.claim_due(now).await?;
        let count = due.len();
        for (id, goal) in due {
            let started = self.now();
            let out = exec(goal).await;
            let finished = self.now();
            let (outcome, detail) = match out {
                Ok(answer) => (RunOutcome::Completed, answer),
                Err(e) => (RunOutcome::Failed, e.to_string()),
            };
            self.finish(
                &id,
                Run {
                    job_id: id.clone(),
                    started_ms: started,
                    finished_ms: finished,
                    outcome,
                    detail: detail.chars().take(MAX_DETAIL_CHARS).collect(),
                },
            )
            .await?;
        }
        Ok(count)
    }

    /// A `Put` for one job card (encoding fallible; a bad card fails the batch).
    fn put_write(&self, sj: &StoredJob) -> Result<Write> {
        Ok(Write::Put {
            collection: COLLECTION,
            tenant: self.tenant.clone(),
            id: sj.job.id.clone(),
            blob: encode_job(sj)?,
        })
    }
}

#[async_trait]
impl Scheduler for StoreScheduler {
    fn name(&self) -> &str {
        "store"
    }

    async fn schedule(&self, spec: &str, goal: &str) -> Result<JobId> {
        if goal.trim().is_empty() {
            return Err(Error::Scheduler("a scheduled job needs a goal".into()));
        }
        let now = self.now();
        let sched = parse(spec, now)?;
        // A one-shot already in the past never fires; say so now rather than
        // persisting a job that silently does nothing.
        let next = next_fire(&sched, now);
        if next.is_none() {
            return Err(Error::Scheduler(
                "that one-shot time is already in the past".into(),
            ));
        }
        // Per-tenant cap. Best-effort (count-then-write is not atomic without a
        // CAS), which is acceptable for a control-plane surface — it bounds a
        // runaway writer, not a tight race.
        if self.backend.count(COLLECTION, &self.tenant).await? >= self.max_jobs {
            return Err(Error::Scheduler(format!(
                "too many scheduled jobs (limit {})",
                self.max_jobs
            )));
        }
        // Durable, collision-free id (no in-memory counter to lose on restart).
        let id = format!("job-{}", uuid::Uuid::new_v4().simple());
        let sj = StoredJob {
            job: Job {
                id: id.clone(),
                spec: spec.trim().to_string(),
                schedule: sched,
                goal: goal.trim().to_string(),
                next_fire_ms: next,
                enabled: true,
            },
            claimed_at_ms: None,
            history: Vec::new(),
        };
        self.write_job(&sj).await?;
        Ok(id)
    }

    async fn list(&self) -> Result<Vec<Job>> {
        let mut out: Vec<Job> = self.load().await?.into_iter().map(|s| s.job).collect();
        // Stable order, so `list` output is reproducible.
        out.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(out)
    }

    async fn cancel(&self, id: &str) -> Result<bool> {
        // A hostile id cannot name a stored card; report "did not exist" rather
        // than letting it reach the store as a key.
        if !safe_segment(id) {
            return Ok(false);
        }
        let existed = self
            .backend
            .get(COLLECTION, &self.tenant, id)
            .await?
            .is_some();
        self.backend
            .apply(&[Write::Delete {
                collection: COLLECTION,
                tenant: self.tenant.clone(),
                id: id.to_string(),
            }])
            .await?;
        Ok(existed)
    }

    async fn history(&self, id: &str) -> Result<Vec<Run>> {
        if !safe_segment(id) {
            return Ok(Vec::new());
        }
        match self.backend.get(COLLECTION, &self.tenant, id).await? {
            Some(blob) => Ok(decode_job(&blob)?.history),
            None => Ok(Vec::new()),
        }
    }
}

// The table-driven suite (four classes + adversarial) lives at the file end per
// clippy `items_after_test_module`.
#[cfg(test)]
mod tests;
