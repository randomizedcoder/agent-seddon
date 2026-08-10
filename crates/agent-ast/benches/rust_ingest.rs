//! Deterministic instruction-count bench for the Rust engine's unique hot path: the
//! charon `.llbc` → intermediate-graph lowering (`lower_llbc`). The shared graph
//! parse + query verbs are already covered by `benches/ast.rs` (the Rust engine feeds
//! the identical `Graph`), so this measures only the charon-specific transform over a
//! real, checked-in `.llbc` fixture.
//!
//! Gated on `ast-rust`: without the feature the file is just an empty `main` (so the
//! `harness = false` bench still links). Ceiling is an absolute `Ir` `hard_limit`.

#[cfg(not(feature = "ast-rust"))]
fn main() {}

#[cfg(feature = "ast-rust")]
use std::hint::black_box;

/// The real `charon --ullbc --no-dedup-serialized-ast` output for the `greeter`
/// fixture crate (the same blob the unit tests + nix check use).
#[cfg(feature = "ast-rust")]
const FIXTURE: &str = include_str!("../tests/fixtures/greeter.ullbc.json");

#[cfg(feature = "ast-rust")]
fn ingest_input() -> (String, std::path::PathBuf) {
    (FIXTURE.to_string(), agent_testkit::tempdir())
}

// Lower a real charon `.llbc` (serde_json parse + walk fun/type/trait decls, resolve
// static + CHA call edges) into the intermediate graph schema.
#[cfg(feature = "ast-rust")]
#[iai_callgrind::library_benchmark(config = iai_callgrind::LibraryBenchmarkConfig::default()
    .tool(iai_callgrind::Callgrind::default().hard_limits([(iai_callgrind::EventKind::Ir, 27_000_000u64)])))]
#[bench::greeter(setup = ingest_input)]
fn rust_lower(input: (String, std::path::PathBuf)) -> usize {
    let (json, root) = input;
    agent_ast::lower_llbc(black_box(&json), black_box(root.as_path()))
        .map(|v| v.to_string().len())
        .unwrap_or(0)
}

#[cfg(feature = "ast-rust")]
iai_callgrind::library_benchmark_group!(
    name = rust_ingest;
    benchmarks = rust_lower
);

#[cfg(feature = "ast-rust")]
iai_callgrind::main!(library_benchmark_groups = rust_ingest);
