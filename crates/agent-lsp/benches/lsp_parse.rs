//! Deterministic instruction-count bench for the diagnostics JSON-RPC parse.
//!
//! A `publishDiagnostics` payload is deserialized into the seam's `Diagnostic`
//! structs after every edit — the per-edit CPU hot path. Input is built purely
//! from `n` (no clocks/randomness). The ceiling is an absolute `Ir` `hard_limits`;
//! a regression fails `cargo bench` → the `bench` check.

use std::hint::black_box;

use iai_callgrind::{
    library_benchmark, library_benchmark_group, main, Callgrind, EventKind, LibraryBenchmarkConfig,
};
use serde_json::{json, Value};

/// A `publishDiagnostics` params with `n` diagnostics (a realistic post-edit batch).
fn payload(n: usize) -> Value {
    let diags: Vec<Value> = (0..n)
        .map(|i| {
            json!({
                "range": {"start": {"line": i, "character": 4}, "end": {"line": i, "character": 20}},
                "severity": (i % 4) + 1,
                "code": format!("E0{i:03}"),
                "source": "rustc",
                "message": "expected u32, found String in this position"
            })
        })
        .collect();
    json!({"uri": "file:///src/main.rs", "diagnostics": diags})
}

// Build + parse a 50-diagnostic payload (input construction is measured too, like
// the edit/web benches). Observed ~1.19M Ir; ceiling ~1.5×.
#[library_benchmark(config = LibraryBenchmarkConfig::default()
    .tool(Callgrind::default().hard_limits([(EventKind::Ir, 1_800_000u64)])))]
fn parse_diagnostics_50() -> usize {
    let params = black_box(payload(50));
    black_box(agent_lsp::bench_parse_diagnostics(&params))
}

library_benchmark_group!(name = lsp_parse; benchmarks = parse_diagnostics_50);
main!(library_benchmark_groups = lsp_parse);
