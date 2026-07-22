//! Recurring unattended agent runs behind the `Scheduler` seam (parity spec 28).
//!
//! Two things make unattended execution safe rather than merely possible, and
//! both are structural here rather than operational advice:
//!
//! * **The overlap guard.** A job firing every 60s that takes 5 minutes must not
//!   stack copies of itself. A fire is *claimed*; a fresh claim means the next
//!   fire is skipped (and recorded as skipped, so the drop is visible). A claim
//!   older than its TTL is reclaimable, so a crashed run cannot wedge the job
//!   forever — and a **future-dated** claim is treated as stale too, because
//!   clock skew must not produce a permanently un-runnable job.
//! * **A recorded outcome per run.** An unattended run that vanishes is
//!   unauditable. Every fire lands in a bounded history.
//!
//! The clock is injected everywhere. A scheduler that reads the system clock
//! internally can only be tested by sleeping, which is slow and flaky.

pub mod schedule;

use agent_core::{Error, Job, JobId, Result, Run, RunOutcome, Scheduler};
use async_trait::async_trait;
use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};

pub use schedule::{next_fire, parse};

/// Reported per fire, so the runtime can meter without this crate depending on
/// `agent-metrics`.
pub type RunObserver = Arc<dyn Fn(&Run) + Send + Sync>;

/// How long a claim stays valid before a crashed run's claim is reclaimable.
const DEFAULT_CLAIM_TTL_MS: u64 = 15 * 60 * 1_000;
/// Cap on retained history per job — an unbounded ledger is a slow leak.
const MAX_HISTORY: usize = 100;
/// Cap on a stored detail string.
const MAX_DETAIL_CHARS: usize = 2_000;

struct JobState {
    job: Job,
    /// When the in-flight run claimed this job (epoch ms); `None` if idle.
    claimed_at_ms: Option<u64>,
    history: Vec<Run>,
}

pub struct LocalScheduler {
    jobs: Mutex<HashMap<JobId, JobState>>,
    next_id: Mutex<u64>,
    now_ms: Arc<dyn Fn() -> u64 + Send + Sync>,
    observer: Option<RunObserver>,
    claim_ttl_ms: u64,
    /// Cap on jobs, since the model can create them.
    max_jobs: usize,
}

fn wall_clock_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl Default for LocalScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl LocalScheduler {
    pub fn new() -> Self {
        Self {
            jobs: Mutex::new(HashMap::new()),
            next_id: Mutex::new(1),
            now_ms: Arc::new(wall_clock_ms),
            observer: None,
            claim_ttl_ms: DEFAULT_CLAIM_TTL_MS,
            max_jobs: 64,
        }
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

    /// Is this claim still live? A claim in the FUTURE is treated as stale:
    /// clock skew (or a restored backup) must not make a job permanently
    /// un-runnable.
    fn claim_is_live(&self, claimed_at_ms: u64, now: u64) -> bool {
        if claimed_at_ms > now {
            return false;
        }
        now.saturating_sub(claimed_at_ms) < self.claim_ttl_ms
    }

    /// Jobs whose next fire has arrived, claimed for execution.
    ///
    /// Returns `(id, goal)` for jobs that won their claim, and records a
    /// `Skipped` run for each that lost — a dropped fire must be *visible*, not
    /// silent.
    fn claim_due(&self, now: u64) -> Vec<(JobId, String)> {
        let mut jobs = self.jobs.lock().expect("scheduler mutex");
        let mut due = Vec::new();
        let mut skipped = Vec::new();

        for st in jobs.values_mut() {
            if !st.job.enabled {
                continue;
            }
            let Some(next) = st.job.next_fire_ms else {
                continue; // spent one-shot
            };
            if next > now {
                continue;
            }
            if let Some(claimed) = st.claimed_at_ms {
                if self.claim_is_live(claimed, now) {
                    // Still running — drop this fire rather than stacking.
                    skipped.push(Run {
                        job_id: st.job.id.clone(),
                        started_ms: now,
                        finished_ms: now,
                        outcome: RunOutcome::Skipped,
                        detail: "previous run still in flight".into(),
                    });
                    // Re-arm so the job does not spin on the same due instant.
                    st.job.next_fire_ms = next_fire(&st.job.schedule, now);
                    continue;
                }
            }
            st.claimed_at_ms = Some(now);
            st.job.next_fire_ms = next_fire(&st.job.schedule, now);
            due.push((st.job.id.clone(), st.job.goal.clone()));
        }

        for r in skipped {
            if let Some(st) = jobs.get_mut(&r.job_id) {
                push_history(&mut st.history, r.clone());
            }
            if let Some(o) = &self.observer {
                o(&r);
            }
        }
        due
    }

    fn finish(&self, id: &str, run: Run) {
        let mut jobs = self.jobs.lock().expect("scheduler mutex");
        if let Some(st) = jobs.get_mut(id) {
            st.claimed_at_ms = None;
            push_history(&mut st.history, run.clone());
            // A spent one-shot is disabled rather than left armed.
            if st.job.next_fire_ms.is_none() {
                st.job.enabled = false;
            }
        }
        drop(jobs);
        if let Some(o) = &self.observer {
            o(&run);
        }
    }

    /// Fire every due job once, running each through `exec`. Returns how many
    /// ran.
    ///
    /// The executor is passed **per tick** rather than stored. Storing it would
    /// require the scheduler to own something that owns the agent — while the
    /// agent owns the scheduler through the `schedule` tool — so the cycle is
    /// avoided rather than worked around, and the closure needs no `'static`.
    ///
    /// Explicit rather than a background loop, so a caller (or a test) controls
    /// exactly when time advances.
    pub async fn tick_with<F, Fut>(&self, exec: F) -> usize
    where
        F: Fn(String) -> Fut,
        Fut: Future<Output = Result<String>>,
    {
        let now = self.now();
        let due = self.claim_due(now);
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
            );
        }
        count
    }
}

