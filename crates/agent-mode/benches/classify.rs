//! Deterministic instruction-count bench for the mode prefilter — the free path
//! that runs every turn. Input is built purely from constants (no clocks/randomness)
//! so callgrind counts are stable. The ceiling is an absolute `Ir` `hard_limits`;
//! a regression fails `cargo bench` → the `bench` flake check.

use std::hint::black_box;

use iai_callgrind::{
    library_benchmark, library_benchmark_group, main, Callgrind, EventKind, LibraryBenchmarkConfig,
};

/// A realistic ambiguous prompt (no early-exit cue) — the prefilter's worst case,
/// since it scans every cue set before returning `None`.
fn prompt() -> String {
    "could you take a look at this and let me know what you think about the overall shape".repeat(4)
}

// Scan every cue set over a ~350-char prompt. The prefilter is `O(cues × len)`
// `contains` calls; this pins its instruction count. Observed ~17k Ir; ceiling ~1.8×.
#[library_benchmark(config = LibraryBenchmarkConfig::default()
    .tool(Callgrind::default().hard_limits([(EventKind::Ir, 30_000u64)])))]
fn prefilter_ambiguous() -> Option<agent_core::TaskMode> {
    let p = black_box(prompt());
    black_box(agent_mode::bench_prefilter(black_box(&p)))
}

library_benchmark_group!(name = classify; benchmarks = prefilter_ambiguous);
main!(library_benchmark_groups = classify);
