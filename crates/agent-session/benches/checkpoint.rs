//! Deterministic instruction-count bench for the per-turn CPU hot path: serialize
//! a conversation working set + content-hash it (what every `checkpoint` does).
//! Input is built purely from `n` (no clocks/randomness). Ceiling is an absolute
//! `Ir` `hard_limits`; a regression fails `cargo bench` → the `bench` check. The
//! gRPC + disk-write paths are I/O-bound and not benched.

use std::hint::black_box;

use agent_core::{Message, Role};
use agent_session::content_id;
use iai_callgrind::{
    library_benchmark, library_benchmark_group, main, Callgrind, EventKind, LibraryBenchmarkConfig,
};

/// An `n`-message conversation (alternating user/assistant, fixed bodies).
fn conversation(n: usize) -> Vec<Message> {
    (0..n)
        .map(|i| Message {
            role: if i % 2 == 0 {
                Role::User
            } else {
                Role::Assistant
            },
            content: format!("turn {i}: consider the retry backoff and the config parse path"),
            tool_calls: vec![],
            tool_call_id: None,
        })
        .collect()
}

// Content-hash a 100-message working set. Observed ~235k Ir; ceiling ~1.5×.
#[library_benchmark(config = LibraryBenchmarkConfig::default()
    .tool(Callgrind::default().hard_limits([(EventKind::Ir, 350_000u64)])))]
fn content_id_100() -> String {
    let msgs = black_box(conversation(100));
    black_box(content_id(
        &msgs,
        black_box(Some("parent-id")),
        black_box("turn"),
    ))
}

library_benchmark_group!(name = checkpoint; benchmarks = content_id_100);
main!(library_benchmark_groups = checkpoint);
