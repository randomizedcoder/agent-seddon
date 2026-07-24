//! Deterministic instruction-count bench for the switch-compaction **partition**
//! (`bench_mode_partition`) — the sync hot path that decides which turn is kept
//! verbatim vs demoted on a mode switch. The async summarize call is not measured
//! here (it's I/O, not the partition); the leak test exercises the full path.
//! Input is built purely from `n` (no clocks/randomness). Ceiling is an absolute
//! `Ir` `hard_limits`; a regression fails `cargo bench` → the `bench` check.
//! Built only with `context-mode-aware` (`required-features` in Cargo.toml).

use std::hint::black_box;

use agent_core::Message;
use iai_callgrind::{
    library_benchmark, library_benchmark_group, main, Callgrind, EventKind, LibraryBenchmarkConfig,
};

/// A synthetic history: one system head, then `n` alternating turns.
fn history(n: usize) -> Vec<Message> {
    let mut msgs = vec![Message::system("system prompt")];
    for i in 0..n {
        let body = "token ".repeat(20);
        msgs.push(if i % 2 == 0 {
            Message::user(body)
        } else {
            Message::assistant(body)
        });
    }
    // A trailing tool result to exercise the orphan-fold nudge.
    msgs.push(Message::tool("id", "result"));
    msgs
}

// Partition a 500-turn window (keep_recent_tokens = 400). The walk is O(tail) plus
// the per-message `estimate_tokens`; the input is built inside the measured region
// (like the sibling `context` bench), so most of the count is the 500 allocations.
// Observed ~445k Ir; ceiling ~1.5×.
#[library_benchmark(config = LibraryBenchmarkConfig::default()
    .tool(Callgrind::default().hard_limits([(EventKind::Ir, 700_000u64)])))]
fn partition_500() -> (usize, usize) {
    let msgs = history(black_box(500));
    black_box(agent_context::bench_mode_partition(
        black_box(&msgs),
        black_box(400),
    ))
}

library_benchmark_group!(name = mode_aware; benchmarks = partition_500);
main!(library_benchmark_groups = mode_aware);
