//! Heap leak + allocation-budget assertion for a per-step dimensional summarize,
//! under dhat. The pass must free the rendered transcript and per-dimension write
//! buffers and stay under a per-iteration budget. Compiled only with
//! `--features dhat-heap,memory-dimensions`; `nix/checks/leak.nix` runs it.
#![cfg(all(feature = "dhat-heap", feature = "memory-dimensions"))]

use std::sync::Arc;

use agent_core::{
    CompletionRequest, CompletionResponse, DimensionStore, LlmProvider, MemoryEvent, Message,
    ModelCapabilities, Result, Usage,
};
use agent_memory::FileDimensions;
use agent_testkit::tempdir;
use async_trait::async_trait;

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

struct FixedDim;
#[async_trait]
impl LlmProvider for FixedDim {
    fn capabilities(&self) -> ModelCapabilities {
        ModelCapabilities {
            supports_tools: false,
            context_window: 4000,
            supports_response_format: false,
            supports_vision: false,
        }
    }
    async fn complete(&self, _req: CompletionRequest) -> Result<CompletionResponse> {
        Ok(CompletionResponse {
            message: Message::assistant(
                r#"{"summaries":[{"dimension":"coding","summary":"did a thing"}]}"#,
            ),
            finish_reason: "stop".into(),
            usage: Some(Usage::default()),
        })
    }
}

fn evt(text: &str) -> MemoryEvent {
    MemoryEvent {
        kind: "goal".into(),
        message: Message::user(text),
        ts_ms: 0,
        session_id: String::new(),
        usage: None,
        iter: None,
        verification: None,
        review: None,
        dimensional: None,
    }
}

#[tokio::test]
async fn summarize_step_does_not_leak() {
    let _profiler = dhat::Profiler::builder().testing().build();
    let td = tempdir();
    let d = FileDimensions::new(td.as_path().join("dimensions")).with_provider(Arc::new(FixedDim));
    let events = vec![evt("do the work"), evt("more work")];

    d.summarize_step(&events).await.unwrap(); // warm up

    let base = dhat::HeapStats::get();
    const ITERS: u64 = 30;
    for _ in 0..ITERS {
        d.summarize_step(&events).await.unwrap();
    }
    let after = dhat::HeapStats::get();

    dhat::assert!(
        after.curr_blocks <= base.curr_blocks + 8,
        "live blocks grew (leak?): {} -> {}",
        base.curr_blocks,
        after.curr_blocks
    );
    let per_iter = (after.total_blocks - base.total_blocks) / ITERS;
    dhat::assert!(per_iter < 2_000, "allocated {per_iter} blocks/run");
}
