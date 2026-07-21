//! Deterministic instruction-count bench for the `OutputSchema` validator.
//!
//! Validation runs on every structured completion (and every repair attempt), so
//! the recursive `type`/`required`/`properties`/`items` walk is a real CPU hot
//! path. Input is built purely from constants (no clocks/randomness). The ceiling
//! is an absolute `Ir` `hard_limits`; a regression fails `cargo bench` → the
//! `bench` check.

use std::hint::black_box;

use iai_callgrind::{
    library_benchmark, library_benchmark_group, main, Callgrind, EventKind, LibraryBenchmarkConfig,
};
use serde_json::{json, Value};

/// A nested object schema with a typed array and required fields — the shape a
/// structured subagent return exercises.
fn schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["files", "confidence", "meta"],
        "properties": {
            "files": {"type": "array", "items": {"type": "string"}},
            "confidence": {"type": "number", "minimum": 0.0, "maximum": 1.0},
            "meta": {
                "type": "object",
                "required": ["kind"],
                "properties": {
                    "kind": {"type": "string", "enum": ["add", "edit", "delete"]},
                    "count": {"type": "integer"}
                }
            }
        }
    })
}

fn value() -> Value {
    json!({
        "files": ["a.rs", "b.rs", "c.rs", "d.rs"],
        "confidence": 0.75,
        "meta": {"kind": "edit", "count": 4}
    })
}

// Validate a matching value against the nested schema. Observed ~39k Ir; ceiling ~1.5×.
#[library_benchmark(config = LibraryBenchmarkConfig::default()
    .tool(Callgrind::default().hard_limits([(EventKind::Ir, 58_000u64)])))]
fn validate_nested() -> usize {
    let schema = black_box(schema());
    let value = black_box(value());
    black_box(agent_validate::bench_validate(&schema, &value))
}

library_benchmark_group!(name = validate; benchmarks = validate_nested);
main!(library_benchmark_groups = validate);
