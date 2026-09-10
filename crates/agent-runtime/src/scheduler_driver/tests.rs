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
use std::sync::Mutex;

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
