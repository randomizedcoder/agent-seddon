//! Heap **leak** + **allocation-budget** assertions, run under the dhat profiler.
//!
//! Only compiled with `--features dhat-heap`, which installs dhat's global
//! allocator (below). `nix/checks/leak.nix` runs `cargo test --features dhat-heap`,
//! so a leak or an allocation blow-up fails `nix flake check`. This is the exemplar
//! every feature PR mirrors for its own hot path.
#![cfg(feature = "dhat-heap")]

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

use agent_metrics::Metrics;

#[test]
fn metrics_registry_does_not_leak() {
    let _profiler = dhat::Profiler::builder().testing().build();

    let before = dhat::HeapStats::get().curr_blocks;
    {
        let m = Metrics::new();
        for _ in 0..100 {
            m.on_tool_exec("edit", 0.001);
        }
        let _ = m.encode_text();
    }
    let stats = dhat::HeapStats::get();

    // No leak: every block allocated inside the scope was freed by drop.
    dhat::assert_eq!(stats.curr_blocks, before);
    // Allocation budget: a generous ceiling that still catches an accidental
    // per-record / per-encode allocation blow-up (the "major low-hanging fruit").
    dhat::assert!(stats.total_blocks < 20_000);
}
