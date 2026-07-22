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
// per-process startup cost of observability. Observed ~651k Ir with the full
// 30-spec seam set. Like the encode bench below, this is linear in the number of
// registered families and steps up as seams land; ~1.4x headroom.
#[library_benchmark(config = LibraryBenchmarkConfig::default()
    .tool(Callgrind::default().hard_limits([(EventKind::Ir, 900_000u64)])))]
fn new_registry() -> Metrics {
    black_box(Metrics::new())
}

// The hot observability path: record many tool-exec samples, then encode the text
// exposition once (what a `/metrics` scrape does). Grows as seams add metric
// families. Observed ~1.17M Ir with the full 30-spec seam set (web/tasks/
// structured/lsp/sandbox/embed, content blocks, scanner, cache breakpoints,
// route decisions, hooks, forge, scheduled runs, and pty). Encoding is linear in
// the number of registered families, so this ceiling steps up as seams land
// rather than staying fixed; ~1.4x headroom over the observed value.
#[library_benchmark(config = LibraryBenchmarkConfig::default()
    .tool(Callgrind::default().hard_limits([(EventKind::Ir, 1_650_000u64)])))]
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
