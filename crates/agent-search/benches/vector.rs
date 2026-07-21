//! Deterministic instruction-count benches for the semantic-search CPU hot paths:
//! the brute-force cosine scan (per query) and the RRF fusion of two ranked lists
//! (per hybrid query). Inputs are built purely from `n` (no clocks/randomness).
//! Ceilings are absolute `Ir` `hard_limits`; a regression fails `cargo bench`.

use std::hint::black_box;

use agent_core::{cosine_similarity, SearchHit};
use agent_search::rrf_fuse;
use iai_callgrind::{
    library_benchmark, library_benchmark_group, main, Callgrind, EventKind, LibraryBenchmarkConfig,
};

/// `n` deterministic 64-dim unit-ish vectors (a pseudo but fixed pattern).
fn corpus(n: usize) -> Vec<Vec<f32>> {
    (0..n)
        .map(|i| {
            (0..64)
                .map(|d| (((i * 31 + d * 7) % 13) as f32) / 13.0)
                .collect()
        })
        .collect()
}

// Scan cosine of a query against 500 corpus vectors. Observed ~small; ceiling generous.
#[library_benchmark(config = LibraryBenchmarkConfig::default()
    .tool(Callgrind::default().hard_limits([(EventKind::Ir, 1_500_000u64)])))]
fn cosine_scan_500() -> u64 {
    let vecs = black_box(corpus(500));
    let q = black_box(corpus(1).pop().unwrap());
    let mut acc = 0u64;
    for v in &vecs {
        // fold into an int so the compiler can't elide the work
        acc = acc.wrapping_add((cosine_similarity(&q, v) * 1e6) as u64);
    }
    black_box(acc)
}

fn hits(n: usize, salt: usize) -> Vec<SearchHit> {
    (0..n)
        .map(|i| SearchHit {
            path: std::path::PathBuf::from(format!("f{}.rs", (i * 7 + salt) % n)),
            line: 0,
            col_start: 0,
            col_end: 0,
            score: 0.0,
            snippet: String::new(),
        })
        .collect()
}

// Fuse two 200-element ranked lists. Observed ~small; ceiling generous.
#[library_benchmark(config = LibraryBenchmarkConfig::default()
    .tool(Callgrind::default().hard_limits([(EventKind::Ir, 1_400_000u64)])))]
fn rrf_fuse_200x2() -> usize {
    let lists = black_box(vec![hits(200, 0), hits(200, 3)]);
    black_box(rrf_fuse(&lists, 50).len())
}

library_benchmark_group!(name = vector; benchmarks = cosine_scan_500, rrf_fuse_200x2);
main!(library_benchmark_groups = vector);
