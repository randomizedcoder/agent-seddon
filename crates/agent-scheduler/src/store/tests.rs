//! `StoreScheduler` matrix over the in-memory [`MemoryBackend`] (the durable
//! semantics are backend-agnostic; the backend tiers themselves are proven in
//! `agent-config-store`). Four case classes (`positive_`/`negative_`/`boundary_`/
//! `corner_`) plus `adversarial_` for the untrusted `tenant`/`id`/blob; each row
//! carries its `desc`/`expect` intent.

use super::*;
use crate::MAX_HISTORY;
use agent_config_store::MemoryBackend;
use agent_core::Schedule;
use rstest::rstest;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

const T0: u64 = 1_704_067_200_000; // 2024-01-01T00:00:00Z

fn backend() -> Arc<dyn Backend> {
    Arc::new(MemoryBackend::new())
}

fn store_at(backend: Arc<dyn Backend>, clock: Arc<AtomicU64>) -> StoreScheduler {
    StoreScheduler::new(backend).with_clock(Arc::new(move || clock.load(Ordering::SeqCst)))
}

fn store_tenant(backend: Arc<dyn Backend>, clock: Arc<AtomicU64>, tenant: &str) -> StoreScheduler {
    StoreScheduler::with_tenant(backend, tenant)
        .expect("valid tenant")
        .with_clock(Arc::new(move || clock.load(Ordering::SeqCst)))
}

/// An executor that counts calls and always succeeds.
fn ok_exec(
    calls: Arc<AtomicUsize>,
) -> impl Fn(String) -> Pin<Box<dyn Future<Output = Result<String>> + Send>> {
    move |_goal| {
        let c = calls.clone();
        Box::pin(async move {
            c.fetch_add(1, Ordering::SeqCst);
            Ok("done".to_string())
        })
    }
}

// --- positive ---------------------------------------------------------------

/// positive: a scheduled job is listable and its id round-trips.
#[tokio::test]
async fn positive_schedule_then_list() {
    let clock = Arc::new(AtomicU64::new(T0));
    let s = store_at(backend(), clock);
    let id = s.schedule("every 60s", "do a thing").await.unwrap();
    let jobs = s.list().await.unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].id, id);
    assert_eq!(jobs[0].goal, "do a thing");
}

/// positive: a due job runs on tick and records a completed outcome.
#[tokio::test]
async fn positive_due_job_runs_and_records_outcome() {
    let clock = Arc::new(AtomicU64::new(T0));
    let calls = Arc::new(AtomicUsize::new(0));
    let s = store_at(backend(), clock.clone());
    let id = s.schedule("every 60s", "g").await.unwrap();

    assert_eq!(
        s.tick_with(ok_exec(calls.clone())).await.unwrap(),
        0,
        "not due yet"
    );
    clock.store(T0 + 60_000, Ordering::SeqCst);
    assert_eq!(s.tick_with(ok_exec(calls.clone())).await.unwrap(), 1, "due");
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    let h = s.history(&id).await.unwrap();
    assert_eq!(h.len(), 1);
    assert_eq!(h[0].outcome, RunOutcome::Completed);
    assert_eq!(h[0].detail, "done");
}

/// positive: cancel removes the job; a cancelled job does not fire.
#[tokio::test]
async fn positive_cancel_removes_the_job() {
    let clock = Arc::new(AtomicU64::new(T0));
    let calls = Arc::new(AtomicUsize::new(0));
    let s = store_at(backend(), clock.clone());
    let id = s.schedule("every 60s", "g").await.unwrap();
    assert!(s.cancel(&id).await.unwrap());
    assert!(!s.cancel(&id).await.unwrap(), "cancelling twice is false");
    clock.store(T0 + 60_000, Ordering::SeqCst);
    s.tick_with(ok_exec(calls.clone())).await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 0, "a cancelled job ran");
}

/// positive: the overlap guard — a fire while a claim is live is skipped (and
/// the drop is recorded), never stacked.
#[tokio::test]
async fn positive_overlapping_fire_is_skipped_not_stacked() {
    let clock = Arc::new(AtomicU64::new(T0));
    let s = store_at(backend(), clock.clone());
    let id = s.schedule("every 60s", "g").await.unwrap();

    clock.store(T0 + 60_000, Ordering::SeqCst);
    assert_eq!(
        s.claim_due(T0 + 60_000).await.unwrap().len(),
        1,
        "first fire wins"
    );

    clock.store(T0 + 120_000, Ordering::SeqCst);
    assert!(
        s.claim_due(T0 + 120_000).await.unwrap().is_empty(),
        "a second copy must not start while the claim is live"
    );

    let h = s.history(&id).await.unwrap();
    assert_eq!(h.len(), 1);
    assert_eq!(
        h[0].outcome,
        RunOutcome::Skipped,
        "the drop must be visible"
    );
}

