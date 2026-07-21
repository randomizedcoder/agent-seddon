//! Heap leak + allocation-budget assertion for the validator, under dhat. The
//! recursive walk builds path strings + an error vec per call; this pins that
//! repeated validate calls free everything and stay under a per-iteration budget.
//! Compiled only with `--features dhat-heap`; `nix/checks/leak.nix` runs it.
#![cfg(feature = "dhat-heap")]

use agent_core::OutputSchema;
use agent_validate::Draft07Validator;
use serde_json::json;

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

#[test]
fn validate_does_not_leak() {
    let _profiler = dhat::Profiler::builder().testing().build();
    let validator = Draft07Validator::new();
    let schema = json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["files", "meta"],
        "properties": {
            "files": {"type": "array", "items": {"type": "string"}},
            "meta": {"type": "object", "required": ["kind"],
                     "properties": {"kind": {"type": "string", "enum": ["a", "b"]}}}
        }
    });
    // A value with one violation, so the error path (string building) is exercised.
    let value = json!({"files": ["x.rs", 3], "meta": {"kind": "a"}});

    let _ = validator.validate(&schema, &value); // warm up
    let base = dhat::HeapStats::get();
    const ITERS: u64 = 200;
    for _ in 0..ITERS {
        let _ = validator.validate(&schema, &value);
    }
    let after = dhat::HeapStats::get();

    dhat::assert!(
        after.curr_blocks <= base.curr_blocks + 8,
        "live blocks grew (leak?): {} -> {}",
        base.curr_blocks,
        after.curr_blocks
    );
    let per_iter = (after.total_blocks - base.total_blocks) / ITERS;
    dhat::assert!(per_iter < 64, "allocated {per_iter} blocks/run");
}
