//! Deterministic instruction-count bench for transcript rendering.
//!
//! The render is a pure function of the transcript, so a stable output implies a
//! stable instruction count — a move here means the render actually changed.

use std::hint::black_box;

use agent_core::Message;
use iai_callgrind::{
    library_benchmark, library_benchmark_group, main, Callgrind, EventKind, LibraryBenchmarkConfig,
};

fn transcript(n: usize) -> Vec<Message> {
    (0..n)
        .map(|i| {
            let body = format!(
                "turn {i}: a realistic message body with some <angle> brackets & entities, \
                 plus a path like crates/agent-core/src/lib.rs and a bit more prose."
            );
            if i % 2 == 0 {
                Message::user(body)
            } else {
                Message::assistant(body)
            }
        })
        .collect()
}

// All three formats over a 200-message transcript.
#[library_benchmark(config = LibraryBenchmarkConfig::default()
    .tool(Callgrind::default().hard_limits([(EventKind::Ir, 12_000_000u64)])))]
fn render_200() -> usize {
    let t = transcript(black_box(200));
    black_box(agent_export::bench_render(black_box(&t)))
}

library_benchmark_group!(name = render; benchmarks = render_200);
main!(library_benchmark_groups = render);