/// positive: a crashed run's claim is reclaimable after the TTL.
#[tokio::test]
async fn positive_stale_claim_is_reclaimed_after_the_ttl() {
    let clock = Arc::new(AtomicU64::new(T0));
    let s = store_at(backend(), clock.clone()).with_claim_ttl_ms(10_000);
    s.schedule("every 60s", "g").await.unwrap();

    clock.store(T0 + 60_000, Ordering::SeqCst);
    assert_eq!(s.claim_due(T0 + 60_000).await.unwrap().len(), 1);
    let later = T0 + 60_000 + 999_000; // well past the TTL, claim never released
    clock.store(later, Ordering::SeqCst);
    assert_eq!(
        s.claim_due(later).await.unwrap().len(),
        1,
        "a dead run must not wedge the job forever"
    );
}

/// positive: two tenants' jobs are isolated, and `Backend::tenants` enumerates
/// exactly the tenants with jobs — the driver's discovery primitive.
#[tokio::test]
async fn positive_two_tenants_are_isolated_and_enumerable() {
    let clock = Arc::new(AtomicU64::new(T0));
    let b = backend();
    let a = store_tenant(b.clone(), clock.clone(), "orga");
    let z = store_tenant(b.clone(), clock.clone(), "orgb");
    a.schedule("every 60s", "ga").await.unwrap();
    z.schedule("every 60s", "gz1").await.unwrap();
    z.schedule("every 60s", "gz2").await.unwrap();

    assert_eq!(a.list().await.unwrap().len(), 1, "orga sees only its own");
    assert_eq!(z.list().await.unwrap().len(), 2, "orgb sees only its own");
    assert_eq!(
        b.tenants(COLLECTION).await.unwrap(),
        vec!["orga".to_string(), "orgb".to_string()],
        "distinct + sorted tenants with jobs"
    );
}

// --- boundary ---------------------------------------------------------------

/// boundary: a one-shot fires exactly once, then stops being armed.
#[tokio::test]
async fn boundary_once_runs_exactly_once() {
    let clock = Arc::new(AtomicU64::new(T0));
    let calls = Arc::new(AtomicUsize::new(0));
    let s = store_at(backend(), clock.clone());
    s.schedule("in 60s", "g").await.unwrap();

    clock.store(T0 + 60_000, Ordering::SeqCst);
    s.tick_with(ok_exec(calls.clone())).await.unwrap();
    clock.store(T0 + 600_000, Ordering::SeqCst);
    s.tick_with(ok_exec(calls.clone())).await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1, "a one-shot fired twice");
}

/// boundary: per-tenant history is bounded.
#[tokio::test]
async fn boundary_history_is_bounded() {
    let clock = Arc::new(AtomicU64::new(T0));
    let calls = Arc::new(AtomicUsize::new(0));
    let s = store_at(backend(), clock.clone());
    let id = s.schedule("every 1s", "g").await.unwrap();
    for i in 1..=(MAX_HISTORY as u64 + 20) {
        clock.store(T0 + i * 1_000, Ordering::SeqCst);
        s.tick_with(ok_exec(calls.clone())).await.unwrap();
    }
    assert!(
        s.history(&id).await.unwrap().len() <= MAX_HISTORY,
        "history grew unbounded"
    );
}

// --- corner -----------------------------------------------------------------

/// corner: durability — a fresh `StoreScheduler` over the same backend sees a
/// job scheduled by a previous instance (unlike the in-memory tier).
#[tokio::test]
async fn corner_jobs_survive_a_new_scheduler_instance() {
    let clock = Arc::new(AtomicU64::new(T0));
    let b = backend();
    let id = {
        let s1 = store_at(b.clone(), clock.clone());
        s1.schedule("every 60s", "g").await.unwrap()
    };
    let s2 = store_at(b.clone(), clock.clone());
    let jobs = s2.list().await.unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(
        jobs[0].id, id,
        "the persisted job is visible to a new instance"
    );
}

