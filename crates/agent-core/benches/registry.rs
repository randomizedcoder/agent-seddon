//! Deterministic instruction-count bench for `ToolRegistry::describe_all` — the
//! per-turn schema assembly the tool-calling loop performs (sort by name + clone
//! each `ToolSchema`). An accidental `O(n²)` or an extra clone would surface here.
//!
//! The loop's *parallelism* is a wall-clock property (validated by the
//! concurrency tests in `agent-runtime`), not an instruction count — so this
//! benches the one deterministic CPU cost the loop pays each turn. Ceiling is an
//! absolute `Ir` `hard_limits`; a regression fails `cargo bench` → the `bench`
//! check. Input is built purely from `n` (no clocks/randomness).

use std::hint::black_box;
use std::sync::Arc;

use agent_core::{Observation, Result, Tool, ToolContext, ToolRegistry, ToolSchema};
use iai_callgrind::{
    library_benchmark, library_benchmark_group, main, Callgrind, EventKind, LibraryBenchmarkConfig,
};

struct Dummy(String);

#[async_trait::async_trait]
impl Tool for Dummy {
    fn name(&self) -> &str {
        &self.0
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.0.clone(),
            description: "a dummy tool".into(),
            parameters: serde_json::json!({"type": "object"}),
        }
    }
    async fn execute(&self, _a: serde_json::Value, _c: &ToolContext) -> Result<Observation> {
        Ok(Observation::ok(""))
    }
}

fn registry(n: usize) -> ToolRegistry {
    let mut r = ToolRegistry::new();
    for i in 0..n {
        r.register(Arc::new(Dummy(format!("tool_{i:03}"))));
    }
    r
}

// Build a 64-tool registry and describe it (register + sort + clone). Observed
// ~250k Ir; ceiling ~1.4×.
#[library_benchmark(config = LibraryBenchmarkConfig::default()
    .tool(Callgrind::default().hard_limits([(EventKind::Ir, 350_000u64)])))]
fn describe_all_64() -> Vec<ToolSchema> {
    let r = registry(black_box(64));
    black_box(r.describe_all())
}

library_benchmark_group!(name = registry; benchmarks = describe_all_64);
main!(library_benchmark_groups = registry);
