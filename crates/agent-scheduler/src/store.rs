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
//! Concurrency note (fail-closed): claims are written under
//! [`Write::CompareAndSwap`] (scheduler S1) — a claim lands only if the job card
//! still holds the bytes this driver read, so exactly one of several drivers
//! ticking the same backend wins a due job and the losers see an
//! [`is_conflict`](agent_config_store::is_conflict) error and skip it (no
//! double-fire). This is **cross-driver** mutual exclusion, above the
//! single-driver overlap-prevention + TTL crash-recovery `LocalScheduler` gives.
//! An `owner` token on each claim records which driver holds it (observability +
//! a fail-closed release: a stale finisher whose claim was TTL-reclaimed by
//! another driver hits a conflict and does not clobber the new claim).

use std::future::Future;
use std::sync::Arc;

use agent_config_store::{is_conflict, Backend, Write};
use agent_core::{safe_segment, Error, Job, JobId, Result, Run, RunOutcome, Scheduler};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{
    claim_is_live, next_fire, parse, push_history, wall_clock_ms, RunObserver,
    DEFAULT_CLAIM_TTL_MS, MAX_DETAIL_CHARS,
};

/// The collection every scheduler job card lives in. `pub` so a tenant-fanning
/// driver (config C2c-2) can enumerate the tenants with jobs via
/// [`Backend::tenants`](agent_config_store::Backend::tenants) without duplicating
/// the literal.
pub const COLLECTION: &str = "scheduler";

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
    /// Which driver holds the live claim (its `owner` token); `None` when idle.
    /// `#[serde(default)]` so a card persisted before S1 still decodes.
    #[serde(default)]
    claimed_by: Option<String>,
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
    /// This driver's claim-owner token (S1). Stamped on a job when this instance
    /// wins its CAS claim, so a claim is attributable and a stale finisher can be
    /// told apart from the driver that reclaimed the job. A fresh per-instance id
    /// (no new dep — `uuid` is already the job-id source) so two drivers over one
    /// backend never share an owner.
    owner: String,
}

