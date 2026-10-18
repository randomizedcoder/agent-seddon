use agent_repo_graph::{extract::extract, ExtractBudget};
use iai_callgrind::{
    library_benchmark, library_benchmark_group, main, Callgrind, EventKind, LibraryBenchmarkConfig,
};
use std::{hint::black_box, path::Path};

// Observed ~8.70M Ir, including the parser thread via its explicit collection toggle.
#[library_benchmark(config = LibraryBenchmarkConfig::default()
    .tool(Callgrind::with_args(["toggle-collect=agent_repo_graph::extract::rust::emit"])
        .hard_limits([(EventKind::Ir, 12_000_000u64)])))]
fn workspace() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ws");
    black_box(extract(&root, &ExtractBudget::default()).unwrap());
}

library_benchmark_group!(name = extraction; benchmarks = workspace);
main!(library_benchmark_groups = extraction);
