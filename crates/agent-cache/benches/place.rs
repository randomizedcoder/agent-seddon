//! Deterministic instruction-count bench for breakpoint placement — it runs on
//! every turn of the loop, before the request is serialized.

use std::hint::black_box;

use agent_core::Message;
use iai_callgrind::{
    library_benchmark, library_benchmark_group, main, Callgrind, EventKind, LibraryBenchmarkConfig,
};

fn history(n: usize) -> Vec<Message> {
    (0..n)
        .map(|i| Message::user(format!("turn {i}: some ordinary conversational content")))
        .collect()
}

// Placement over a 200-message window with 12 tools.
#[library_benchmark(config = LibraryBenchmarkConfig::default()
    .tool(Callgrind::default().hard_limits([(EventKind::Ir, 400_000u64)])))]
fn place_200() -> usize {
    let msgs = history(black_box(200));
    black_box(agent_cache::bench_place(black_box(&msgs), black_box(12)))
}

library_benchmark_group!(name = place; benchmarks = place_200);
main!(library_benchmark_groups = place);
