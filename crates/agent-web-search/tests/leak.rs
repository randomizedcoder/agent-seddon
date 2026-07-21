//! Allocation-budget leak test for the ranking path (parity spec 12).
//!
//! dhat's `Profiler` is a process-global singleton, so all assertions live in
//! ONE `#[test]` (see docs/components/benchmarking.md).

#![cfg(feature = "dhat-heap")]

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

use agent_core::WebResult;

fn results(n: usize) -> Vec<WebResult> {
    (0..n)
        .map(|i| WebResult {
            url: format!("https://example.test/page{}?utm_source=x", i % 150),
            title: format!("Result {i}"),
            snippet: "a snippet as a provider would return it".repeat(4),
            score: ((i % 7) as f32) / 7.0,
            published_ms: None,
        })
        .collect()
}

#[test]
fn ranking_frees_everything_and_stays_in_budget() {
    let profiler = dhat::Profiler::builder().testing().build();

    for _ in 0..64 {
        let _ = agent_web_search::bench_rank(results(200));
    }
    let mid = dhat::HeapStats::get();
    for _ in 0..64 {
        let _ = agent_web_search::bench_rank(results(200));
    }
    let end = dhat::HeapStats::get();

    assert_eq!(
        mid.curr_blocks, end.curr_blocks,
        "live blocks grew across iterations ({} -> {}): ranking leaks",
        mid.curr_blocks, end.curr_blocks
    );
    assert!(
        end.max_bytes < 8 * 1024 * 1024,
        "peak heap {} exceeds the ranking budget",
        end.max_bytes
    );
    drop(profiler);
}
