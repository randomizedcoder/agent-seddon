//! T11 — the driver tick (`06-test-matrix.md`), over `MemCampaigns` under a
//! recording wrapper, a counting planner, a recording poller and closure execs.
//! `boundary_config_floor` / `boundary_config_ceiling` live with `CampaignCfg` in
//! `agent-runtime`; `adversarial_owner_from_env_missing` is a `cli_e2e` row over the
//! `--run-task` stub (`run_task_stub` unit rows in `agent-cli`).

use super::*;
use agent_core::campaign::{
    Actor, AttemptKind, AttemptOutcome, CampaignError, Complete, Decomposed, Decomposition,
    ListFilter, MarkLeaf, NewCampaign, PlanClose, PlanStart, Reaped, ReviewOutcome, TaskAttempt,
    TaskEvent,
};
use agent_testkit::campaign::conformance::{
    campaign, children, failed, leaf, owner, pr, ready_leaves, split, split_with,
};
use agent_testkit::campaign::MemCampaigns;
use rstest::rstest;

type Log = Arc<Mutex<Vec<(String, &'static str)>>>;

/// The store methods that are tick phases, in the order the design lists them.
const PHASES: [&str; 5] = ["reap", "reap_decomposing", "poll", "plannable", "claim"];

/// A store double that delegates every seam method to the memory tier and logs
/// the phase methods per tenant; `fail_claim` makes `claim` a backend error
/// (`adversarial_store_error_mid_tick`).
struct Recording {
    inner: Arc<dyn CampaignStore>,
    log: Log,
    fail_claim: bool,
}

impl Recording {
    fn note(&self, method: &'static str) {
        self.log
            .lock()
            .unwrap()
            .push((self.inner.tenant().to_string(), method));
    }
}

#[async_trait]
impl CampaignStore for Recording {
    fn tenant(&self) -> &str {
        self.inner.tenant()
    }
    async fn create(&self, req: NewCampaign, actor: &Actor) -> CampaignResult<Task> {
        self.inner.create(req, actor).await
    }
    async fn plan_start(&self, task: TaskId) -> CampaignResult<PlanStart> {
        self.inner.plan_start(task).await
    }
    async fn decompose(&self, req: Decomposition) -> CampaignResult<Decomposed> {
        self.inner.decompose(req).await
    }
    async fn mark_leaf(&self, req: MarkLeaf) -> CampaignResult<Task> {
        self.inner.mark_leaf(req).await
    }
    async fn plan_close(&self, req: PlanClose) -> CampaignResult<Task> {
        self.inner.plan_close(req).await
    }
    async fn claim(&self, req: ClaimRequest) -> CampaignResult<Vec<Claimed>> {
        self.note("claim");
        if self.fail_claim {
            return Err(CampaignError::Backend("scripted claim failure".into()));
        }
        self.inner.claim(req).await
    }
    async fn heartbeat(&self, task: TaskId, owner: &Owner, lease_secs: i64) -> CampaignResult<()> {
        self.inner.heartbeat(task, owner, lease_secs).await
    }
    async fn reap(&self) -> CampaignResult<Vec<Reaped>> {
        self.note("reap");
        self.inner.reap().await
    }
    async fn reap_decomposing(&self, max_age_secs: i64) -> CampaignResult<Vec<TaskId>> {
        self.note("reap_decomposing");
        self.inner.reap_decomposing(max_age_secs).await
    }
    async fn start(&self, task: TaskId, owner: &Owner) -> CampaignResult<Task> {
        self.inner.start(task, owner).await
    }
    async fn complete(&self, req: Complete) -> CampaignResult<Task> {
        self.inner.complete(req).await
    }
    async fn fail(&self, req: Fail) -> CampaignResult<Task> {
        self.inner.fail(req).await
    }
    async fn resolve_review(&self, task: TaskId, outcome: ReviewOutcome) -> CampaignResult<Task> {
        self.inner.resolve_review(task, outcome).await
    }
    async fn review_note(
        &self,
        task: TaskId,
        note: agent_core::campaign::ReviewNote,
    ) -> CampaignResult<bool> {
        self.inner.review_note(task, note).await
    }
    async fn approve(
        &self,
        task: TaskId,
        expected_version: u64,
        actor: &Actor,
    ) -> CampaignResult<Task> {
        self.inner.approve(task, expected_version, actor).await
    }
    async fn approve_children(&self, parent: TaskId, actor: &Actor) -> CampaignResult<Vec<Task>> {
        self.inner.approve_children(parent, actor).await
    }
    async fn answer(
        &self,
        task: TaskId,
        expected_version: u64,
        text: String,
        actor: &Actor,
    ) -> CampaignResult<Task> {
        self.inner.answer(task, expected_version, text, actor).await
    }
    async fn retry(&self, task: TaskId, actor: &Actor) -> CampaignResult<Task> {
        self.inner.retry(task, actor).await
    }
    async fn update_policy(
        &self,
        campaign: TaskId,
        policy: Policy,
        actor: &Actor,
    ) -> CampaignResult<Task> {
        self.inner.update_policy(campaign, policy, actor).await
    }
    async fn cancel(&self, task: TaskId, actor: &Actor) -> CampaignResult<Vec<Task>> {
        self.inner.cancel(task, actor).await
    }
    async fn replan(&self, task: TaskId, actor: &Actor) -> CampaignResult<Task> {
        self.inner.replan(task, actor).await
    }
    async fn get(&self, task: TaskId) -> CampaignResult<Task> {
        self.inner.get(task).await
    }
    async fn list_campaigns(&self, filter: ListFilter) -> CampaignResult<Vec<Task>> {
        self.inner.list_campaigns(filter).await
    }
    async fn subtree(&self, node: TaskId) -> CampaignResult<Vec<Task>> {
        self.inner.subtree(node).await
    }
    async fn children(&self, parent: TaskId) -> CampaignResult<Vec<Task>> {
        self.inner.children(parent).await
    }
    async fn events(&self, task: TaskId) -> CampaignResult<Vec<TaskEvent>> {
        self.inner.events(task).await
    }
    async fn attempts(&self, task: TaskId) -> CampaignResult<Vec<TaskAttempt>> {
        self.inner.attempts(task).await
    }
    async fn plannable(&self, limit: usize) -> CampaignResult<Vec<Task>> {
        self.note("plannable");
        self.inner.plannable(limit).await
    }
    async fn in_review(&self, limit: usize) -> CampaignResult<Vec<Task>> {
        self.inner.in_review(limit).await
    }
}

/// The backend double: the memory tier, every opened store wrapped in
/// [`Recording`]; `tenants()` is logged under `*`.
struct RecordingBackend {
    inner: MemCampaigns,
    log: Log,
    fail_claim_for: Option<String>,
}

#[async_trait]
impl CampaignBackend for RecordingBackend {
    async fn tenants(&self) -> CampaignResult<Vec<String>> {
        self.log.lock().unwrap().push(("*".to_string(), "tenants"));
        CampaignBackend::tenants(&self.inner).await
    }
    fn with_tenant(&self, tenant: &str) -> CampaignResult<Arc<dyn CampaignStore>> {
        let inner = CampaignBackend::with_tenant(&self.inner, tenant)?;
        Ok(Arc::new(Recording {
            inner,
            log: Arc::clone(&self.log),
            fail_claim: self.fail_claim_for.as_deref() == Some(tenant),
        }))
    }
}

/// A poller that only records that it ran, in the same log as the store phases.
struct RecordingPoller(Log);

#[async_trait]
impl PrPoller for RecordingPoller {
    async fn poll(&self, store: Arc<dyn CampaignStore>, _batch: usize) -> PollReport {
        self.0
            .lock()
            .unwrap()
            .push((store.tenant().to_string(), "poll"));
        PollReport::default()
    }
}

/// A planner that records the limit it was given and splits every plannable node
/// into one child (a real store write, so `decomposed` counts are observable).
#[derive(Default)]
struct CountingPlanner {
    limits: Mutex<Vec<usize>>,
}

#[async_trait]
impl TickPlanner for CountingPlanner {
    async fn tick(&self, store: Arc<dyn CampaignStore>, limit: usize) -> PlanReport {
        self.limits.lock().unwrap().push(limit);
        let queue = store.plannable(limit).await.expect("plannable");
        let mut report = PlanReport {
            summary: TickSummary {
                selected: queue.len(),
                ..TickSummary::default()
            },
            nodes: Vec::new(),
            model: "counting".into(),
        };
        for t in queue {
            let d = split(&*store, t.task_id, 1).await;
            report.summary.split += 1;
            let planned = Planned {
                task: t.task_id,
                outcome: crate::PlanOutcome::Split {
                    parent: d.parent,
                    children: 1,
                    low_confidence: false,
                },
                calls: 0,
                repairs: 0,
                tokens: TokenUsage::default(),
                prompt_hash: None,
            };
            report.nodes.push((t, Ok(planned)));
        }
        report
    }
}

/// The fixture: one memory tier, its recording backend, the shared log.
struct Fx {
    mem: MemCampaigns,
    backend: Arc<RecordingBackend>,
    log: Log,
}

impl Fx {
    fn new() -> Self {
        Self::with_failing_claim(None)
    }

    fn with_failing_claim(tenant: Option<&str>) -> Self {
        let mem = MemCampaigns::new();
        let log: Log = Arc::new(Mutex::new(Vec::new()));
        let backend = Arc::new(RecordingBackend {
            inner: mem.clone(),
            log: Arc::clone(&log),
            fail_claim_for: tenant.map(str::to_string),
        });
        Self { mem, backend, log }
    }

    /// A direct (unlogged) handle for seeding and inspecting a tenant.
    fn store(&self, tenant: &str) -> Arc<dyn CampaignStore> {
        Arc::new(self.mem.with_tenant(tenant).expect("safe tenant"))
    }

    fn driver(&self, tenants: Tenants, cfg: DriverConfig) -> Driver {
        Driver::new(
            Arc::clone(&self.backend) as Arc<dyn CampaignBackend>,
            tenants,
            cfg,
            Arc::new(CountingPlanner::default()),
        )
    }

    /// The phase methods the tick called on `tenant`, in order.
    fn phases(&self, tenant: &str) -> Vec<&'static str> {
        self.log
            .lock()
            .unwrap()
            .iter()
            .filter(|(t, m)| t == tenant && PHASES.contains(m))
            .map(|(_, m)| *m)
            .collect()
    }

    fn log_entries(&self) -> Vec<(String, &'static str)> {
        self.log.lock().unwrap().clone()
    }
}

