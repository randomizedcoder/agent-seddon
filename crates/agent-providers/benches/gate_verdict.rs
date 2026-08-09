//! Deterministic instruction-count bench for the consensus gate's per-round hot path
//! (`parse_verdict` + sanitization + the no-progress issue-set comparison) — it runs
//! once per critic round on every gated response, so its cost rides every final
//! answer. The LLM calls themselves are provider-bound and bench-SKIP; this guards
//! the parsing/sanitizing layer the gate adds. Input is built purely from constants
//! (no clock, no randomness). Ceiling is an absolute `Ir` `hard_limits`; a regression
//! fails `cargo bench`. See docs/design/cognition-graph/01-consensus-gate.md.

use std::hint::black_box;

use agent_providers::consensus::bench_verdict_round;
use iai_callgrind::{
    library_benchmark, library_benchmark_group, main, Callgrind, EventKind, LibraryBenchmarkConfig,
};

/// A worst-case-shaped verdict: prose + fenced JSON wrapper (the common model
/// misbehavior), a full issue list at the cap (some evidence-free, dropped), a full
/// alternatives list (one trigger-less, dropped), a hostile confidence.
fn verdict(issue_claims: &[&str]) -> String {
    let issues: Vec<String> = issue_claims
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let evidence = if i % 4 == 3 { "" } else { "checked against the repo" };
            format!(
                r#"{{"severity":"{}","claim":"{c} (site {i}, path crates/agent-x/src/lib.rs:{i})","evidence":"{evidence}"}}"#,
                match i % 3 {
                    0 => "high",
                    1 => "medium",
                    _ => "weaponized", // unknown → bounded to medium
                }
            )
        })
        .collect();
    let alts = r#"
      {"option":"sqlite","summary":"simpler ops, single file, no server to run","reconsider_when":"if the fleet story never materializes"},
      {"option":"clickhouse","summary":"append-only MergeTree scales across thousands of sessions","reconsider_when":"if sessions and analytics grow"},
      {"option":"grpc","summary":"central shared ledger","reconsider_when":""}"#;
    format!(
        "Here is my assessment of the answer.\n```json\n{{\"pass\": false, \"issues\": [{}], \"alternatives\": [{}], \"confidence\": 1e9}}\n```",
        issues.join(","),
        alts
    )
}

// One full round on capped-size lists, including the stalled-comparison against a
// previous round that shares half its claims — the realistic no-progress check.
// Observed ~200k Ir (serde-dominated: two ≤2KB JSON parses + sanitize + set compare;
// optimization pass 2026-08-09 found no fruit worth taking); ceiling ~2.5×.
#[library_benchmark(config = LibraryBenchmarkConfig::default()
    .tool(Callgrind::default().hard_limits([(EventKind::Ir, 500_000u64)])))]
fn verdict_round_full_lists() -> usize {
    let cur = verdict(&[
        "the flag is wrong",
        "the path does not exist",
        "the API has no such method",
        "vibes",
        "the test never runs",
        "the version pin is stale",
        "the error is swallowed",
        "the cap is unbounded",
    ]);
    let prev = verdict(&[
        "the flag is wrong",
        "the path does not exist",
        "the API has no such method",
        "vibes",
        "a different claim",
        "another different claim",
        "yet another claim",
        "the cap is unbounded",
    ]);
    black_box(bench_verdict_round(
        black_box(&cur),
        black_box(&prev),
        black_box(3),
    ))
}

library_benchmark_group!(name = gate_verdict; benchmarks = verdict_round_full_lists);
main!(library_benchmark_groups = gate_verdict);
