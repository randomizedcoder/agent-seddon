//! iai-callgrind bench for the tokenizer CPU hot path — `ApproxTokenizer::count_text`
//! over a large code-like buffer. `count`/`count_messages` are thin async folds on
//! top of this, so guarding the sync core guards the loop cost. Deterministic
//! (no clocks/randomness); the Ir ceiling lives in `nix/checks/bench.nix`.

use agent_tokenizer::ApproxTokenizer;
use iai_callgrind::{
    library_benchmark, library_benchmark_group, main, Callgrind, EventKind, LibraryBenchmarkConfig,
};
use std::hint::black_box;

/// A ~4 KB mixed code/prose buffer (words, long identifiers, dense punctuation,
/// unicode) — the input shape the byte heuristic mis-estimates worst.
fn corpus() -> String {
    let unit = "fn compute_something(really_long_identifier: i32) -> Result<i32, Error> {\n    \
                let café = really_long_identifier + 42; // héllo wörld\n    Ok(café * 2)\n}\n";
    unit.repeat(24)
}

// Observed ~103k Ir over a ~4 KB buffer; ceiling ~1.4×.
#[library_benchmark(config = LibraryBenchmarkConfig::default()
    .tool(Callgrind::default().hard_limits([(EventKind::Ir, 145_000u64)])))]
fn count_large_buffer() -> u32 {
    let tok = ApproxTokenizer::new();
    let text = corpus();
    black_box(tok.count_text(black_box(&text)))
}

library_benchmark_group!(
    name = tokenize;
    benchmarks = count_large_buffer
);

main!(library_benchmark_groups = tokenize);