/// T13 `negative_noop_poller_no_store_calls`: the [`NoopPoller`] never touches
/// the store, even with an `in_review` leaf waiting.
#[tokio::test]
async fn negative_noop_poller_no_store_calls() {
    let fx = Fx::new();
    let seed = fx.store("ta");
    let (_, leaves) = ready_leaves(&*seed, 1).await;
    agent_testkit::campaign::conformance::in_review(&*seed, leaves[0].task_id, &owner("w1"), 1)
        .await;
    let logged = fx.backend.with_tenant("ta").expect("open");
    let report = NoopPoller.poll(logged, 20).await;
    assert_eq!(report, PollReport::default());
    assert!(fx.log_entries().is_empty(), "{:?}", fx.log_entries());
}

fn cfg(per_tenant_workers: usize, global_workers: usize) -> DriverConfig {
    DriverConfig {
        per_tenant_workers,
        global_workers,
        worker_timeout: Duration::from_secs(60),
        ..DriverConfig::default()
    }
}

fn ok_exec() -> Option<Arc<dyn WorkerExec>> {
    Some(Arc::new(ClosureExec(|_, _, _, _| async { Ok(()) })))
}

fn pending_exec() -> Option<Arc<dyn WorkerExec>> {
    Some(Arc::new(ClosureExec(|_, _, _, _| std::future::pending())))
}