fn push_history(history: &mut Vec<Run>, r: Run) {
    history.push(r);
    if history.len() > MAX_HISTORY {
        let excess = history.len() - MAX_HISTORY;
        history.drain(0..excess);
    }
}

#[async_trait]
impl Scheduler for LocalScheduler {
    fn name(&self) -> &str {
        "local"
    }

    async fn schedule(&self, spec: &str, goal: &str) -> Result<JobId> {
        if goal.trim().is_empty() {
            return Err(Error::Scheduler("a scheduled job needs a goal".into()));
        }
        let now = self.now();
        let sched = parse(spec, now)?;
        let mut jobs = self.jobs.lock().expect("scheduler mutex");
        if jobs.len() >= self.max_jobs {
            return Err(Error::Scheduler(format!(
                "too many scheduled jobs (limit {})",
                self.max_jobs
            )));
        }
        let id = {
            let mut n = self.next_id.lock().expect("id mutex");
            let id = format!("job-{n}");
            *n += 1;
            id
        };
        // A one-shot already in the past never fires; say so now rather than
        // registering a job that silently does nothing.
        let next = next_fire(&sched, now);
        if next.is_none() {
            return Err(Error::Scheduler(
                "that one-shot time is already in the past".into(),
            ));
        }
        jobs.insert(
            id.clone(),
            JobState {
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
            },
        );
        Ok(id)
    }

    async fn list(&self) -> Result<Vec<Job>> {
        let jobs = self.jobs.lock().expect("scheduler mutex");
        let mut out: Vec<Job> = jobs.values().map(|s| s.job.clone()).collect();
        // Stable order, so `list` output is reproducible.
        out.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(out)
    }

    async fn cancel(&self, id: &str) -> Result<bool> {
        let mut jobs = self.jobs.lock().expect("scheduler mutex");
        Ok(jobs.remove(id).is_some())
    }