/// A fresh, collision-free claim-owner token for one `StoreScheduler` instance.
fn new_owner() -> String {
    format!("drv-{}", uuid::Uuid::new_v4().simple())
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
            owner: new_owner(),
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
            owner: new_owner(),
        })
    }

    /// Override this driver's claim-owner token (default: a fresh per-instance id).
    /// Useful for a stable, human-readable owner or for deterministic tests.
    pub fn with_owner(mut self, owner: impl Into<String>) -> Self {
        self.owner = owner.into();
        self
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

    /// Load every job card for this tenant, each paired with its **raw blob**
    /// (decoded, hostile blobs fail closed). The raw bytes are what a
    /// [`Write::CompareAndSwap`] claim conditions on — the exact value this driver
    /// read, so its claim lands only if no other driver has since rewritten the card.
    async fn load_raw(&self) -> Result<Vec<(StoredJob, Vec<u8>)>> {
        self.backend
            .list(COLLECTION, &self.tenant)
            .await?
            .into_iter()
            .map(|b| Ok((decode_job(&b)?, b)))
            .collect()
    }

    /// Load every job card for this tenant (decoded, hostile blobs fail closed).
    async fn load(&self) -> Result<Vec<StoredJob>> {
        Ok(self
            .load_raw()
            .await?
            .into_iter()
            .map(|(sj, _)| sj)
            .collect())
    }

    /// Persist `sj` only if its card still holds `prior` (the bytes we read) — the
    /// cross-driver claim/release primitive (S1). The idempotent `EnsureTenant`
    /// keeps the store's foreign-key check satisfied; the `CompareAndSwap` is what
    /// makes exactly one racing driver win. A conflict rolls back the whole batch.
    async fn cas_apply(&self, sj: &StoredJob, prior: Vec<u8>) -> Result<()> {
        let blob = encode_job(sj)?;
        self.backend
            .apply(&[
                Write::EnsureTenant {
                    tenant: self.tenant.clone(),
                },
                Write::CompareAndSwap {
                    collection: COLLECTION,
                    tenant: self.tenant.clone(),
                    id: sj.job.id.clone(),
                    expected: Some(prior),
                    blob,
                },
            ])
            .await
    }

    /// Jobs whose next fire has arrived, claimed for execution — the durable twin
    /// of `LocalScheduler::claim_due`. Returns `(id, goal)` for jobs that won a
    /// claim; records a visible `Skipped` run for each that lost to a live claim.
    ///
    /// Each claim (and each re-arm of a still-live job) is persisted as its **own**
    /// [`Write::CompareAndSwap`] conditioned on the exact bytes read, so when
    /// several drivers tick one backend at once exactly one wins each due job — the
    /// losers see an [`is_conflict`] error and skip it silently (no double-fire, no
    /// hard error). One CAS-apply per due job (rather than one big batch) so a lost
    /// race on one job never rolls back this driver's other valid claims; fine for a
    /// control-plane tick, where the due set per tick is small.
    pub async fn claim_due(&self, now: u64) -> Result<Vec<(JobId, String)>> {
        let mut due = Vec::new();
        let mut skipped: Vec<Run> = Vec::new();

        for (mut sj, prior) in self.load_raw().await? {
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
                    // Re-arm under CAS; if another driver already re-armed or claimed
                    // this job, its write is authoritative — skip silently.
                    match self.cas_apply(&sj, prior).await {
                        Ok(()) => skipped.push(r),
                        Err(e) if is_conflict(&e) => {}
                        Err(e) => return Err(e),
                    }
                    continue;
                }
            }
            sj.claimed_at_ms = Some(now);
            sj.claimed_by = Some(self.owner.clone());
            sj.job.next_fire_ms = next_fire(&sj.job.schedule, now);
            // Win the job only if our read is still current: exactly one driver's
            // CAS succeeds; a loser sees a conflict and does not add it to `due`.
            match self.cas_apply(&sj, prior).await {
                Ok(()) => due.push((sj.job.id.clone(), sj.job.goal.clone())),
                Err(e) if is_conflict(&e) => {}
                Err(e) => return Err(e),
            }
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
                // Release only a claim this driver still owns. If the run's claim was
                // TTL-reclaimed by another driver (which stamped its own owner), this
                // finish is stale — record nothing and leave the new claim intact,
                // rather than clobber the owner now re-running the job.
                if sj.claimed_by.as_deref() == Some(self.owner.as_str()) {
                    sj.claimed_at_ms = None;
                    sj.claimed_by = None;
                    push_history(&mut sj.history, run.clone());
                    if sj.job.next_fire_ms.is_none() {
                        sj.job.enabled = false;
                    }
                    // CAS guards the window between the read above and this write: if
                    // another driver reclaims in that gap, the conflict is a no-op.
                    match self.cas_apply(&sj, blob).await {
                        Ok(()) => {}
                        Err(e) if is_conflict(&e) => {}
                        Err(e) => return Err(e),
                    }
                }
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
            self.run_claimed(&id, goal, &exec).await?;
        }
        Ok(count)
    }

    /// Run one **already-claimed** job through `exec` and record its outcome,
    /// releasing the claim via [`finish`](Self::finish).
    ///
    /// Split out of [`tick_with`](Self::tick_with) so the multi-tenant driver
    /// (config C2c-2, scheduler **S2**) can claim across tenants with
    /// [`claim_due`](Self::claim_due) and then dispatch the claimed jobs with its
    /// own fan-out / fairness (a global concurrency ceiling + round-robin), while
    /// reusing the *identical* run-recording semantics (timing, outcome, the
    /// `MAX_DETAIL_CHARS` detail cap, and the CAS-guarded claim release). A failed
    /// `exec` is still recorded (as [`RunOutcome::Failed`]); the run is over either
    /// way.
    pub async fn run_claimed<F, Fut>(&self, id: &str, goal: String, exec: F) -> Result<()>
    where
        F: FnOnce(String) -> Fut,
        Fut: Future<Output = Result<String>>,
    {
        let started = self.now();
        let out = exec(goal).await;
        let finished = self.now();
        let (outcome, detail) = match out {
            Ok(answer) => (RunOutcome::Completed, answer),
            Err(e) => (RunOutcome::Failed, e.to_string()),
        };
        self.finish(
            id,
            Run {
                job_id: id.to_string(),
                started_ms: started,
                finished_ms: finished,
                outcome,
                detail: detail.chars().take(MAX_DETAIL_CHARS).collect(),
            },
        )
        .await
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
            claimed_by: None,
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

// Tenant isolation over a REAL Postgres server — the durable twin of the registry's
// `pg_tenant_tests` (config C2c-2). A job scheduled under one verified tenant is
// invisible to another over the shared store's `(collection, tenant, id)` keying,
// proven end to end over the tier `nix flake check` cannot host. `#[ignore]`-gated
// and run single-threaded by the `pg-integration` harness (which sets
// `AGENT_CONFIG_STORE_TEST_DSN`); dedicated tenants keep the run isolated.
#[cfg(all(test, feature = "scheduler-store-postgres"))]
mod pg_tests {
    use super::StoreScheduler;
    use agent_config_store::PgBackend;
    use agent_core::Scheduler;
    use std::sync::Arc;

    // desc (postgres, live): a job scheduled under tenant A is invisible to tenant
    // B's `list`, keyed entirely by tenant through the shared postgres store — the
    // durable multi-tenant isolation proof the tenant-fanning driver relies on.
    #[tokio::test]
    #[ignore = "needs a running postgres (nix run .#postgres-up) — run via `nix run .#integration`"]
    async fn positive_pg_two_tenants_isolated() {
        let dsn = std::env::var("AGENT_CONFIG_STORE_TEST_DSN")
            .expect("AGENT_CONFIG_STORE_TEST_DSN must be set by the pg-integration harness");
        let backend: Arc<dyn agent_config_store::Backend> = Arc::new(
            PgBackend::connect(&dsn, 4, true)
                .await
                .expect("connect postgres + ensure schema"),
        );
        // Dedicated tenants for this run; clean slate (idempotent across re-runs).
        const A: &str = "sched_tenant_it_a";
        const B: &str = "sched_tenant_it_b";
        for t in [A, B] {
            let s = StoreScheduler::with_tenant(backend.clone(), t).expect("tenant");
            for j in s.list().await.expect("list") {
                s.cancel(&j.id).await.expect("cleanup");
            }
        }
        let a = StoreScheduler::with_tenant(backend.clone(), A).expect("tenant a");
        let b = StoreScheduler::with_tenant(backend.clone(), B).expect("tenant b");
        a.schedule("every 3600s", "tenant-a recurring goal")
            .await
            .expect("schedule under A");
        let seen_a = a.list().await.expect("list A");
        let seen_b = b.list().await.expect("list B");
        assert_eq!(seen_a.len(), 1, "tenant A sees its own job");
        assert_eq!(seen_a[0].goal, "tenant-a recurring goal");
        assert!(seen_b.is_empty(), "tenant B must not see tenant A's job");
        // Cleanup.
        for j in a.list().await.expect("list A") {
            a.cancel(&j.id).await.expect("cleanup A");
        }
    }
}