/// Starts the leaf (`claimed → running`) then hangs, like a worker mid-session.
fn start_then_pend() -> Option<Arc<dyn WorkerExec>> {
    Some(Arc::new(ClosureExec(
        |_, store: Arc<dyn CampaignStore>, claimed: Claimed, owner: Owner| async move {
            store
                .start(claimed.task.task_id, &owner)
                .await
                .map_err(|e| e.to_string())?;
            std::future::pending::<()>().await;
            Ok(())
        },
    )))
}

fn panic_exec() -> Option<Arc<dyn WorkerExec>> {
    Some(Arc::new(ClosureExec(|_, _, _, _| async {
        panic!("boom");
        #[allow(unreachable_code)]
        Ok(())
    })))
}

/// Let the spawned workers run their first steps (single-threaded runtime).
async fn settle() {
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
}

async fn states(store: &dyn CampaignStore, leaves: &[Task]) -> Vec<TaskState> {
    let mut out = Vec::new();
    for l in leaves {
        out.push(store.get(l.task_id).await.unwrap().state);
    }
    out
}

async fn work_attempts(store: &dyn CampaignStore, id: TaskId) -> Vec<TaskAttempt> {
    store
        .attempts(id)
        .await
        .unwrap()
        .into_iter()
        .filter(|a| a.kind == AttemptKind::Work)
        .collect()
}

fn tenants(list: &[&str]) -> Tenants {
    Tenants::Fixed(list.iter().map(|s| (*s).to_string()).collect())
}

/// Starts and completes the leaf with a known spend, like a worker that opened
/// PR 7 (`TokenUsage::new(60, 40)`), so the settled report has an attempt to read.
fn complete_exec() -> Option<Arc<dyn WorkerExec>> {
    Some(Arc::new(ClosureExec(
        |_, store: Arc<dyn CampaignStore>, claimed: Claimed, owner: Owner| async move {
            store
                .start(claimed.task.task_id, &owner)
                .await
                .map_err(|e| e.to_string())?;
            store
                .complete(Complete {
                    task: claimed.task.task_id,
                    owner,
                    pr: pr(7),
                    tokens: TokenUsage::new(60, 40),
                    session_id: Some("s1".into()),
                })
                .await
                .map_err(|e| e.to_string())?;
            Ok(())
        },
    )))
}

/// A [`TickObserver`] that keeps every report it was handed (CP-08 T11 rows).
#[derive(Default)]
struct RecordingObserver {
    ticks: Mutex<Vec<(TickReport, Duration)>>,
    drains: Mutex<Vec<DrainReport>>,
}

