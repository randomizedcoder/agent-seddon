//! Heap leak + allocation-budget assertion for the local sandbox exec-driver
//! path, under dhat. Each `exec` spawns a subprocess and captures its output; this
//! pins that the parent-side Command/pipe/capture allocations free across runs
//! (the local backend needs no external binary, so this stays hermetic).
//! Compiled only with `--features dhat-heap`; `nix/checks/leak.nix` runs it.
#![cfg(feature = "dhat-heap")]

use agent_core::{ExecSpec, Sandbox};
use agent_sandbox::LocalSandbox;
use agent_testkit::tempdir;

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

#[tokio::test]
async fn local_exec_does_not_leak() {
    let _profiler = dhat::Profiler::builder().testing().build();
    let dir = tempdir();
    let sandbox = LocalSandbox;

    // Warm up (first spawn touches the blocking pool), then measure a flat
    // live-block count across N execs.
    let _ = sandbox
        .exec(&ExecSpec::sh("printf hi", dir.clone()))
        .await
        .unwrap();
    let base = dhat::HeapStats::get();

    const ITERS: u64 = 60;
    for _ in 0..ITERS {
        let out = sandbox
            .exec(&ExecSpec::sh("printf hi", dir.clone()))
            .await
            .unwrap();
        assert_eq!(out.stdout, "hi");
    }
    let after = dhat::HeapStats::get();

    dhat::assert!(
        after.curr_blocks <= base.curr_blocks + 8,
        "live blocks grew (leak?): {} -> {}",
        base.curr_blocks,
        after.curr_blocks
    );
    let per_iter = (after.total_blocks - base.total_blocks) / ITERS;
    dhat::assert!(per_iter < 300, "allocated {per_iter} blocks/run");
}
