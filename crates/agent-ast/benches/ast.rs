//! Deterministic instruction-count benches for the `agent-ast` CPU hot paths: the
//! JSON→graph parse (per reindex) and the hottest query verbs — `callers` (backward
//! BFS), `implementations` (index lookup), and a capped `find_symbol` scan. Inputs
//! are built purely from `n` (no clocks/randomness), so the `Ir` counts are
//! reproducible.
//!
//! The graph build is done in each bench's `setup` (run OUTSIDE the callgrind
//! measurement), so a query bench measures *only* the traversal, not the parse — the
//! first cut measured ~38M Ir for every verb because the shared `built()` setup
//! dominated. Ceilings are absolute `Ir` `hard_limits`; a regression fails the bench.

use std::hint::black_box;

use agent_ast::graph::Graph;
use agent_core::{SymbolQuery, SymbolRef};
use iai_callgrind::{
    library_benchmark, library_benchmark_group, main, Callgrind, EventKind, LibraryBenchmarkConfig,
};

/// A deterministic helper-shaped JSON blob: `n` symbols in a chain (each calls the
/// next) plus one interface every even symbol "implements". Mirrors the real helper
/// output shape so the parse cost is representative.
fn blob(n: usize) -> String {
    let mut symbols = String::from("[");
    let mut edges = String::from("[");
    let mut implements = String::from("[");
    for i in 0..n {
        if i > 0 {
            symbols.push(',');
        }
        symbols.push_str(&format!(
            r#"{{"id":{i},"kind":"func","name":"Fn{i}","package":"example.com/p","file":"p/a.go","line":{i},"exported":true}}"#
        ));
        if i + 1 < n {
            if i > 0 {
                edges.push(',');
            }
            edges.push_str(&format!(r#"{{"caller_id":{i},"callee_id":{}}}"#, i + 1));
        }
        if i % 2 == 0 {
            if i > 0 {
                implements.push(',');
            }
            implements.push_str(&format!(r#"{{"type_id":{i},"interface_id":0}}"#));
        }
    }
    symbols.push(']');
    edges.push(']');
    implements.push(']');
    format!(
        r#"{{"symbols":{symbols},"edges":{edges},"implements":{implements},"imports":[],"packages":[],"truncated":false}}"#
    )
}

/// Setup (excluded from measurement): the raw blob + an existing root so `confine`
/// keeps the fixture symbols.
fn parse_input() -> (String, std::path::PathBuf) {
    (blob(1000), agent_testkit::tempdir())
}

/// Setup: a fully-built 1000-symbol graph, so query benches measure only the query.
fn built_1000() -> Graph {
    let (text, root) = parse_input();
    Graph::parse(&text, root.as_path()).unwrap()
}

// Parse a 1000-symbol helper blob into an indexed graph (serde_json + indexing).
#[library_benchmark(config = LibraryBenchmarkConfig::default()
    .tool(Callgrind::default().hard_limits([(EventKind::Ir, 42_000_000u64)])))]
#[bench::n1000(setup = parse_input)]
fn parse(input: (String, std::path::PathBuf)) -> usize {
    let (text, root) = input;
    Graph::parse(black_box(&text), black_box(root.as_path()))
        .unwrap()
        .symbol_count()
}

// Backward BFS (`callers`) over a 1000-node chain, max hops.
#[library_benchmark(config = LibraryBenchmarkConfig::default()
    .tool(Callgrind::default().hard_limits([(EventKind::Ir, 3_400_000u64)])))]
#[bench::n1000(setup = built_1000)]
fn callers(g: Graph) -> usize {
    black_box(g.callers(&black_box(SymbolRef::name("Fn999")), 8))
        .nodes
        .len()
}

// Interface-implementation lookup over a 1000-symbol graph.
#[library_benchmark(config = LibraryBenchmarkConfig::default()
    .tool(Callgrind::default().hard_limits([(EventKind::Ir, 4_400_000u64)])))]
#[bench::n1000(setup = built_1000)]
fn implementations(g: Graph) -> usize {
    black_box(g.implementations(&black_box(SymbolRef::name("Fn0")))).len()
}

// A capped substring `find_symbol` scan over a 1000-symbol graph.
#[library_benchmark(config = LibraryBenchmarkConfig::default()
    .tool(Callgrind::default().hard_limits([(EventKind::Ir, 3_300_000u64)])))]
#[bench::n1000(setup = built_1000)]
fn find_symbol(g: Graph) -> usize {
    let q = SymbolQuery {
        name: "Fn1".into(),
        kind: None,
        package: None,
        exact: false,
        limit: 50,
    };
    black_box(g.find_symbol(&black_box(q))).len()
}

library_benchmark_group!(
    name = ast;
    benchmarks = parse, callers, implementations, find_symbol
);
main!(library_benchmark_groups = ast);