impl TickObserver for RecordingObserver {
    fn on_tick(&self, report: &TickReport, elapsed: Duration) {
        self.ticks.lock().unwrap().push((report.clone(), elapsed));
    }
    fn on_drain(&self, report: &DrainReport) {
        self.drains.lock().unwrap().push(report.clone());
    }
}

// -- T11 rows ------------------------------------------------------------------

/// The observer is handed the tick's report (the same value the caller gets, the
/// planner's model label included) with its wall time, and the drain's report
/// with every settled worker.
#[tokio::test]
async fn positive_observer_sees_tick_and_elapsed() {
    let fx = Fx::new();
    let a = fx.store("ta");
    let (_, leaves) = ready_leaves(&*a, 1).await;
    let root = campaign(&*a, "to plan").await;
    let obs = Arc::new(RecordingObserver::default());
    let driver = fx
        .driver(tenants(&["ta"]), cfg(2, 8))
        .with_exec(ok_exec())
        .with_observer(Arc::clone(&obs) as Arc<dyn TickObserver>);
    let report = driver.tick().await;
    {
        let ticks = obs.ticks.lock().unwrap();
        assert_eq!(ticks.len(), 1);
        assert_eq!(ticks[0].0, report);
        assert_eq!(
            ticks[0].0.dispatched,
            vec![("ta".to_string(), leaves[0].task_id)]
        );
        let plan = ticks[0].0.per_tenant[0].plan.as_ref().unwrap();
        assert_eq!(plan.model, "counting");
        assert!(plan.nodes.iter().any(|(t, _)| t.task_id == root.task_id));
    }
    let drained = driver.drain(Duration::from_secs(5)).await;
    let drains = obs.drains.lock().unwrap();
    assert_eq!(drains.len(), 1);
    assert_eq!(drains[0], drained);
    assert_eq!(drains[0].settled.len(), 1);
    assert_eq!(drains[0].settled[0].outcome, WorkerOutcome::Ok);
    assert_eq!(obs.ticks.lock().unwrap().len(), 1, "a drain is not a tick");
}

/// A disabled driver reports nothing to its observer (nothing ran).
#[tokio::test]
async fn corner_observer_silent_when_disabled() {
    let fx = Fx::new();
    let a = fx.store("ta");
    ready_leaves(&*a, 1).await;
    let obs = Arc::new(RecordingObserver::default());
    let driver = fx
        .driver(
            tenants(&["ta"]),
            DriverConfig {
                enabled: false,
                ..cfg(2, 8)
            },
        )
        .with_exec(ok_exec())
        .with_observer(Arc::clone(&obs) as Arc<dyn TickObserver>);
    assert!(driver.tick().await.disabled);
    assert!(obs.ticks.lock().unwrap().is_empty());
    let drained = driver.drain(Duration::from_secs(1)).await;
    assert_eq!(drained, DrainReport::default());
    assert_eq!(obs.drains.lock().unwrap().len(), 1, "a drain still reports");
}

/// The settled report carries the spend and model the worker closed its attempt
/// with — read back from the store, so a subprocess worker's numbers count too.
#[tokio::test]
async fn positive_settled_carries_work_attempt() {
    let fx = Fx::new();
    let a = fx.store("ta");
    let (_, leaves) = ready_leaves(&*a, 1).await;
    let driver = fx
        .driver(tenants(&["ta"]), cfg(2, 8))
        .with_exec(complete_exec());
    driver.tick().await;
    let drained = driver.drain(Duration::from_secs(5)).await;
    assert_eq!(drained.settled.len(), 1);
    let s = &drained.settled[0];
    assert_eq!(s.outcome, WorkerOutcome::Ok);
    assert_eq!(s.tokens, TokenUsage::new(60, 40));
    let attempts = work_attempts(&*a, leaves[0].task_id).await;
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].outcome, AttemptOutcome::Pr);
    assert_eq!(s.model, attempts[0].model);
    assert_eq!(a.get(s.task).await.unwrap().state, TaskState::InReview);
}

/// No `work` attempt to read (only a planner attempt, or no task at all) is a
/// zero spend and an empty label — never an error after the leaf is settled.
#[tokio::test]
async fn corner_settled_without_attempt_zero() {
    let fx = Fx::new();
    let a = fx.store("ta");
    let root = campaign(&*a, "planned only").await;
    split(&*a, root.task_id, 1).await;
    assert!(!a.attempts(root.task_id).await.unwrap().is_empty());
    assert_eq!(
        latest_work_attempt(&*a, root.task_id).await,
        (TokenUsage::default(), String::new())
    );
    assert_eq!(
        latest_work_attempt(&*a, TaskId(999_999)).await,
        (TokenUsage::default(), String::new())
    );
}

