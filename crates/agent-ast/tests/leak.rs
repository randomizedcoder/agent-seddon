//! Heap leak + allocation-budget assertions for the `agent-ast` graph hot paths,
//! under dhat. Compiled only with `--features dhat-heap`; `nix/checks/leak.nix` runs
//! it. Pins that (a) parsing the helper JSON into an indexed graph and (b) repeated
//! queries over a held graph free everything they allocate across iterations — the
//! long-lived `--serve-ast` path holds one graph and answers many queries.

#![cfg(feature = "dhat-heap")]

use agent_ast::graph::Graph;
use agent_core::{SymbolQuery, SymbolRef};

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

/// A deterministic helper-shaped blob: `n` symbols in a call chain plus one
/// interface every even symbol implements (mirrors the bench fixture).
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

#[test]
fn ast_graph_paths_do_not_leak() {
    let _profiler = dhat::Profiler::builder().testing().build();

    let text = blob(500);
    let root = agent_testkit::tempdir();
    let root = root.as_path();

    // Parse (ingest) hot path: build the indexed graph repeatedly, dropping each.
    let _warm = Graph::parse(&text, root).unwrap();
    let base = dhat::HeapStats::get();
    const PARSE_ITERS: u64 = 40;
    for _ in 0..PARSE_ITERS {
        let g = Graph::parse(&text, root).unwrap();
        assert_eq!(g.symbol_count(), 500);
        drop(g);
    }
    let after = dhat::HeapStats::get();
    dhat::assert!(
        after.curr_blocks <= base.curr_blocks + 8,
        "parse: live blocks grew (leak?): {} -> {}",
        base.curr_blocks,
        after.curr_blocks
    );

    // Query hot path over one held graph (the `--serve-ast` shape): many queries, no
    // growth in live blocks.
    let g = Graph::parse(&text, root).unwrap();
    let q = SymbolQuery {
        name: "Fn1".into(),
        kind: None,
        package: None,
        exact: false,
        limit: 50,
    };
    let _ = g.callers(&SymbolRef::name("Fn499"), 8); // warm
    let qbase = dhat::HeapStats::get();
    const QUERY_ITERS: u64 = 100;
    for _ in 0..QUERY_ITERS {
        let _ = g.callers(&SymbolRef::name("Fn499"), 8);
        let _ = g.implementations(&SymbolRef::name("Fn0"));
        let _ = g.find_symbol(&q);
    }
    let qafter = dhat::HeapStats::get();
    dhat::assert!(
        qafter.curr_blocks <= qbase.curr_blocks + 8,
        "query: live blocks grew (leak?): {} -> {}",
        qbase.curr_blocks,
        qafter.curr_blocks
    );
    let per_iter = (qafter.total_blocks - qbase.total_blocks) / QUERY_ITERS;
    dhat::assert!(per_iter < 4096, "query: allocated {per_iter} blocks/run");

    // The Rust engine's charon `.llbc` → graph ingest hot path (feature `ast-rust`):
    // lowering the real fixture + building the graph repeatedly frees everything. Same
    // process-wide profiler (dhat allows only one), so it lives inside this one test.
    #[cfg(feature = "ast-rust")]
    {
        const FIXTURE: &str = include_str!("fixtures/greeter.ullbc.json");
        let build = || {
            let v = agent_ast::lower_llbc(FIXTURE, root).unwrap();
            Graph::parse_value(v, root)
        };
        let _warm = build();
        let rbase = dhat::HeapStats::get();
        const INGEST_ITERS: u64 = 40;
        for _ in 0..INGEST_ITERS {
            let g = build();
            assert!(g.symbol_count() > 0);
            drop(g);
        }
        let rafter = dhat::HeapStats::get();
        dhat::assert!(
            rafter.curr_blocks <= rbase.curr_blocks + 8,
            "rust ingest: live blocks grew (leak?): {} -> {}",
            rbase.curr_blocks,
            rafter.curr_blocks
        );
    }

    // The C/C++ engine's tree-sitter parse → graph ingest hot path (feature `ast-cpp`):
    // parsing a small C corpus + building the graph repeatedly frees everything.
    #[cfg(feature = "ast-cpp")]
    {
        let dir = agent_testkit::tempdir();
        for i in 0..8 {
            std::fs::write(
                dir.join(format!("f{i}.c")),
                format!(
                    "static int leaf{i}(int x){{return x+{i};}}\n\
                     int top{i}(int x){{return leaf{i}(x)+leaf{i}(x);}}\n\
                     struct T{i}{{int a;}};\n"
                ),
            )
            .unwrap();
        }
        let build = || Graph::parse_value(agent_ast::lower_cpp_tree(&dir), &dir);
        let _warm = build();
        let cbase = dhat::HeapStats::get();
        const CPP_ITERS: u64 = 30;
        for _ in 0..CPP_ITERS {
            let g = build();
            assert!(g.symbol_count() > 0);
            drop(g);
        }
        let cafter = dhat::HeapStats::get();
        dhat::assert!(
            cafter.curr_blocks <= cbase.curr_blocks + 8,
            "cpp ingest: live blocks grew (leak?): {} -> {}",
            cbase.curr_blocks,
            cafter.curr_blocks
        );
    }
}
