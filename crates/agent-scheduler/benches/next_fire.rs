//! Deterministic instruction-count bench for cron next-fire computation.
//!
//! The scan steps a minute at a time, so a badly-matching expression is the
//! expensive case — that is what this measures. Fully clock-injected, so the
//! input is a constant and the count is stable.

use std::hint::black_box;

use iai_callgrind::{
    library_benchmark, library_benchmark_group, main, Callgrind, EventKind, LibraryBenchmarkConfig,
};

/// 2024-01-01T00:00:00Z.
const T0: u64 = 1_704_067_200_000;

// A once-a-year expression: the scan walks ~a year of minutes, the worst
// realistic case.
#[library_benchmark(config = LibraryBenchmarkConfig::default()
    .tool(Callgrind::default().hard_limits([(EventKind::Ir, 120_000_000u64)])))]
fn next_fire_sparse() -> Option<u64> {
    black_box(agent_scheduler::schedule::bench_next_fire(
        black_box("0 0 1 1 *"),
        black_box(T0 + 60_000),
    ))
}

// The common case: every five minutes.
#[library_benchmark(config = LibraryBenchmarkConfig::default()
    .tool(Callgrind::default().hard_limits([(EventKind::Ir, 200_000u64)])))]
fn next_fire_dense() -> Option<u64> {
    black_box(agent_scheduler::schedule::bench_next_fire(
        black_box("*/5 * * * *"),
        black_box(T0),
    ))
}

library_benchmark_group!(name = next_fire; benchmarks = next_fire_sparse, next_fire_dense);
main!(library_benchmark_groups = next_fire);