/// Per tenant: reap, reap_decomposing, poll, plan (`plannable`), claim — in that order.
#[tokio::test]
async fn positive_phase_order() {
    let fx = Fx::new();
    for t in ["ta", "tb"] {
        ready_leaves(&*fx.store(t), 1).await;
    }
    let driver = fx
        .driver(tenants(&["ta", "tb"]), cfg(2, 4))
        .with_poller(Arc::new(RecordingPoller(Arc::clone(&fx.log))))
        .with_exec(ok_exec());
    let report = driver.tick().await;
    assert!(!report.disabled);
    assert_eq!(report.tenants, ["ta", "tb"]);
    for t in ["ta", "tb"] {
        assert_eq!(fx.phases(t), PHASES, "{t}");
    }
    assert_eq!(report.dispatched.len(), 2);
    assert_eq!(report.errors, 0);
    let drained = driver.drain(Duration::from_secs(5)).await;
    assert_eq!(drained.aborted, 0);
    assert_eq!(drained.settled.len(), 2);
    assert!(drained
        .settled
        .iter()
        .all(|s| s.outcome == WorkerOutcome::Ok));
}

/// Two tenants with deep queues (ten leaves each, over two campaigns: a root
/// holds at most eight children) under a global cap of 4: dispatch alternates.
#[tokio::test]
async fn positive_round_robin() {
    let fx = Fx::new();
    for t in ["ta", "tb"] {
        for _ in 0..2 {
            ready_leaves(&*fx.store(t), 5).await;
        }
    }
    let driver = fx
        .driver(tenants(&["ta", "tb"]), cfg(2, 4))
        .with_exec(pending_exec());
    let report = driver.tick().await;
    let order: Vec<&str> = report.dispatched.iter().map(|(t, _)| t.as_str()).collect();
    assert_eq!(order, ["ta", "tb", "ta", "tb"]);
    assert_eq!(report.claimed(), 4);
    let drained = driver.drain(Duration::ZERO).await;
    assert_eq!(drained.aborted, 4);
}

/// Three tenants, three ticks: the starting tenant rotates.
#[tokio::test]
async fn positive_rotated_start() {
    let fx = Fx::new();
    let driver = fx.driver(tenants(&["ta", "tb", "tc"]), cfg(2, 4));
    let mut firsts = Vec::new();
    for _ in 0..3 {
        firsts.push(driver.tick().await.tenants[0].clone());
    }
    assert_eq!(firsts, ["ta", "tb", "tc"]);
    // The fourth wraps around.
    assert_eq!(driver.tick().await.tenants, ["ta", "tb", "tc"]);
}

/// Two drivers mint different owners, each 32 lowercase hex chars.
#[tokio::test]
async fn positive_owner_per_process() {
    let fx = Fx::new();
    let a = fx.driver(tenants(&["ta"]), cfg(2, 4));
    let b = fx.driver(tenants(&["ta"]), cfg(2, 4));
    assert_ne!(a.owner(), b.owner());
    for d in [&a, &b] {
        let o = d.owner().as_str();
        assert_eq!(o.len(), 32);
        assert!(o
            .bytes()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }
}

#[test]
fn positive_mint_owner_hex() {
    let a = mint_owner();
    let b = mint_owner();
    assert_ne!(a, b);
    assert_eq!(a.as_str().len(), 32);
    assert!(a
        .as_str()
        .bytes()
        .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)));
}

/// `enabled = false`: the report says so and nothing is called, not even discovery.
#[tokio::test]
async fn negative_disabled() {
    let fx = Fx::new();
    ready_leaves(&*fx.store("ta"), 1).await;
    let driver = fx
        .driver(
            Tenants::Discover,
            DriverConfig {
                enabled: false,
                ..cfg(2, 4)
            },
        )
        .with_exec(ok_exec());
    let report = driver.tick().await;
    assert!(report.disabled);
    assert!(report.tenants.is_empty() && report.dispatched.is_empty());
    assert!(fx.log_entries().is_empty(), "{:?}", fx.log_entries());
    assert_eq!(
        fx.store("ta").get(TaskId(1)).await.unwrap().state,
        TaskState::Decomposed
    );
}

/// No tenants (a fixed empty list, or discovery over an empty tier): a no-op.
#[tokio::test]
async fn corner_no_tenants() {
    let fx = Fx::new();
    let fixed = fx
        .driver(Tenants::Fixed(vec![]), cfg(2, 4))
        .with_exec(ok_exec());
    let report = fixed.tick().await;
    assert!(report.tenants.is_empty() && report.per_tenant.is_empty());
    assert!(fx.log_entries().is_empty());
    let discover = fx.driver(Tenants::Discover, cfg(2, 4)).with_exec(ok_exec());
    let report = discover.tick().await;
    assert!(report.tenants.is_empty());
    assert_eq!(fx.log_entries(), vec![("*".to_string(), "tenants")]);
}

