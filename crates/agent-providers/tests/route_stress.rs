//! Concurrency stress for the task-router (model-router 02b): many tasks, a
//! mixed-health fleet, breakers flapping under a racing fake clock, hostile and
//! benign hints interleaved. The assertions are liveness + accounting, not
//! ordering: every call ends in a DEFINED outcome (no panic, no hang), every
//! attempt was observed as exactly one `Decided` or `NoCandidate`, and the
//! router still answers cleanly after the storm (no wedged breaker state).
//! Run: `cargo test -p agent-providers --features provider-router --test route_stress`.
#![cfg(feature = "provider-router")]

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

use agent_core::{
    ChunkStream, CompletionRequest, CompletionResponse, Error, LlmProvider, Message,
    ModelCapabilities, PoolTier, Result, RouteHint, RouteRole, TaskMode,
};
use agent_providers::route::{Match, Policy, Prefer, Role, Rule};
use agent_providers::{RouteEvent, RouterUpstream, TaskRouter};

fn caps() -> ModelCapabilities {
    ModelCapabilities {
        supports_tools: true,
        // Unknown window: the hostile-min_context hints stay fail-soft filters
        // rather than starving the whole fleet.
        context_window: 0,
        supports_response_format: false,
        supports_vision: false,
    }
}

/// Always succeeds.
struct Solid;
#[async_trait::async_trait]
impl LlmProvider for Solid {
    fn capabilities(&self) -> ModelCapabilities {
        caps()
    }
    async fn complete(&self, _r: CompletionRequest) -> Result<CompletionResponse> {
        Ok(CompletionResponse {
            message: Message::assistant("ok"),
            finish_reason: "stop".into(),
            usage: None,
        })
    }
    async fn stream(&self, _r: CompletionRequest) -> Result<ChunkStream> {
        Err(Error::Provider("no stream".into()))
    }
}

/// Fails retryably every other call — keeps breakers opening and half-closing.
struct Flaky(AtomicUsize);
#[async_trait::async_trait]
impl LlmProvider for Flaky {
    fn capabilities(&self) -> ModelCapabilities {
        caps()
    }
    async fn complete(&self, _r: CompletionRequest) -> Result<CompletionResponse> {
        if self.0.fetch_add(1, Ordering::Relaxed).is_multiple_of(2) {
            Err(Error::Provider("http 429: flap".into()))
        } else {
            Ok(CompletionResponse {
                message: Message::assistant("flaky-ok"),
                finish_reason: "stop".into(),
                usage: None,
            })
        }
    }
    async fn stream(&self, _r: CompletionRequest) -> Result<ChunkStream> {
        Err(Error::Provider("no stream".into()))
    }
}

/// Always fails retryably — a permanently down upstream the breaker must cage.
struct Down;
#[async_trait::async_trait]
impl LlmProvider for Down {
    fn capabilities(&self) -> ModelCapabilities {
        caps()
    }
    async fn complete(&self, _r: CompletionRequest) -> Result<CompletionResponse> {
        Err(Error::Provider("http 503: down".into()))
    }
    async fn stream(&self, _r: CompletionRequest) -> Result<ChunkStream> {
        Err(Error::Provider("no stream".into()))
    }
}

fn up(id: String, provider: Arc<dyn LlmProvider>, tags: &[&str], tier: PoolTier) -> RouterUpstream {
    RouterUpstream {
        id,
        tags: tags.iter().map(|s| (*s).to_string()).collect(),
        tier,
        input_cost: 1.0,
        provider,
    }
}

