//! Deterministic instruction-count bench for the `apply_patch` parser.
//!
//! The parser is the novel, pure hot path (the applier is filesystem-bound); an
//! accidental `O(n²)` in hunk scanning would surface here. Input is built from a
//! constant (no clocks/randomness) so the count is reproducible. Ceiling is an
//! absolute `Ir` `hard_limits` — a regression fails `cargo bench` → the `bench`
//! check. See docs/benchmarking.md.

use std::hint::black_box;

use iai_callgrind::{
    library_benchmark, library_benchmark_group, main, Callgrind, EventKind, LibraryBenchmarkConfig,
};

/// A patch envelope with `n` update hunks over one file — a realistic multi-hunk
/// batch, built purely from `n`.
fn envelope(n: usize) -> String {
    let mut s = String::from("*** Begin Patch\n*** Update File: src/lib.rs\n");
    for i in 0..n {
        s.push_str(&format!(
            "@@ fn f{i} @@\n-    let x{i} = {i};\n+    let x{i} = {i} + 1;\n"
        ));
    }
    s.push_str("*** End Patch");
    s
}

// Parse a 200-hunk envelope. Observed ~1.0M Ir; ceiling ~1.4×.
#[library_benchmark(config = LibraryBenchmarkConfig::default()
    .tool(Callgrind::default().hard_limits([(EventKind::Ir, 1_400_000u64)])))]
fn parse_200_hunks() -> usize {
    let patch = envelope(black_box(200));
    black_box(agent_tools::parse_op_count(black_box(&patch)))
}

library_benchmark_group!(name = patch; benchmarks = parse_200_hunks);
main!(library_benchmark_groups = patch);