/// A tenant with only `failed` / `blocked` nodes claims nothing and does not stop
/// the next tenant; discovery does not even list it.
#[tokio::test]
async fn corner_tenant_all_blocked() {
    let fx = Fx::new();
    let a = fx.store("ta");
    let root = campaign(&*a, "stuck").await;
    let mut specs = children(2);
    specs[1].depends_on = vec![1];
    let d = split_with(&*a, root.task_id, specs, 71).await;
    let first = leaf(&*a, d.children[0].task_id).await;
    let second = leaf(&*a, d.children[1].task_id).await;
    failed(&*a, first.task_id).await;
    assert_eq!(
        states(&*a, &[first, second]).await,
        [TaskState::Failed, TaskState::Blocked]
    );
    assert_eq!(a.get(root.task_id).await.unwrap().state, TaskState::Blocked);
    ready_leaves(&*fx.store("tb"), 1).await;

    let driver = fx
        .driver(tenants(&["ta", "tb"]), cfg(2, 4))
        .with_exec(ok_exec());
    let report = driver.tick().await;
    assert_eq!(report.per_tenant[0].tenant, "ta");
    assert_eq!(report.per_tenant[0].claimed, 0);
    assert_eq!(report.per_tenant[1].tenant, "tb");
    assert_eq!(report.per_tenant[1].claimed, 1);
    assert_eq!(report.dispatched.len(), 1);
    driver.drain(Duration::from_secs(5)).await;
    assert_eq!(
        CampaignBackend::tenants(&fx.mem).await.unwrap(),
        vec!["tb".to_string()]
    );
}

/// Five `in_review` leaves hold no worker: two new claims are still made.
#[tokio::test]
async fn corner_in_review_not_counted() {
    let fx = Fx::new();
    let a = fx.store("ta");
    let (_, leaves) = ready_leaves(&*a, 7).await;
    let w = owner("w1");
    let held = a
        .claim(ClaimRequest {
            owner: w.clone(),
            limit: 5,
            lease_secs: 600,
        })
        .await
        .unwrap();
    assert_eq!(held.len(), 5);
    for (i, c) in held.iter().enumerate() {
        a.start(c.task.task_id, &w).await.unwrap();
        a.complete(Complete {
            task: c.task.task_id,
            owner: w.clone(),
            pr: pr(i as i64 + 1),
            tokens: TokenUsage::default(),
            session_id: None,
        })
        .await
        .unwrap();
    }
    assert_eq!(a.in_review(10).await.unwrap().len(), 5);
    let driver = fx.driver(tenants(&["ta"]), cfg(2, 8)).with_exec(ok_exec());
    let report = driver.tick().await;
    assert_eq!(report.claimed(), 2);
    let now = states(&*a, &leaves).await;
    assert_eq!(now.iter().filter(|s| **s == TaskState::Claimed).count(), 2);
    driver.drain(Duration::from_secs(5)).await;
}

/// `per_tenant_workers 2` over five leaves: two run, three stay `ready`.
#[tokio::test]
async fn boundary_per_tenant_workers() {
    let fx = Fx::new();
    let a = fx.store("ta");
    let (_, leaves) = ready_leaves(&*a, 5).await;
    let driver = fx
        .driver(tenants(&["ta"]), cfg(2, 8))
        .with_exec(start_then_pend());
    let report = driver.tick().await;
    assert_eq!(report.dispatched.len(), 2);
    settle().await;
    let now = states(&*a, &leaves).await;
    assert_eq!(now.iter().filter(|s| **s == TaskState::Running).count(), 2);
    assert_eq!(now.iter().filter(|s| **s == TaskState::Ready).count(), 3);
    // A second tick claims nothing more: both per-tenant permits are held.
    let again = driver.tick().await;
    assert_eq!(again.claimed(), 0);
    assert_eq!(driver.drain(Duration::ZERO).await.aborted, 2);
}

/// Three tenants of two under `global_workers 4`: exactly four in flight, the
/// semaphore at zero, and every permit back after the drain.
#[tokio::test]
async fn boundary_global_workers() {
    let fx = Fx::new();
    for t in ["ta", "tb", "tc"] {
        ready_leaves(&*fx.store(t), 2).await;
    }
    let driver = fx
        .driver(tenants(&["ta", "tb", "tc"]), cfg(2, 4))
        .with_exec(pending_exec());
    assert_eq!(driver.global_available(), 4);
    let report = driver.tick().await;
    assert_eq!(report.dispatched.len(), 4);
    assert_eq!(driver.global_available(), 0);
    assert_eq!(report.per_tenant[2].claimed, 0, "the budget was spent");
    let drained = driver.drain(Duration::ZERO).await;
    assert_eq!(drained.aborted, 4);
    assert_eq!(driver.global_available(), 4);
}

