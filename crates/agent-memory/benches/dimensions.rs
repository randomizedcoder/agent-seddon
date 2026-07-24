//! Deterministic instruction-count bench for the per-step classify **parse**
//! (`bench_parse_step`) — the sync hot path that turns the model's step response
//! into typed summaries before the (async, unmeasured) per-dimension writes. Input
//! is a fixed JSON string (no clocks/randomness). Ceiling is an absolute `Ir`
//! `hard_limits`; a regression fails `cargo bench` → the `bench` check.
//! Built only with `memory-dimensions` (`required-features` in Cargo.toml).

use std::hint::black_box;

use iai_callgrind::{
    library_benchmark, library_benchmark_group, main, Callgrind, EventKind, LibraryBenchmarkConfig,
};

/// A fixed step response with `n` per-dimension summaries.
fn step_json(n: usize) -> String {
    let items: Vec<String> = (0..n)
        .map(|i| {
            format!(r#"{{"dimension":"coding","summary":"summary number {i}","is_new":false}}"#)
        })
        .collect();
    format!(r#"{{"summaries":[{}]}}"#, items.join(","))
}

// Parse a 32-summary step. Observed ~102k Ir (serde over a fixed string built in
// the measured region); ceiling ~2×.
#[library_benchmark(config = LibraryBenchmarkConfig::default()
    .tool(Callgrind::default().hard_limits([(EventKind::Ir, 200_000u64)])))]
fn parse_step_32() -> usize {
    let json = step_json(black_box(32));
    black_box(agent_memory::bench_parse_step(black_box(&json)))
}

library_benchmark_group!(name = dimensions; benchmarks = parse_step_32);
main!(library_benchmark_groups = dimensions);
