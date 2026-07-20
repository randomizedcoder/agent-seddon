//! Deterministic instruction-count bench for the `edit` hot path.
//!
//! Exact replacement is a cheap string scan; the interesting cost is the **fuzzy**
//! fallback, whose line-window matching is `O(lines × old_lines)` — an accidental
//! blow-up would surface here. Input is built purely from `n` (no clocks/randomness).
//! Ceiling is an absolute `Ir` `hard_limits`; a regression fails `cargo bench` →
//! the `bench` check.

use std::hint::black_box;

use iai_callgrind::{
    library_benchmark, library_benchmark_group, main, Callgrind, EventKind, LibraryBenchmarkConfig,
};

/// An `n`-line file whose target line uses smart quotes, so the exact match fails
/// and the fuzzy fallback scans the whole file.
fn file(n: usize) -> String {
    let mut s = String::new();
    for i in 0..n {
        if i == n / 2 {
            s.push_str("    log(\u{2018}target\u{2019});\n");
        } else {
            s.push_str(&format!("    let x{i} = {i};\n"));
        }
    }
    s
}

// Fuzzy-replace the smart-quote line in a 400-line file (exact fails → full scan).
// Observed ~1.41M Ir; ceiling ~1.4×.
#[library_benchmark(config = LibraryBenchmarkConfig::default()
    .tool(Callgrind::default().hard_limits([(EventKind::Ir, 2_000_000u64)])))]
fn fuzzy_replace_400() -> usize {
    let content = file(black_box(400));
    black_box(agent_tools::bench_apply(
        black_box(&content),
        black_box("    log('target');"),
        black_box("    log('changed');"),
        black_box(true),
    ))
}

library_benchmark_group!(name = edit; benchmarks = fuzzy_replace_400);
main!(library_benchmark_groups = edit);
