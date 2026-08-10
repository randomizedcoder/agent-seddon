//! Heap leak + allocation-budget assertion for the fork engine under dhat:
//! the full spawn → race → **cancel** → merge → report cycle must free
//! everything it allocates — an aborted laggard branch must not strand its
//! task, request clone, or queue slot. Compiled only with
//! `--features dhat-heap,provider-branching`; `nix/checks/leak.nix` runs it.
#![cfg(all(feature = "dhat-heap", feature = "provider-branching"))]

use std::sync::Arc;
use std::time::Duration;

use agent_core::{
    CompletionRequest, CompletionResponse, LlmProvider, Message, ModelCapabilities, Result,
};
use agent_providers::{BranchCfg, BranchSpec, BranchingProvider, JoinPolicy};

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

struct Fast;
#[async_trait::async_trait]
impl LlmProvider for Fast {
    fn capabilities(&self) -> ModelCapabilities {
        ModelCapabilities::default()
    }
    async fn complete(&self, _r: CompletionRequest) -> Result<CompletionResponse> {
        Ok(CompletionResponse {
            message: Message::assistant("quick"),
            finish_reason: "stop".into(),
            usage: None,
        })
    }
}

/// Never finishes inside the test — every cycle aborts it mid-sleep.
struct Laggard;
#[async_trait::async_trait]
impl LlmProvider for Laggard {
    fn capabilities(&self) -> ModelCapabilities {
        ModelCapabilities::default()
    }
    async fn complete(&self, _r: CompletionRequest) -> Result<CompletionResponse> {
        tokio::time::sleep(Duration::from_secs(30)).await;
        Ok(CompletionResponse {
            message: Message::assistant("late"),
            finish_reason: "stop".into(),
            usage: None,
        })
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn fork_cancel_cycle_does_not_leak() {
    let _profiler = dhat::Profiler::builder().testing().build();
    let provider = BranchingProvider::new(
        "s",
        Arc::new(Fast),
        vec![
            BranchSpec {
                label: "fast".into(),
                lens: "a".into(),
                provider: Arc::new(Fast),
            },
            BranchSpec {
                label: "slow".into(),
                lens: "b".into(),
                provider: Arc::new(Laggard),
            },
        ],
    )
    .with_cfg(BranchCfg {
        policy: JoinPolicy::Any,
        ..BranchCfg::default()
    });
    let req = CompletionRequest {
        messages: vec![Message::user("task")],
        tools: Vec::new(),
        max_tokens: 64,
        temperature: 0.0,
        response_format: None,
        route: None,
    };

    // Warm-up: runtime/task-queue scratch counts as baseline, not growth.
    let r = provider.complete(req.clone()).await.expect("winner");
    assert_eq!(r.message.content_text(), "quick");
    // Let the aborted laggard's teardown settle before baselining.
    tokio::task::yield_now().await;

    let base = dhat::HeapStats::get();
    const ITERS: u64 = 30;
    for _ in 0..ITERS {
        let r = provider.complete(req.clone()).await.expect("winner");
        assert_eq!(r.message.content_text(), "quick");
    }
    tokio::task::yield_now().await;
    let end = dhat::HeapStats::get();

    // Every cycle spawned two tasks and aborted one mid-sleep: all of it must
    // be freed again (an abort that strands the branch would show up here).
    assert!(
        end.curr_bytes <= base.curr_bytes + 4 * 1024,
        "live heap grew across {ITERS} fork/cancel cycles: {} -> {} bytes",
        base.curr_bytes,
        end.curr_bytes
    );
    let per_cycle = (end.total_bytes - base.total_bytes) / ITERS;
    assert!(
        per_cycle < 128 * 1024,
        "fork cycle allocates {per_cycle} bytes — budget blown"
    );
}
