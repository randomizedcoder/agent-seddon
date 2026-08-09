//! Heap leak + allocation-budget assertion for the graph-document load cycle
//! under dhat: parse → validate → drop over the shipped example document must
//! free everything it allocates and stay under a per-cycle budget (an editor
//! calling `Validate` on every drag must not grow the server). Compiled only
//! with `--features dhat-heap`; `nix/checks/leak.nix` runs it.
#![cfg(feature = "dhat-heap")]

use agent_graph::{testdata, textproto, validate, NodeTypeRegistry};

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

#[test]
fn load_validate_cycle_does_not_leak() {
    let _profiler = dhat::Profiler::builder().testing().build();
    let registry = NodeTypeRegistry::builtin();
    let text = textproto::print(&testdata::intermediate()).expect("print");
    // Warm-up: prost-reflect's descriptor pool + parser tables are built
    // lazily on first parse — count them as baseline, not growth.
    let doc = textproto::parse(&text).expect("warm-up parse");
    assert!(validate(&doc, &registry).is_empty());
    drop(doc);

    let base = dhat::HeapStats::get();
    const ITERS: u64 = 50;
    for _ in 0..ITERS {
        let doc = textproto::parse(&text).expect("parse");
        let issues = validate(&doc, &registry);
        assert!(issues.is_empty(), "{issues:?}");
    }
    let end = dhat::HeapStats::get();

    // Everything allocated by the cycles must be freed again.
    assert!(
        end.curr_bytes <= base.curr_bytes,
        "live heap grew across {ITERS} load cycles: {} -> {} bytes",
        base.curr_bytes,
        end.curr_bytes
    );
    // And the per-cycle allocation volume stays bounded (parse + decode +
    // validation scratch for a ~5-node document).
    let per_cycle = (end.total_bytes - base.total_bytes) / ITERS;
    assert!(
        per_cycle < 256 * 1024,
        "load cycle allocates {per_cycle} bytes — budget blown"
    );
}
