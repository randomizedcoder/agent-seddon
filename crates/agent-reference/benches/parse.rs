//! Deterministic instruction-count bench for the `@`-reference parser.
//!
//! Every turn's prompt is scanned for `@file`/`@dir`/`@symbol`/`@url` mentions
//! before it is sent, so the manual char-scanner in [`agent_reference::parse`] is a
//! real CPU hot path. Input is built purely from constants (no clocks/randomness).
//! The ceiling is an absolute `Ir` `hard_limits`; a regression fails `cargo bench`
//! → the `bench` check.

use std::hint::black_box;

use iai_callgrind::{
    library_benchmark, library_benchmark_group, main, Callgrind, EventKind, LibraryBenchmarkConfig,
};

/// A prose prompt carrying a mix of every reference kind, a quoted path with a
/// range, an email that must NOT match, and a duplicate to exercise the dedup set.
fn prompt() -> String {
    let mut s = String::from(
        "Port @symbol:AuthService per @url:https://spec.test/rfc into @dir:src/auth, \
         following @file:src/lib.rs:40-80 and @file:\"my dir/a b.rs\":10-20. \
         Mail me at foo@bar.example when @file:src/lib.rs:40-80 lands.",
    );
    // Pad with prose containing a stray `@` sign to stress the boundary scan.
    for _ in 0..8 {
        s.push_str(" just a normal clause with an @ sign and no reference here.");
    }
    s
}

// Parse the mixed prompt. Observed ~small Ir; ceiling ~1.5× headroom.
#[library_benchmark(config = LibraryBenchmarkConfig::default()
    .tool(Callgrind::default().hard_limits([(EventKind::Ir, 120_000u64)])))]
fn parse_mixed() -> usize {
    let prompt = black_box(prompt());
    black_box(agent_reference::bench_parse(&prompt))
}

library_benchmark_group!(name = parse; benchmarks = parse_mixed);
main!(library_benchmark_groups = parse);
