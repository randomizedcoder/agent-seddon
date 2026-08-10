//! Heap leak + allocation-budget assertion for the task-router's per-call path
//! under dhat: sustained routed completes — including failover hops and a
//! carried `RouteHint` — must free every scratch allocation, and the
//! borrowed-view decision path must stay within a small per-call block budget
//! (the 02b low-hanging-fruit pass removed the per-call id/tag clones; this
//! budget keeps them out). Compiled only with
//! `--features dhat-heap,provider-router`; `nix/checks/leak.nix` runs it.
#![cfg(all(feature = "dhat-heap", feature = "provider-router"))]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use agent_core::{
    ChunkStream, CompletionRequest, CompletionResponse, Error, LlmProvider, Message,
    ModelCapabilities, PoolTier, Result, RouteHint, RouteRole, TaskMode,
};
use agent_providers::route::{Policy, Prefer};
use agent_providers::{RouterUpstream, TaskRouter};

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

struct Ok1(&'static str);
#[async_trait::async_trait]
impl LlmProvider for Ok1 {
    fn capabilities(&self) -> ModelCapabilities {
        ModelCapabilities {
            supports_tools: true,
            context_window: 100_000,
            supports_response_format: false,
            supports_vision: false,
        }
    }
    async fn complete(&self, _r: CompletionRequest) -> Result<CompletionResponse> {
        Ok(CompletionResponse {
            message: Message::assistant(self.0),
            finish_reason: "stop".into(),
            usage: None,
        })
    }
    async fn stream(&self, _r: CompletionRequest) -> Result<ChunkStream> {
        Err(Error::Provider("no stream".into()))
    }
}

/// Fails retryably every time — forces a failover hop on every iteration.
struct Fail(AtomicUsize);
#[async_trait::async_trait]
impl LlmProvider for Fail {
    fn capabilities(&self) -> ModelCapabilities {
        Ok1("").capabilities()
    }
    async fn complete(&self, _r: CompletionRequest) -> Result<CompletionResponse> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Err(Error::Provider("http 429: hop".into()))
    }
    async fn stream(&self, _r: CompletionRequest) -> Result<ChunkStream> {
        Err(Error::Provider("no stream".into()))
    }
}

fn up(id: &str, provider: Arc<dyn LlmProvider>) -> RouterUpstream {
    RouterUpstream {
        id: id.into(),
        tags: vec!["reasoning".into()],
        tier: PoolTier::Heavy,
        input_cost: 1.0,
        provider,
    }
}

fn req() -> CompletionRequest {
    CompletionRequest {
        messages: vec![Message::user("hi")],
        route: Some(RouteHint {
            role: Some(RouteRole::Judge),
            task_mode: Some(TaskMode::Review),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[tokio::test]
async fn routed_complete_with_failover_does_not_leak() {
    let _profiler = dhat::Profiler::builder().testing().build();
    // 16 members; the preference walks into the failing one first, so every
    // iteration pays a full decision + one failover hop + a success.
    let mut ups = vec![up("hopper", Arc::new(Fail(AtomicUsize::new(0))))];
    for i in 0..15 {
        ups.push(up(&format!("ok{i}"), Arc::new(Ok1("fine"))));
    }
    let policy = Policy {
        rules: vec![],
        default_prefer: Prefer {
            upstreams: vec!["hopper".into(), "ok0".into()],
            ..Default::default()
        },
    };
    // A generous breaker threshold keeps the hop on the hot path every time
    // (an open breaker would reorder it away and quietly measure less).
    let r = TaskRouter::new(ups, policy)
        .expect("router")
        .with_breaker(usize::MAX, 1);

    let _ = r.complete(req()).await; // warm up

    let base = dhat::HeapStats::get();
    const ITERS: u64 = 50;
    for _ in 0..ITERS {
        let resp = r.complete(req()).await.expect("fails over to ok0");
        assert_eq!(resp.message.content_text(), "fine");
    }
    let after = dhat::HeapStats::get();

    dhat::assert!(
        after.curr_blocks <= base.curr_blocks + 8,
        "live blocks grew (leak?): {} -> {}",
        base.curr_blocks,
        after.curr_blocks
    );
    // Decision + hint clone + failover scratch per call, over a 16-member
    // fleet: the borrowed-view path keeps this small. The budget is the guard
    // against reintroducing per-member id/tag clones (16 members would blow
    // straight through it).
    let per_iter = (after.total_blocks - base.total_blocks) / ITERS;
    dhat::assert!(per_iter < 120, "allocated {per_iter} blocks/run");
}
