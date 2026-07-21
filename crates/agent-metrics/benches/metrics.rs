//! Exemplar iai-callgrind benchmark — the pattern every feature PR follows.
//!
//! Each case runs under callgrind for a **deterministic instruction count** (no
//! wall-clock noise). Each carries an **absolute `Ir` ceiling** (`hard_limits`);
//! exceeding it fails `cargo bench` (exit 3), which gates `nix flake check` via the
//! `bench` check. Ceilings are ~1.4× the observed count — bump one here (the diff
//! records it) when a legitimate change moves a bench.
//!
//! Run with `nix run .#bench` or, in the dev shell, `cargo bench -p agent-metrics`
//! (both put `valgrind` + the matching `iai-callgrind-runner` on PATH).
//!
//! Note: iai-callgrind's `#[library_benchmark]` macro rejects `///` doc comments on
//! the benched fn, so those are plain `//` comments.

use std::hint::black_box;

use agent_metrics::Metrics;
use iai_callgrind::{
    library_benchmark, library_benchmark_group, main, Callgrind, EventKind, LibraryBenchmarkConfig,
};

// Constructing the shared registry (registers the metric families) — the
// per-process startup cost of observability. Grows as each seam adds families
// (specs 11–19). Observed ~453k Ir; ceiling has headroom for more seams.
#[library_benchmark(config = LibraryBenchmarkConfig::default()
    .tool(Callgrind::default().hard_limits([(EventKind::Ir, 650_000u64)])))]
fn new_registry() -> Metrics {
    black_box(Metrics::new())
}

// The hot observability path: record many tool-exec samples, then encode the text
// exposition once (what a `/metrics` scrape does). Grows as seams add metric
// families (web/tasks/structured/lsp/sandbox/embed — specs 11–15). Observed ~813k Ir.
#[library_benchmark(config = LibraryBenchmarkConfig::default()
    .tool(Callgrind::default().hard_limits([(EventKind::Ir, 1_000_000u64)])))]
fn record_and_encode() -> String {
    let m = Metrics::new();
    for _ in 0..100 {
        m.on_tool_exec(black_box("edit"), black_box(0.001));
    }
    black_box(m.encode_text())
}

library_benchmark_group!(
    name = metrics;
    benchmarks = new_registry, record_and_encode
);

main!(library_benchmark_groups = metrics);
