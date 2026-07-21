//! Deterministic instruction-count bench for the `web_fetch` HTML sanitizer.
//!
//! `web_fetch` decodes untrusted HTML on every fetch, so the single-pass tokenizer
//! feeding the markdown + text converters is a real CPU hot path. Input is built
//! purely from `n` (no clocks/randomness) via the shared `html_document` fixture.
//! The ceiling is an absolute `Ir` `hard_limits`; a regression fails `cargo bench`
//! → the `bench` check.

use std::hint::black_box;

use iai_callgrind::{
    library_benchmark, library_benchmark_group, main, Callgrind, EventKind, LibraryBenchmarkConfig,
};

// Convert a 200-block HTML document to both markdown and text (the two consumers
// of the tokenizer). Observed ~5.5M Ir; ceiling ~1.45×.
#[library_benchmark(config = LibraryBenchmarkConfig::default()
    .tool(Callgrind::default().hard_limits([(EventKind::Ir, 8_000_000u64)])))]
fn sanitize_200() -> usize {
    let html = agent_testkit::bench::html_document(black_box(200));
    black_box(agent_tools::bench_sanitize(black_box(&html)))
}

library_benchmark_group!(name = web; benchmarks = sanitize_200);
main!(library_benchmark_groups = web);
