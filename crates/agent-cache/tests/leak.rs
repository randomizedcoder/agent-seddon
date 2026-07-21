//! Allocation-budget leak test for breakpoint placement (parity spec 24).
//!
//! Placement runs on every turn, before the request is serialized. dhat's
//! `Profiler` is a process-global singleton, so all assertions live in ONE
//! `#[test]` (see docs/components/benchmarking.md).

#![cfg(feature = "dhat-heap")]

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

use agent_core::{CacheCapabilities, CacheStrategy, Message, PromptShape};

fn history(n: usize) -> Vec<Message> {
    (0..n)
        .map(|i| Message::user(format!("turn {i}: ordinary conversational content")))
        .collect()
}

#[test]
fn placement_frees_everything_and_stays_in_budget() {
    let profiler = dhat::Profiler::builder().testing().build();
    let msgs = history(200);
    let anthropic = CacheCapabilities::anthropic();
    let openai = CacheCapabilities::openai();

    // Iteration-based: assert live blocks are FLAT across runs.
    let run = || {
        let shape = PromptShape::new(true, 12, &msgs);
        let _ = agent_cache::StablePrefix.place(&shape, &anthropic);
        let _ = agent_cache::TailWindow::default().place(&shape, &anthropic);
        // The key path allocates a scratch buffer; make sure it is released too.
        let _ = agent_cache::StablePrefix.place(&shape, &openai);
    };
    for _ in 0..64 {
        run();
    }
    let mid = dhat::HeapStats::get();
    for _ in 0..64 {
        run();
    }
    let end = dhat::HeapStats::get();

    assert_eq!(
        mid.curr_blocks, end.curr_blocks,
        "live blocks grew across iterations ({} -> {}): placement leaks",
        mid.curr_blocks, end.curr_blocks
    );
    assert!(
        end.max_bytes < 4 * 1024 * 1024,
        "peak heap {} exceeds the placement budget",
        end.max_bytes
    );
    drop(profiler);
}
