//! Deterministic instruction-count bench for result normalization: dedup by
//! canonical URL, stable ranking, and the output caps. Runs on every search.
//! The HTTP backends are network-bound and are deliberately not benched.

use std::hint::black_box;

use agent_core::WebResult;
use iai_callgrind::{
    library_benchmark, library_benchmark_group, main, Callgrind, EventKind, LibraryBenchmarkConfig,
};

/// 200 results with duplicates and tied scores — the work ranking actually does.
fn results(n: usize) -> Vec<WebResult> {
    (0..n)
        .map(|i| WebResult {
            // Every 4th URL duplicates an earlier one (in a different case /
            // with a fragment), so dedup does real work.
            url: if i % 4 == 0 {
                format!("HTTPS://Example.test/page{}#frag", i / 2)
            } else {
                format!("https://example.test/page{i}?utm_source=x")
            },
            title: format!("Result number {i} about asynchronous rust"),
            snippet: "A reasonably sized snippet of text as a provider would return it.".repeat(3),
            score: ((i % 7) as f32) / 7.0,
            published_ms: None,
        })
        .collect()
}

// Observed ~1.24M Ir for 200 results; ceiling ~1.4x.
//
// Was 5.49M until `canonical_url` was hoisted out of the sort comparator: it
// allocates, and a comparator calling it runs O(n log n) times on both sides.
// Canonicalizing once per result instead cut this 4.4x.
#[library_benchmark(config = LibraryBenchmarkConfig::default()
    .tool(Callgrind::default().hard_limits([(EventKind::Ir, 1_750_000u64)])))]
fn rank_200() -> usize {
    let r = results(black_box(200));
    black_box(agent_web_search::bench_rank(black_box(r)))
}

library_benchmark_group!(name = rank; benchmarks = rank_200);
main!(library_benchmark_groups = rank);
