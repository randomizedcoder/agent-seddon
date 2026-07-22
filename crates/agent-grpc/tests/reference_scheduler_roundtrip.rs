//! The `ReferenceResolver` and `Scheduler` seams, round-tripped over gRPC.
//!
//! Both were already retained on `Agent`, so this pair is the mechanical floor
//! of the sweep: no runtime plumbing, only wire work.

mod common;
use common::{spawn, Transport};

use agent_core::{ReferenceResolver, Resolution, Scheduler};
use agent_grpc::client::{GrpcReference, GrpcScheduler};
use agent_grpc::server::{reference_router, scheduler_router};
use async_trait::async_trait;
use rstest::rstest;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// ReferenceResolver
// ---------------------------------------------------------------------------

/// A resolver that reports one block and one warning, so the round-trip has
/// something in every field to compare.
struct FixtureResolver;

#[async_trait]
impl ReferenceResolver for FixtureResolver {
    async fn resolve(&self, prompt: &str, budget_tokens: usize) -> Resolution {
        Resolution {
            blocks: vec![agent_core::ContextBlock {
                source: "@file:src/lib.rs".into(),
                content: format!("prompt={prompt} budget={budget_tokens}"),
            }],
            warnings: vec!["@file:missing.rs not found".into()],
            blocked: false,
        }
    }
}

#[rstest]
#[case::tcp(Transport::Tcp)]
#[case::uds(Transport::Uds)]
#[tokio::test(flavor = "multi_thread")]
async fn positive_reference_resolution_round_trips(#[case] transport: Transport) {
    let (dial, _srv) = spawn(transport, reference_router(Arc::new(FixtureResolver))).await;
    let client = GrpcReference::connect(&dial).unwrap();

    let res = client.resolve("see @file:src/lib.rs", 500).await;

    assert_eq!(res.blocks.len(), 1);
    assert_eq!(res.blocks[0].source, "@file:src/lib.rs");
    // The budget must reach the server as itself, not be dropped or truncated.
    assert_eq!(
        res.blocks[0].content,
        "prompt=see @file:src/lib.rs budget=500"
    );
    assert_eq!(res.warnings, vec!["@file:missing.rs not found"]);
    assert!(!res.blocked);
}

/// `blocked` is a distinct outcome from "no blocks" and must survive as itself:
/// it tells the caller to leave the prompt unmodified.
#[tokio::test(flavor = "multi_thread")]
async fn positive_blocked_resolution_survives_as_blocked() {
    struct Blocked;
    #[async_trait]
    impl ReferenceResolver for Blocked {
        async fn resolve(&self, _p: &str, _b: usize) -> Resolution {
            Resolution {
                blocks: vec![],
                warnings: vec!["over budget".into()],
                blocked: true,
            }
        }
    }

    let (dial, _srv) = spawn(Transport::Tcp, reference_router(Arc::new(Blocked))).await;
    let client = GrpcReference::connect(&dial).unwrap();

    let res = client.resolve("@file:huge.bin", 1).await;
    assert!(
        res.blocked,
        "blocked must not be flattened into 'no blocks'"
    );
    assert!(res.blocks.is_empty());
}

/// An unreachable resolver must **degrade**, not fail and not claim `blocked`.
/// `resolve` has no error channel by design — one bad mention can't fail a turn —
/// and `blocked` means "deliberately refused", which an outage is not.
#[tokio::test(flavor = "multi_thread")]
async fn negative_unreachable_resolver_degrades_with_a_warning() {
    let dial = agent_grpc::Endpoint::parse("127.0.0.1:1");
    let client = GrpcReference::connect(&dial).unwrap();

    let res = client.resolve("see @file:x.rs", 100).await;
    assert!(res.blocks.is_empty());
    assert!(
        !res.blocked,
        "an outage is a degraded expansion, not a deliberate refusal"
    );
    assert!(
        !res.warnings.is_empty(),
        "the operator must be told the resolver was unreachable"
    );
}

/// A huge budget must not wrap or panic when narrowed to `usize` server-side.
#[rstest]
#[case::boundary_zero(0)]
#[case::boundary_one(1)]
#[case::adversarial_max(usize::MAX)]
#[tokio::test(flavor = "multi_thread")]
async fn adversarial_extreme_budget_is_safe(#[case] budget: usize) {
    let (dial, _srv) = spawn(Transport::Tcp, reference_router(Arc::new(FixtureResolver))).await;
    let client = GrpcReference::connect(&dial).unwrap();
    let res = client.resolve("p", budget).await;
    assert_eq!(res.blocks.len(), 1);
}

// ---------------------------------------------------------------------------
// Scheduler
// ---------------------------------------------------------------------------

fn scheduler() -> Arc<dyn Scheduler> {
    Arc::new(agent_scheduler::LocalScheduler::new())
}

