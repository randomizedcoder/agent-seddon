//! Deterministic instruction-count bench for `estimate_tokens` — called in the
//! compaction loop's drop condition, so its cost matters. Input is built purely
//! from `n` (no clocks/randomness). Ceiling is an absolute `Ir` `hard_limits`;
//! a regression fails `cargo bench` → the `bench` check.

use std::hint::black_box;

use agent_core::Message;
use iai_callgrind::{
    library_benchmark, library_benchmark_group, main, Callgrind, EventKind, LibraryBenchmarkConfig,
};

/// A synthetic `n`-message history (alternating roles, fixed body).
fn history(n: usize) -> Vec<Message> {
    (0..n)
        .map(|i| {
            let body = "token ".repeat(20);
            if i % 2 == 0 {
                Message::user(body)
            } else {
                Message::assistant(body)
            }
        })
        .collect()
}

// Estimate tokens over a 500-message window. Observed ~206k Ir; ceiling ~1.4×.
#[library_benchmark(config = LibraryBenchmarkConfig::default()
    .tool(Callgrind::default().hard_limits([(EventKind::Ir, 300_000u64)])))]
fn estimate_tokens_500() -> u32 {
    let msgs = history(black_box(500));
    black_box(agent_context::bench_estimate_tokens(black_box(&msgs)))
}

library_benchmark_group!(name = context; benchmarks = estimate_tokens_500);
main!(library_benchmark_groups = context);
