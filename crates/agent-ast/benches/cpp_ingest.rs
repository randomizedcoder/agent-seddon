//! Deterministic instruction-count bench for the C/C++ engine's hot path: the
//! tree-sitter parse + extract + lower of a fixed multi-file C/C++ corpus into the
//! shared graph. The graph query verbs are already covered by `benches/ast.rs` (the
//! C/C++ engine feeds the identical `Graph`), so this measures only the parse/lower.
//!
//! Gated on `ast-cpp`: without the feature the file is just an empty `main` (so the
//! `harness = false` bench still links). Ceiling is an absolute `Ir` `hard_limit`.

#[cfg(not(feature = "ast-cpp"))]
fn main() {}

#[cfg(feature = "ast-cpp")]
use std::hint::black_box;

/// A deterministic C corpus: `n` files, each with a small call chain, so the parse +
/// name-resolution cost is representative and reproducible.
#[cfg(feature = "ast-cpp")]
fn corpus(root: &std::path::Path, n: usize) {
    for i in 0..n {
        let body = format!(
            "static int leaf{i}(int x){{return x+{i};}}\n\
             int mid{i}(int x){{return leaf{i}(x)+leaf{i}(x);}}\n\
             int top{i}(int x){{return mid{i}(x)+mid{i}(x);}}\n\
             struct T{i}{{int a;int b;}};\n"
        );
        std::fs::write(root.join(format!("f{i}.c")), body).unwrap();
    }
}

#[cfg(feature = "ast-cpp")]
fn ingest_input() -> std::path::PathBuf {
    let dir = agent_testkit::tempdir();
    corpus(&dir, 50);
    dir
}

// Parse + extract + lower a 50-file C corpus into the intermediate graph schema.
#[cfg(feature = "ast-cpp")]
#[iai_callgrind::library_benchmark(config = iai_callgrind::LibraryBenchmarkConfig::default()
    .tool(iai_callgrind::Callgrind::default().hard_limits([(iai_callgrind::EventKind::Ir, 54_000_000u64)])))]
#[bench::c50(setup = ingest_input)]
fn cpp_lower(dir: std::path::PathBuf) -> usize {
    agent_ast::lower_cpp_tree(black_box(dir.as_path()))
        .to_string()
        .len()
}

#[cfg(feature = "ast-cpp")]
iai_callgrind::library_benchmark_group!(
    name = cpp_ingest;
    benchmarks = cpp_lower
);

#[cfg(feature = "ast-cpp")]
iai_callgrind::main!(library_benchmark_groups = cpp_ingest);