/// Ten plannable roots, `plan_per_tick 4`: the planner sees limit 4 and exactly
/// four are decomposed.
#[tokio::test]
async fn boundary_plan_per_tick() {
    let fx = Fx::new();
    let a = fx.store("ta");
    for i in 0..10 {
        campaign(&*a, &format!("root {i}")).await;
    }
    let planner = Arc::new(CountingPlanner::default());
    let driver = Driver::new(
        Arc::clone(&fx.backend) as Arc<dyn CampaignBackend>,
        tenants(&["ta"]),
        DriverConfig {
            plan_per_tick: 4,
            ..cfg(2, 4)
        },
        Arc::clone(&planner) as Arc<dyn TickPlanner>,
    );
    let report = driver.tick().await;
    assert_eq!(*planner.limits.lock().unwrap(), vec![4]);
    assert_eq!(report.planned(), 4);
    let roots = a.list_campaigns(ListFilter::default()).await.unwrap();
    let decomposed = roots
        .iter()
        .filter(|r| r.state == TaskState::Decomposed)
        .count();
    assert_eq!(decomposed, 4);
    assert_eq!(report.per_tenant[0].plan.as_ref().unwrap().summary.split, 4);
}

/// `plan_per_tick 0`: the planner is never called; claims still happen.
#[tokio::test]
async fn boundary_plan_per_tick_zero() {
    let fx = Fx::new();
    let a = fx.store("ta");
    campaign(&*a, "plannable but not planned").await;
    ready_leaves(&*a, 1).await;
    let planner = Arc::new(CountingPlanner::default());
    let driver = Driver::new(
        Arc::clone(&fx.backend) as Arc<dyn CampaignBackend>,
        tenants(&["ta"]),
        DriverConfig {
            plan_per_tick: 0,
            ..cfg(2, 4)
        },
        Arc::clone(&planner) as Arc<dyn TickPlanner>,
    )
    .with_exec(ok_exec());
    let report = driver.tick().await;
    assert!(planner.limits.lock().unwrap().is_empty());
    assert!(report.per_tenant[0].plan.is_none());
    assert_eq!(report.claimed(), 1);
    assert_eq!(fx.phases("ta"), ["reap", "reap_decomposing", "claim"]);
    driver.drain(Duration::from_secs(5)).await;
}

/// A panicking worker: the leaf is `failed` with a bounded error, the permits come
/// back, and the next tick runs.
#[tokio::test]
async fn adversarial_worker_panics() {
    let fx = Fx::new();
    let a = fx.store("ta");
    let (_, leaves) = ready_leaves(&*a, 1).await;
    let driver = fx
        .driver(tenants(&["ta"]), cfg(2, 8))
        .with_exec(panic_exec());
    let report = driver.tick().await;
    assert_eq!(report.dispatched.len(), 1);
    let drained = driver.drain(Duration::from_secs(5)).await;
    assert_eq!(drained.aborted, 0);
    assert_eq!(drained.settled.len(), 1);
    let s = &drained.settled[0];
    assert_eq!((s.tenant.as_str(), s.task), ("ta", leaves[0].task_id));
    assert_eq!(s.outcome, WorkerOutcome::Panic);
    let t = a.get(leaves[0].task_id).await.unwrap();
    assert_eq!(t.state, TaskState::Failed);
    let attempts = work_attempts(&*a, t.task_id).await;
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].outcome, AttemptOutcome::Error);
    // The settled report read the closed attempt back (the driver's own `fail`
    // spent nothing).
    assert_eq!(s.tokens, TokenUsage::default());
    assert_eq!(s.model, attempts[0].model);
    let err = attempts[0].error.clone().unwrap();
    assert!(err.starts_with("worker panicked: boom"), "{err}");
    assert!(err.chars().count() <= MAX_ERROR);
    assert_eq!(driver.global_available(), 8);
    let again = driver.tick().await;
    assert!(!again.disabled);
    assert!(again.dispatched.is_empty());
}

/// A hanging worker is cut at `worker_timeout`: the leaf is `failed` with cause
/// `timeout` and the permits come back. Paused clock: no real waiting.
#[tokio::test(start_paused = true)]
async fn adversarial_worker_hangs() {
    let fx = Fx::new();
    let a = fx.store("ta");
    let (_, leaves) = ready_leaves(&*a, 1).await;
    let driver = fx
        .driver(tenants(&["ta"]), cfg(2, 8))
        .with_exec(start_then_pend());
    let report = driver.tick().await;
    assert_eq!(report.dispatched.len(), 1);
    settle().await;
    assert_eq!(
        a.get(leaves[0].task_id).await.unwrap().state,
        TaskState::Running
    );
    assert_eq!(driver.global_available(), 7);
    tokio::time::advance(Duration::from_secs(61)).await;
    let drained = driver.drain(Duration::from_secs(5)).await;
    assert_eq!(drained.aborted, 0);
    assert_eq!(drained.settled[0].outcome, WorkerOutcome::Timeout);
    let t = a.get(leaves[0].task_id).await.unwrap();
    assert_eq!(t.state, TaskState::Failed);
    let attempts = work_attempts(&*a, t.task_id).await;
    assert_eq!(attempts[0].outcome, AttemptOutcome::Timeout);
    assert!(attempts[0]
        .error
        .as_deref()
        .unwrap()
        .contains("timed out after 60s"));
    assert_eq!(driver.global_available(), 8);
}