/// A rotating mix of benign, role/mode, override, and hostile hints.
fn hint_for(i: usize) -> Option<RouteHint> {
    match i % 6 {
        0 => None,
        1 => Some(RouteHint {
            role: Some(RouteRole::Judge),
            task_mode: Some(TaskMode::Review),
            ..Default::default()
        }),
        2 => Some(RouteHint {
            override_upstream: Some("solid3".into()),
            ..Default::default()
        }),
        3 => Some(RouteHint {
            // Hostile numbers + an unknown override: must degrade, never panic.
            min_context: u32::MAX,
            max_cost: Some(f32::NAN),
            override_upstream: Some("no-such-upstream".into()),
            ..Default::default()
        }),
        4 => Some(RouteHint {
            role: Some(RouteRole::Summarize),
            tier: Some(PoolTier::Light),
            ..Default::default()
        }),
        _ => Some(RouteHint {
            // Over-long override: dropped wholesale before comparison.
            override_upstream: Some("x".repeat(64 * 1024)),
            ..Default::default()
        }),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn stress_concurrent_mixed_fleet_stays_live_and_accounted() {
    // 10 solid + 10 flaky + 10 down upstreams across tiers.
    let mut ups = Vec::new();
    for i in 0..10 {
        ups.push(up(
            format!("solid{i}"),
            Arc::new(Solid),
            &["steady"],
            PoolTier::Light,
        ));
        ups.push(up(
            format!("flaky{i}"),
            Arc::new(Flaky(AtomicUsize::new(0))),
            &["reasoning"],
            PoolTier::Heavy,
        ));
        ups.push(up(
            format!("down{i}"),
            Arc::new(Down),
            &["reasoning"],
            PoolTier::Heavy,
        ));
    }
    // The default preference walks INTO the failing members first, so failover
    // and breaker churn are guaranteed, not incidental.
    let policy = Policy {
        rules: vec![
            Rule {
                match_: Match {
                    role: Some(Role::Judge),
                    ..Default::default()
                },
                prefer: Prefer {
                    tags: vec!["reasoning".into()],
                    tier: Some(PoolTier::Heavy),
                    upstreams: vec![],
                    policy: None,
                },
            },
            Rule {
                match_: Match {
                    role: Some(Role::Summarize),
                    ..Default::default()
                },
                prefer: Prefer {
                    upstreams: vec!["solid0".into()],
                    ..Default::default()
                },
            },
        ],
        default_prefer: Prefer {
            upstreams: vec!["down0".into(), "flaky0".into(), "solid0".into()],
            ..Default::default()
        },
    };

    // A racing fake clock: every call advances time, so breaker cooldowns
    // (10ms here) open and re-close mid-storm.
    let clock = Arc::new(AtomicU64::new(0));
    let c = clock.clone();
    let decided = Arc::new(AtomicUsize::new(0));
    let no_candidate = Arc::new(AtomicUsize::new(0));
    let (d, n) = (decided.clone(), no_candidate.clone());

    let router = Arc::new(
        TaskRouter::new(ups, policy)
            .expect("router")
            .with_breaker(2, 10)
            .with_clock(Arc::new(move || c.fetch_add(1, Ordering::Relaxed)))
            .with_observer(Arc::new(move |ev| match ev {
                RouteEvent::Decided { .. } => {
                    d.fetch_add(1, Ordering::Relaxed);
                }
                RouteEvent::NoCandidate { .. } => {
                    n.fetch_add(1, Ordering::Relaxed);
                }
                _ => {}
            })),
    );

    const TASKS: usize = 32;
    const CALLS: usize = 50;
    let mut handles = Vec::new();
    for t in 0..TASKS {
        let r = router.clone();
        handles.push(tokio::spawn(async move {
            let mut ok = 0usize;
            let mut err = 0usize;
            for i in 0..CALLS {
                let req = CompletionRequest {
                    messages: vec![Message::user(format!("t{t} call {i}"))],
                    route: hint_for(t + i),
                    ..Default::default()
                };
                match r.complete(req).await {
                    Ok(resp) => {
                        assert!(!resp.message.content_text().is_empty());
                        ok += 1;
                    }
                    // A defined provider error is an acceptable outcome under
                    // churn; a panic/hang is not (either would fail the test).
                    Err(Error::Provider(_)) => err += 1,
                    Err(e) => panic!("unexpected error class under stress: {e}"),
                }
            }
            (ok, err)
        }));
    }

    let mut total_ok = 0;
    let mut total_err = 0;
    for h in handles {
        let (ok, err) = h.await.expect("no task panicked");
        total_ok += ok;
        total_err += err;
    }
    assert_eq!(total_ok + total_err, TASKS * CALLS, "every call accounted");
    // Solid upstreams exist and are always eligible, so the storm cannot have
    // been a total loss.
    assert!(total_ok > 0, "no call ever succeeded: {total_err} errors");
    // Exactly one decision-or-shed observation per call — the observer stream
    // neither dropped nor duplicated under concurrency.
    assert_eq!(
        decided.load(Ordering::Relaxed) + no_candidate.load(Ordering::Relaxed),
        TASKS * CALLS,
        "decision accounting drifted"
    );

    // After the storm: breakers must not be wedged — advance past every
    // cooldown and a plain request must still succeed.
    clock.fetch_add(10_000, Ordering::Relaxed);
    let resp = router
        .complete(CompletionRequest {
            messages: vec![Message::user("post-storm")],
            route: Some(RouteHint {
                override_upstream: Some("solid5".into()),
                ..Default::default()
            }),
            ..Default::default()
        })
        .await
        .expect("router recovered after the storm");
    assert_eq!(resp.message.content_text(), "ok");
}