/// corner: a cron job re-armed on an instant its expression matches must not
/// re-fire without the clock advancing (the LocalScheduler hot-loop regression).
#[tokio::test]
async fn corner_cron_job_does_not_spin_when_rearmed_on_a_match() {
    let clock = Arc::new(AtomicU64::new(T0));
    let calls = Arc::new(AtomicUsize::new(0));
    let s = store_at(backend(), clock.clone());
    s.schedule("cron: 0 * * * *", "g").await.unwrap();

    clock.store(T0 + 3_600_000, Ordering::SeqCst);
    assert_eq!(s.tick_with(ok_exec(calls.clone())).await.unwrap(), 1);
    for _ in 0..5 {
        assert_eq!(
            s.tick_with(ok_exec(calls.clone())).await.unwrap(),
            0,
            "job re-fired without time advancing"
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

// --- negative ---------------------------------------------------------------

/// negative: a failing run is recorded as failed, never silently dropped.
#[tokio::test]
async fn negative_failing_run_is_recorded() {
    let clock = Arc::new(AtomicU64::new(T0));
    let s = store_at(backend(), clock.clone());
    let id = s.schedule("every 60s", "g").await.unwrap();
    clock.store(T0 + 60_000, Ordering::SeqCst);
    s.tick_with(|_g| async { Err(Error::Scheduler("boom".into())) })
        .await
        .unwrap();
    let h = s.history(&id).await.unwrap();
    assert_eq!(h[0].outcome, RunOutcome::Failed);
    assert!(h[0].detail.contains("boom"));
}

#[rstest]
// desc: an empty goal is rejected — a job that does nothing is a mistake, not a job.
#[case::empty_goal("every 60s", "")]
// desc: an unparseable spec is rejected at schedule time.
#[case::bad_spec("nonsense", "g")]
// desc: a one-shot already in the past never fires ⇒ rejected up front.
#[case::past_one_shot("once: 1", "g")]
#[tokio::test]
async fn negative_bad_schedule_requests_are_rejected(#[case] spec: &str, #[case] goal: &str) {
    let clock = Arc::new(AtomicU64::new(T0));
    let s = store_at(backend(), clock);
    assert!(s.schedule(spec, goal).await.is_err());
}

// --- adversarial ------------------------------------------------------------

#[rstest]
// desc: path traversal in a tenant is rejected by the `safe_segment` gate.
#[case::traversal("../../etc")]
// desc: a path separator in a tenant is rejected.
#[case::separator("a/b")]
// desc: bare `..` is rejected.
#[case::dotdot("..")]
#[tokio::test]
async fn adversarial_hostile_tenant_is_rejected(#[case] tenant: &str) {
    let err = StoreScheduler::with_tenant(backend(), tenant)
        .err()
        .expect("hostile tenant rejected");
    assert!(
        format!("{err}").contains("invalid tenant"),
        "fail closed: {err}"
    );
}

/// adversarial: a hostile job id never reaches the store as a key — cancel says
/// "did not exist" and history is empty, rather than erroring or traversing.
#[tokio::test]
async fn adversarial_hostile_job_id_is_confined() {
    let clock = Arc::new(AtomicU64::new(T0));
    let s = store_at(backend(), clock);
    assert!(
        !s.cancel("../../evil").await.unwrap(),
        "hostile id cannot exist"
    );
    assert!(
        s.history("a/b").await.unwrap().is_empty(),
        "hostile id has no history"
    );
}

/// adversarial: a tampered (non-decodable) job blob fails closed on read rather
/// than yielding a partial job.
#[tokio::test]
async fn adversarial_tampered_blob_fails_closed() {
    let clock = Arc::new(AtomicU64::new(T0));
    let b = backend();
    b.apply(&[
        Write::EnsureTenant {
            tenant: DEFAULT_TENANT.to_string(),
        },
        Write::Put {
            collection: COLLECTION,
            tenant: DEFAULT_TENANT.to_string(),
            id: "job-tampered".to_string(),
            blob: b"not json".to_vec(),
        },
    ])
    .await
    .unwrap();
    let s = store_at(b, clock);
    let err = s
        .list()
        .await
        .expect_err("a tampered blob must fail closed");
    assert!(
        format!("{err}").contains("decode job"),
        "clear decode error: {err}"
    );
}

/// adversarial: a future-dated claim (clock skew / restored backup) is treated
/// as stale, so the job is still reclaimable rather than permanently wedged.
#[tokio::test]
async fn adversarial_future_dated_claim_is_reclaimed() {
    let clock = Arc::new(AtomicU64::new(T0));
    let b = backend();
    // Seed a due job whose claim is dated in the future.
    let sj = StoredJob {
        job: Job {
            id: "job-skew".to_string(),
            spec: "every 60s".to_string(),
            schedule: Schedule::Interval { secs: 60 },
            goal: "g".to_string(),
            next_fire_ms: Some(T0),
            enabled: true,
        },
        claimed_at_ms: Some(T0 + 999_999),
        history: Vec::new(),
    };
    b.apply(&[
        Write::EnsureTenant {
            tenant: DEFAULT_TENANT.to_string(),
        },
        Write::Put {
            collection: COLLECTION,
            tenant: DEFAULT_TENANT.to_string(),
            id: "job-skew".to_string(),
            blob: encode_job(&sj).unwrap(),
        },
    ])
    .await
    .unwrap();
    let s = store_at(b, clock);
    let now = T0 + 60_000;
    assert_eq!(
        s.claim_due(now).await.unwrap().len(),
        1,
        "a future claim must not block execution"
    );
}

/// adversarial: the model can create jobs, so the per-tenant count is bounded.
#[tokio::test]
async fn adversarial_job_count_is_bounded() {
    let clock = Arc::new(AtomicU64::new(T0));
    let s = store_at(backend(), clock).with_max_jobs(3);
    for _ in 0..3 {
        s.schedule("every 60s", "g").await.unwrap();
    }
    assert!(
        s.schedule("every 60s", "g").await.is_err(),
        "unbounded jobs"
    );
}
