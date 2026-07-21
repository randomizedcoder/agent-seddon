//! Allocation-budget leak test for the render path (parity spec 20).
//!
//! dhat's `Profiler` is a process-global singleton, so all assertions live in
//! ONE `#[test]` (see docs/components/benchmarking.md).

#![cfg(feature = "dhat-heap")]

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

use agent_core::Message;

fn transcript(n: usize) -> Vec<Message> {
    (0..n)
        .map(|i| {
            Message::user(format!(
                "turn {i}: a message body with <brackets> & entities"
            ))
        })
        .collect()
}

#[test]
fn render_frees_everything_and_stays_in_budget() {
    let profiler = dhat::Profiler::builder().testing().build();

    for _ in 0..32 {
        let _ = agent_export::bench_render(&transcript(200));
    }
    let mid = dhat::HeapStats::get();
    for _ in 0..32 {
        let _ = agent_export::bench_render(&transcript(200));
    }
    let end = dhat::HeapStats::get();

    assert_eq!(
        mid.curr_blocks, end.curr_blocks,
        "live blocks grew across iterations ({} -> {}): render leaks",
        mid.curr_blocks, end.curr_blocks
    );
    assert!(
        end.max_bytes < 16 * 1024 * 1024,
        "peak heap {} exceeds the render budget",
        end.max_bytes
    );
    drop(profiler);
}
