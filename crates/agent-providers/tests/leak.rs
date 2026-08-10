//! Heap leak + allocation-budget assertion for the pool's dispatch path, under
//! dhat. The in-flight guard must free its slot on every path — success and panic
//! — and selection must not leak scratch. Compiled only with
//! `--features dhat-heap,provider-pool`; `nix/checks/leak.nix` runs it.
#![cfg(all(feature = "dhat-heap", feature = "provider-pool"))]

use std::sync::Arc;

use agent_core::{
    CompletionRequest, CompletionResponse, LlmPool, LlmProvider, Message, ModelCapabilities,
    PoolTier, Result,
};
use agent_providers::{Candidate, PoolPolicy, PoolProvider, PoolSpec};

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

struct Ok1(&'static str);
#[async_trait::async_trait]
impl LlmProvider for Ok1 {
    fn capabilities(&self) -> ModelCapabilities {
        ModelCapabilities {
            supports_tools: true,
            context_window: 1000,
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
}

fn member(name: &'static str) -> PoolSpec {
    PoolSpec {
        candidate: Candidate {
            name: name.into(),
            provider: Arc::new(Ok1(name)),
        },
        tier: PoolTier::Light,
        cost: 0.0,
        weight: 1.0,
        max_concurrency: 0,
    }
}

fn req() -> CompletionRequest {
    CompletionRequest {
        messages: vec![Message::user("hi")],
        tools: vec![],
        max_tokens: 16,
        temperature: 0.0,
        response_format: None,
        route: None,
    }
}

#[tokio::test]
async fn dispatch_does_not_leak_and_releases_in_flight() {
    let _profiler = dhat::Profiler::builder().testing().build();
    let p = PoolProvider::new("test", vec![member("a"), member("b"), member("c")])
        .expect("pool")
        .with_policy(PoolPolicy::LeastLoaded);

    let _ = p.complete_all(req(), PoolTier::Light, 3).await; // warm up

    let base = dhat::HeapStats::get();
    const ITERS: u64 = 50;
    for _ in 0..ITERS {
        let out = p.complete_all(req(), PoolTier::Light, 3).await;
        assert_eq!(out.len(), 3);
    }
    let after = dhat::HeapStats::get();

    dhat::assert!(
        after.curr_blocks <= base.curr_blocks + 8,
        "live blocks grew (leak?): {} -> {}",
        base.curr_blocks,
        after.curr_blocks
    );
    // Every dispatch released its in-flight slot.
    let h = p.health().await;
    for m in &h.members {
        dhat::assert!(
            m.in_flight == 0,
            "member {} leaked load: {}",
            m.name,
            m.in_flight
        );
    }
    let per_iter = (after.total_blocks - base.total_blocks) / ITERS;
    dhat::assert!(per_iter < 400, "allocated {per_iter} blocks/run");
}
