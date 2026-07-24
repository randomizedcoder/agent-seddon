//! Heap leak + allocation-budget assertion for a switch compaction, under dhat.
//! A reshape must free the dropped middle and its intermediate render buffer and
//! stay under a per-iteration budget. Compiled only with `--features dhat-heap`
//! (and `context-mode-aware`); `nix/checks/leak.nix` runs it.
#![cfg(all(feature = "dhat-heap", feature = "context-mode-aware"))]

use std::sync::Arc;

use agent_context::ModeAwareWindow;
use agent_core::{
    CompletionRequest, CompletionResponse, ContextStrategy, LlmProvider, Message,
    ModelCapabilities, Result, Role, TaskMode, TokenBudget, Usage, WorkingSet,
};
use async_trait::async_trait;

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

struct FixedSummarizer;
#[async_trait]
impl LlmProvider for FixedSummarizer {
    fn capabilities(&self) -> ModelCapabilities {
        ModelCapabilities {
            supports_tools: false,
            context_window: 1000,
            supports_response_format: false,
            supports_vision: false,
        }
    }
    async fn complete(&self, _req: CompletionRequest) -> Result<CompletionResponse> {
        Ok(CompletionResponse {
            message: Message::assistant("SUMMARY"),
            finish_reason: "stop".into(),
            usage: Some(Usage::default()),
        })
    }
}

fn long(role: Role, n: usize) -> Message {
    let content = "x ".repeat(n);
    match role {
        Role::System => Message::system(content),
        Role::User => Message::user(content),
        Role::Assistant => Message::assistant(content),
        Role::Tool => Message::tool("id", content),
    }
}

fn fixture() -> WorkingSet {
    WorkingSet {
        messages: vec![
            long(Role::System, 20),
            long(Role::User, 400),
            long(Role::Assistant, 400),
            long(Role::User, 400),
            long(Role::Assistant, 30),
        ],
    }
}

#[tokio::test]
async fn switch_compaction_does_not_leak() {
    let _profiler = dhat::Profiler::builder().testing().build();
    let strat = ModeAwareWindow::new(Arc::new(FixedSummarizer), 200);
    let budget = TokenBudget {
        max_context_tokens: 100_000,
        reserve_output: 1000,
    };

    // Warm up.
    strat.on_mode_switch(TaskMode::Other, TaskMode::Implement);
    let mut w = fixture();
    strat.compact(&mut w, &budget).await.unwrap();

    let base = dhat::HeapStats::get();
    const ITERS: u64 = 50;
    for _ in 0..ITERS {
        strat.on_mode_switch(TaskMode::Implement, TaskMode::Review);
        let mut working = fixture();
        strat.compact(&mut working, &budget).await.unwrap();
        // `working` drops here — the reshaped set must be freed.
    }
    let after = dhat::HeapStats::get();

    dhat::assert!(
        after.curr_blocks <= base.curr_blocks + 8,
        "live blocks grew (leak?): {} -> {}",
        base.curr_blocks,
        after.curr_blocks
    );
    let per_iter = (after.total_blocks - base.total_blocks) / ITERS;
    dhat::assert!(per_iter < 400, "allocated {per_iter} blocks/run");
}