/// The store fails `claim` for tenant A: logged and counted, tenant B is still
/// served, and no permit leaks.
#[tokio::test]
async fn adversarial_store_error_mid_tick() {
    let fx = Fx::with_failing_claim(Some("ta"));
    for t in ["ta", "tb"] {
        ready_leaves(&*fx.store(t), 1).await;
    }
    let driver = fx
        .driver(tenants(&["ta", "tb"]), cfg(2, 4))
        .with_exec(ok_exec());
    let report = driver.tick().await;
    assert_eq!(report.errors, 1);
    assert_eq!(report.per_tenant[0].claimed, 0);
    assert_eq!(report.per_tenant[0].errors, 1);
    assert_eq!(report.per_tenant[1].claimed, 1);
    assert_eq!(report.dispatched.len(), 1);
    assert_eq!(report.dispatched[0].0, "tb");
    driver.drain(Duration::from_secs(5)).await;
    assert_eq!(driver.global_available(), 4);
    assert_eq!(
        fx.store("ta").get(TaskId(2)).await.unwrap().state,
        TaskState::Ready,
        "A's leaf is untouched"
    );
}

/// A fixed tenant that is not a path-safe segment is skipped without opening
/// anything; the others are served.
#[tokio::test]
async fn adversarial_tenant_unsafe_skipped() {
    let fx = Fx::new();
    ready_leaves(&*fx.store("ta"), 1).await;
    let driver = fx
        .driver(tenants(&["../x", "ta"]), cfg(2, 4))
        .with_exec(ok_exec());
    let report = driver.tick().await;
    assert_eq!(report.per_tenant[0].tenant, "../x");
    assert!(report.per_tenant[0].skipped.is_some());
    assert_eq!(report.per_tenant[0].errors, 1);
    assert!(fx.log_entries().iter().all(|(t, _)| t != "../x"));
    assert_eq!(report.per_tenant[1].claimed, 1);
    driver.drain(Duration::from_secs(5)).await;
}

// -- pure helpers ---------------------------------------------------------------

#[rstest]
#[case::positive_two_tenants(vec![("a", vec![1, 2, 3]), ("b", vec![4, 5])], vec![("a", 1), ("b", 4), ("a", 2), ("b", 5), ("a", 3)])]
#[case::positive_three_even(vec![("a", vec![1]), ("b", vec![2]), ("c", vec![3])], vec![("a", 1), ("b", 2), ("c", 3)])]
#[case::corner_empty(vec![], vec![])]
#[case::corner_one_empty_batch(vec![("a", vec![]), ("b", vec![1])], vec![("b", 1)])]
#[case::boundary_single(vec![("a", vec![1])], vec![("a", 1)])]
fn interleave_rows(#[case] batches: Vec<(&str, Vec<u32>)>, #[case] want: Vec<(&str, u32)>) {
    assert_eq!(interleave(batches), want);
}

#[rstest]
#[case::positive_tenant_binds(2, 4, 2)]
#[case::positive_budget_binds(4, 2, 2)]
#[case::corner_tenant_full(0, 4, 0)]
#[case::corner_budget_spent(3, 0, 0)]
#[case::boundary_equal(2, 2, 2)]
fn claim_limit_rows(#[case] free: usize, #[case] budget: usize, #[case] want: usize) {
    assert_eq!(claim_limit(free, budget), want);
}

#[test]
fn adversarial_panic_text_bounded() {
    let huge: Box<dyn std::any::Any + Send> = Box::new("x".repeat(10_000));
    let text = panic_text(huge);
    assert_eq!(text.chars().count(), MAX_ERROR);
    assert!(text.starts_with("worker panicked: xxx"));
    let short: Box<dyn std::any::Any + Send> = Box::new("boom");
    assert_eq!(panic_text(short), "worker panicked: boom");
    let other: Box<dyn std::any::Any + Send> = Box::new(42_u8);
    assert_eq!(panic_text(other), "worker panicked: non-string payload");
}

#[test]
fn positive_driver_config_default_mirrors_the_shipped_keys() {
    let d = DriverConfig::default();
    assert!(d.enabled);
    assert_eq!(d.per_tenant_workers, 2);
    assert_eq!(d.global_workers, 8);
    assert_eq!(d.plan_per_tick, 4);
    assert_eq!(d.worker_timeout, Duration::from_secs(3_600));
    assert_eq!(d.lease_secs, Policy::default().lease_secs);
    assert_eq!(d.decomposing_max_secs, DECOMPOSING_MAX_SECS);
    assert_eq!(POLL_BATCH, 20);
}
