//! Deterministic instruction-count bench for the graph-document **load** path —
//! exactly what `FileGraphs::get` does per read (and what `GraphService::Put`/
//! `Validate` do per call): textproto parse → wire→core decode → full typed
//! validation. Input is the `intermediate` corpus document (the shipped
//! example), printed once in setup; the node-type registry is prebuilt in setup
//! like the store prebuilds it. Runs at startup / on edit, never on the turn
//! hot path. Ceiling is an absolute `Ir` `hard_limits`; a regression fails
//! `cargo bench`.

use std::hint::black_box;

use agent_graph::{testdata, textproto, validate, NodeTypeRegistry};
use iai_callgrind::{
    library_benchmark, library_benchmark_group, main, Callgrind, EventKind, LibraryBenchmarkConfig,
};

fn setup() -> (String, NodeTypeRegistry) {
    let text = textproto::print(&testdata::intermediate()).expect("print");
    (text, NodeTypeRegistry::builtin())
}

// Observed 599,480 Ir (2026-08-09 optimization pass). The once-per-process
// descriptor-pool decode happens in setup (the `print` call touches the same
// `OnceLock` the store's long-lived registry does), so this measures the
// MARGINAL parse + decode + validate of a ~5-node document — dominated by
// prost-reflect text parsing, with validation a small tail. Startup/edit-time
// only, never the turn hot path — no fruit worth taking. Ceiling ~2.5×.
#[library_benchmark(config = LibraryBenchmarkConfig::default()
    .tool(Callgrind::default().hard_limits([(EventKind::Ir, 1_500_000u64)])))]
#[bench::intermediate_example(setup = setup)]
fn load_and_validate((text, registry): (String, NodeTypeRegistry)) -> usize {
    let doc = textproto::parse(&text).expect("parse");
    let issues = validate(&doc, &registry);
    // Path guard (the standing bench lesson): a wrong path must fail loudly,
    // not silently measure a cheaper branch.
    assert!(issues.is_empty(), "example must validate clean: {issues:?}");
    black_box(doc.nodes.len())
}

library_benchmark_group!(name = graph_load; benchmarks = load_and_validate);
main!(library_benchmark_groups = graph_load);
