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
