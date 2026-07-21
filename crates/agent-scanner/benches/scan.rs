//! Deterministic instruction-count bench for the content scan — the genuine CPU
//! hot path: every side-effecting tool call scans its argument/body before the
//! `Policy` gate decides, so this runs on the critical path of the loop.
//!
//! Input is built purely from a fixed corpus (no clocks/randomness). The ceiling
//! is an absolute `Ir` `hard_limits`; a regression fails `cargo bench` → the
//! `bench` check. The OSV path is network-bound and is deliberately not benched.

use std::hint::black_box;

use iai_callgrind::{
    library_benchmark, library_benchmark_group, main, Callgrind, EventKind, LibraryBenchmarkConfig,
};

/// A realistic multi-KB source-like buffer with one secret near the end, so the
/// regex set and the entropy pass both do full work.
fn corpus() -> String {
    let mut s = String::with_capacity(16 * 1024);
    for i in 0..200 {
        s.push_str(&format!(
            "// line {i}: ordinary source text with identifiers and punctuation;\n\
             let value_{i} = compute(argument_{i}, \"a normal string literal\");\n"
        ));
    }
    s.push_str("aws_secret = \"AKIAIOSFODNN7EXAMPLE\"\n");
    s
}

/// The same buffer with NO findings — the production-common case, since almost
/// every scanned body is clean. Worth measuring separately: the matching path
/// pays extra to recover spans, so a single number would hide the cost that
/// actually dominates in practice.
fn clean_corpus() -> String {
    let mut s = String::with_capacity(16 * 1024);
    for i in 0..200 {
        s.push_str(&format!(
            "// line {i}: ordinary source text with identifiers and punctuation;\n\
             let value_{i} = compute(argument_{i}, \"a normal string literal\");\n"
        ));
    }
    s
}

// Secret + entropy scan over ~16KB containing one secret. Observed ~7.0M Ir;
// ceiling ~1.4x. Patterns run in ASCII mode (`(?i-u)`, `(?-u:\w)`): Unicode
// case-folding and Unicode `\w` build far larger automata and cost ~2.8x more on
// the threat set, for vocabulary that is ASCII by construction.
#[library_benchmark(config = LibraryBenchmarkConfig::default()
    .tool(Callgrind::default().hard_limits([(EventKind::Ir, 10_000_000u64)])))]
fn scan_secrets_16k() -> usize {
    let buf = corpus();
    black_box(agent_scanner::bench_scan_secrets(black_box(&buf)))
}

// Threat-pattern scan over the same buffer. Observed ~21.1M Ir; ceiling ~1.4x.
#[library_benchmark(config = LibraryBenchmarkConfig::default()
    .tool(Callgrind::default().hard_limits([(EventKind::Ir, 30_000_000u64)])))]
fn scan_threats_16k() -> usize {
    let buf = corpus();
    black_box(agent_scanner::bench_scan_threats(black_box(&buf)))
}

// Both scans over clean content — what almost every real call looks like.
#[library_benchmark(config = LibraryBenchmarkConfig::default()
    .tool(Callgrind::default().hard_limits([(EventKind::Ir, 40_000_000u64)])))]
fn scan_clean_16k() -> usize {
    let buf = clean_corpus();
    black_box(
        agent_scanner::bench_scan_secrets(black_box(&buf))
            + agent_scanner::bench_scan_threats(black_box(&buf)),
    )
}

library_benchmark_group!(
    name = scan;
    benchmarks = scan_secrets_16k, scan_threats_16k, scan_clean_16k
);
main!(library_benchmark_groups = scan);