/// Schedule → list → cancel across the wire, with the parsed schedule intact.
#[rstest]
#[case::tcp(Transport::Tcp)]
#[case::uds(Transport::Uds)]
#[tokio::test(flavor = "multi_thread")]
async fn positive_schedule_list_cancel_round_trips(#[case] transport: Transport) {
    let (dial, _srv) = spawn(transport, scheduler_router(scheduler())).await;
    let client = GrpcScheduler::connect(&dial).unwrap();

    let id = client
        .schedule("every 30m", "triage issues")
        .await
        .expect("schedule");

    let jobs = client.list().await.expect("list");
    let job = jobs.iter().find(|j| j.id == id).expect("job listed");
    assert_eq!(job.goal, "triage issues");
    assert_eq!(job.spec, "every 30m");
    // The PARSED schedule must cross the wire, not just the display string — a
    // client that had to re-parse could disagree with the server about firing.
    assert_eq!(
        job.schedule,
        agent_core::Schedule::Interval { secs: 1800 },
        "the parsed schedule must survive the hop"
    );

    assert!(client.cancel(&id).await.expect("cancel"));
    // Cancelling twice is a no-op reporting `false`, not an error — which is what
    // makes the client's cancel retry safe.
    assert!(!client.cancel(&id).await.expect("second cancel"));
}

/// Every `Schedule` variant must round-trip as itself.
#[rstest]
#[case::interval("every 45s", agent_core::Schedule::Interval { secs: 45 })]
#[case::cron("cron: 0 6 * * *", agent_core::Schedule::Cron { expr: "0 6 * * *".into() })]
// Far future on purpose: the scheduler rejects a one-shot already in the past
// (correctly — it would never fire), so a fixed past instant would rot the test.
#[case::once_absolute("once: 4102444800000", agent_core::Schedule::Once { at_ms: 4102444800000 })]
#[tokio::test(flavor = "multi_thread")]
async fn positive_every_schedule_variant_round_trips(
    #[case] spec: &str,
    #[case] want: agent_core::Schedule,
) {
    let (dial, _srv) = spawn(Transport::Tcp, scheduler_router(scheduler())).await;
    let client = GrpcScheduler::connect(&dial).unwrap();

    let id = client.schedule(spec, "g").await.expect("schedule");
    let jobs = client.list().await.expect("list");
    let job = jobs.iter().find(|j| j.id == id).expect("job listed");
    assert_eq!(job.schedule, want);
}

/// A spec the scheduler rejects must fail at scheduling time, over the wire, not
/// produce a job that silently never fires.
#[rstest]
#[case::negative_gibberish("not a schedule")]
#[case::negative_unsupported_macro("@hourly")]
#[case::adversarial_empty("")]
#[case::adversarial_cron_extension("cron: 0 6 L * *")]
// A one-shot already in the past would never fire; rejecting it at scheduling
// time is the whole point of the "never silently mis-schedule" rule.
#[case::negative_one_shot_in_the_past("once: 1000")]
#[tokio::test(flavor = "multi_thread")]
async fn adversarial_bad_spec_is_rejected_at_scheduling_time(#[case] spec: &str) {
    let (dial, _srv) = spawn(Transport::Tcp, scheduler_router(scheduler())).await;
    let client = GrpcScheduler::connect(&dial).unwrap();

    assert!(
        client.schedule(spec, "g").await.is_err(),
        "`{spec}` must be rejected rather than scheduled and never fired"
    );
}

/// History is empty for a job that has not run, and cancelling an unknown id
/// reports `false` rather than erroring.
#[tokio::test(flavor = "multi_thread")]
async fn boundary_fresh_job_has_no_history_and_unknown_cancel_is_false() {
    let (dial, _srv) = spawn(Transport::Tcp, scheduler_router(scheduler())).await;
    let client = GrpcScheduler::connect(&dial).unwrap();

    let id = client.schedule("every 1h", "g").await.unwrap();
    assert!(client.history(&id).await.expect("history").is_empty());
    assert!(!client.cancel("no-such-job").await.expect("unknown cancel"));
}

/// The seam is unreachable ⇒ `Err`. A `schedule` that quietly no-ops is this
/// seam's worst failure: nobody watches an unattended job, so its absence shows
/// up only as work that never happened.
#[tokio::test(flavor = "multi_thread")]
async fn negative_unreachable_scheduler_errors() {
    let dial = agent_grpc::Endpoint::parse("127.0.0.1:1");
    let client = GrpcScheduler::connect(&dial).unwrap();

    assert!(client.schedule("every 1h", "g").await.is_err());
    assert!(client.list().await.is_err());
}

/// A job whose schedule is absent on the wire is malformed — guessing would
/// either spin (interval 0) or never fire. It must be dropped from a listing
/// rather than defaulted, and must not take the good rows with it.
#[test]
fn adversarial_job_without_a_schedule_is_rejected_not_defaulted() {
    let bad = agent_proto::pb::SchedJob {
        id: "j1".into(),
        spec: "every 1h".into(),
        schedule: None,
        goal: "g".into(),
        next_fire_ms: None,
        enabled: true,
    };
    assert!(
        agent_core::Job::try_from(bad).is_err(),
        "a job with no schedule must not decode to a defaulted one"
    );
}