    async fn history(&self, id: &str) -> Result<Vec<Run>> {
        let jobs = self.jobs.lock().expect("scheduler mutex");
        Ok(jobs.get(id).map(|s| s.history.clone()).unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

    const T0: u64 = 1_704_067_200_000; // 2024-01-01T00:00:00Z

    /// An executor that counts calls and always succeeds.
    fn ok_exec(
        calls: Arc<AtomicUsize>,
    ) -> impl Fn(String) -> std::pin::Pin<Box<dyn Future<Output = Result<String>> + Send>> {
        move |_goal| {
            let c = calls.clone();
            Box::pin(async move {
                c.fetch_add(1, Ordering::SeqCst);
                Ok("done".to_string())
            })
        }
    }

    fn sched_at(clock: Arc<AtomicU64>) -> LocalScheduler {
        let c = clock.clone();
        LocalScheduler::new().with_clock(Arc::new(move || c.load(Ordering::SeqCst)))
    }

    #[tokio::test]
    async fn positive_due_job_runs_on_tick() {
        let clock = Arc::new(AtomicU64::new(T0));
        let calls = Arc::new(AtomicUsize::new(0));
        let s = sched_at(clock.clone());
        s.schedule("every 60s", "do a thing").await.unwrap();

        assert_eq!(s.tick_with(ok_exec(calls.clone())).await, 0, "not due yet");
        clock.store(T0 + 60_000, Ordering::SeqCst);
        assert_eq!(s.tick_with(ok_exec(calls.clone())).await, 1, "due");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn positive_history_records_the_outcome() {
        let clock = Arc::new(AtomicU64::new(T0));
        let calls = Arc::new(AtomicUsize::new(0));
        let s = sched_at(clock.clone());
        let id = s.schedule("every 60s", "g").await.unwrap();
        clock.store(T0 + 60_000, Ordering::SeqCst);
        s.tick_with(ok_exec(calls.clone())).await;

        let h = s.history(&id).await.unwrap();
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].outcome, RunOutcome::Completed);
        assert_eq!(h[0].detail, "done");
    }

    /// A failing run is recorded as failed, not silently dropped — an unattended
    /// failure that leaves no trace is the whole problem.
    #[tokio::test]
    async fn negative_failing_run_is_recorded() {
        let clock = Arc::new(AtomicU64::new(T0));
        let s = sched_at(clock.clone());
        let id = s.schedule("every 60s", "g").await.unwrap();
        clock.store(T0 + 60_000, Ordering::SeqCst);
        s.tick_with(|_g| async { Err(Error::Scheduler("boom".into())) })
            .await;

        let h = s.history(&id).await.unwrap();
        assert_eq!(h[0].outcome, RunOutcome::Failed);
        assert!(h[0].detail.contains("boom"));
    }

    /// The headline guard: a job whose previous run is still in flight must not
    /// stack a second copy.
    #[tokio::test]
    async fn positive_overlapping_fire_is_skipped_not_stacked() {
        let clock = Arc::new(AtomicU64::new(T0));
        let s = sched_at(clock.clone());
        let id = s.schedule("every 60s", "g").await.unwrap();

        // Simulate an in-flight run by claiming without finishing.
        clock.store(T0 + 60_000, Ordering::SeqCst);
        let due = s.claim_due(T0 + 60_000);
        assert_eq!(due.len(), 1, "first fire wins the claim");

        // The next fire, while the claim is live, must be skipped.
        clock.store(T0 + 120_000, Ordering::SeqCst);
        let due2 = s.claim_due(T0 + 120_000);
        assert!(due2.is_empty(), "a second copy must not start");

        let h = s.history(&id).await.unwrap();
        assert_eq!(h.len(), 1);
        assert_eq!(
            h[0].outcome,
            RunOutcome::Skipped,
            "the drop must be visible"
        );
    }

    /// A crashed run holds its claim forever; the TTL is what stops that wedging
    /// the job permanently.
    #[tokio::test]
    async fn positive_stale_claim_is_reclaimed_after_the_ttl() {
        let clock = Arc::new(AtomicU64::new(T0));
        let s = sched_at(clock.clone()).with_claim_ttl_ms(10_000);
        s.schedule("every 60s", "g").await.unwrap();

        clock.store(T0 + 60_000, Ordering::SeqCst);
        assert_eq!(s.claim_due(T0 + 60_000).len(), 1);
        // Well past the TTL, with the claim never released (a crash).
        let later = T0 + 60_000 + 999_000;
        clock.store(later, Ordering::SeqCst);
        assert_eq!(
            s.claim_due(later).len(),
            1,
            "a dead run must not wedge the job forever"
        );
    }

    /// Clock skew (or a restored backup) can produce a claim dated in the
    /// future. Treating that as live would make the job permanently un-runnable.
    #[tokio::test]
    async fn adversarial_future_dated_claim_is_treated_as_stale() {
        let clock = Arc::new(AtomicU64::new(T0));
        let s = sched_at(clock.clone());
        assert!(
            !s.claim_is_live(T0 + 999_999, T0),
            "a future claim must not block execution"
        );
    }

    /// A one-shot runs once and then stops being armed.
    #[tokio::test]
    async fn boundary_once_runs_exactly_once() {
        let clock = Arc::new(AtomicU64::new(T0));
        let calls = Arc::new(AtomicUsize::new(0));
        let s = sched_at(clock.clone());
        s.schedule("in 60s", "g").await.unwrap();

        clock.store(T0 + 60_000, Ordering::SeqCst);
        s.tick_with(ok_exec(calls.clone())).await;
        clock.store(T0 + 600_000, Ordering::SeqCst);
        s.tick_with(ok_exec(calls.clone())).await;
        assert_eq!(calls.load(Ordering::SeqCst), 1, "a one-shot fired twice");
    }

    #[tokio::test]
    async fn positive_cancel_removes_the_job() {
        let clock = Arc::new(AtomicU64::new(T0));
        let calls = Arc::new(AtomicUsize::new(0));
        let s = sched_at(clock.clone());
        let id = s.schedule("every 60s", "g").await.unwrap();
        assert!(s.cancel(&id).await.unwrap());
        assert!(!s.cancel(&id).await.unwrap(), "cancelling twice is false");
        clock.store(T0 + 60_000, Ordering::SeqCst);
        s.tick_with(ok_exec(calls.clone())).await;
        assert_eq!(calls.load(Ordering::SeqCst), 0, "a cancelled job ran");
    }

    /// The model can create jobs, so the count must be bounded.
    #[tokio::test]
    async fn adversarial_job_count_is_bounded() {
        let clock = Arc::new(AtomicU64::new(T0));
        let s = sched_at(clock).with_max_jobs(3);
        for _ in 0..3 {
            s.schedule("every 60s", "g").await.unwrap();
        }
        assert!(
            s.schedule("every 60s", "g").await.is_err(),
            "unbounded jobs"
        );
    }

    /// History must not grow without bound.
    #[tokio::test]
    async fn adversarial_history_is_bounded() {
        let clock = Arc::new(AtomicU64::new(T0));
        let calls = Arc::new(AtomicUsize::new(0));
        let s = sched_at(clock.clone());
        let id = s.schedule("every 1s", "g").await.unwrap();
        for i in 1..=(MAX_HISTORY as u64 + 20) {
            clock.store(T0 + i * 1_000, Ordering::SeqCst);
            s.tick_with(ok_exec(calls.clone())).await;
        }
        assert!(
            s.history(&id).await.unwrap().len() <= MAX_HISTORY,
            "history grew unbounded"
        );
    }

    #[rstest]
    #[case::negative_empty_goal("every 60s", "")]
    #[case::negative_bad_spec("nonsense", "g")]
    #[case::negative_past_one_shot("once: 1", "g")]
    #[tokio::test]
    async fn negative_bad_schedule_requests_are_rejected(#[case] spec: &str, #[case] goal: &str) {
        let clock = Arc::new(AtomicU64::new(T0));
        let s = sched_at(clock);
        assert!(s.schedule(spec, goal).await.is_err());
    }

    /// Regression: a cron job re-armed at an instant its expression matches must
    /// NOT become immediately due again. With "at or after" next-fire semantics
    /// this spun in a hot loop, firing continuously without the clock moving.
    #[tokio::test]
    async fn adversarial_cron_job_does_not_spin_when_rearmed_on_a_match() {
        // 00:00 exactly — matches `0 * * * *`.
        let clock = Arc::new(AtomicU64::new(T0));
        let calls = Arc::new(AtomicUsize::new(0));
        let s = sched_at(clock.clone());
        s.schedule("cron: 0 * * * *", "g").await.unwrap();

        // Advance to the top of the next hour and fire once.
        clock.store(T0 + 3_600_000, Ordering::SeqCst);
        assert_eq!(s.tick_with(ok_exec(calls.clone())).await, 1);
        // Ticking again at the SAME instant must do nothing.
        for _ in 0..5 {
            assert_eq!(
                s.tick_with(ok_exec(calls.clone())).await,
                0,
                "job re-fired without time advancing"
            );
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    /// `list` must be reproducible.
    #[tokio::test]
    async fn positive_list_is_stably_ordered() {
        let clock = Arc::new(AtomicU64::new(T0));
        let s = sched_at(clock);
        for _ in 0..5 {
            s.schedule("every 60s", "g").await.unwrap();
        }
        let a: Vec<String> = s.list().await.unwrap().into_iter().map(|j| j.id).collect();
        let b: Vec<String> = s.list().await.unwrap().into_iter().map(|j| j.id).collect();
        assert_eq!(a, b);
    }
}
