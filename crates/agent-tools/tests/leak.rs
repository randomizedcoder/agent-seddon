//! Heap leak + allocation-budget assertion for `apply_patch`, under dhat.
//!
//! `apply_patch` is async and uses `tokio::fs`, whose blocking-thread pool retains
//! buffers by design — so a one-shot "live blocks return to zero" check would
//! misread steady-state pooling as a leak. Instead this runs the tool many times
//! and asserts **live blocks stay flat** across iterations (a real leak grows
//! linearly) plus a per-iteration allocation budget. Compiled only with
//! `--features dhat-heap,tool-patch`; `nix/checks/leak.nix` runs it.
#![cfg(all(feature = "dhat-heap", feature = "tool-patch"))]

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

use agent_core::{Tool, ToolContext};
use agent_testkit::tempdir;

async fn apply_once(ctx: &ToolContext) {
    // Reset the file so every iteration does identical work.
    std::fs::write(ctx.cwd.join("f.txt"), "before\nkeep\n").unwrap();
    let obs = agent_tools::ApplyPatchTool
        .execute(
            serde_json::json!({
                "patch": "*** Begin Patch\n*** Update File: f.txt\n@@\n-before\n+after\n keep\n*** End Patch"
            }),
            ctx,
        )
        .await
        .unwrap();
    assert!(!obs.is_error, "{}", obs.content);
}

#[tokio::test]
async fn apply_patch_does_not_leak() {
    let _profiler = dhat::Profiler::builder().testing().build();
    let dir = tempdir();
    let ctx = ToolContext { cwd: dir.clone() };

    // Warm up so tokio's blocking-pool buffers reach steady state.
    apply_once(&ctx).await;

    let base = dhat::HeapStats::get();
    const ITERS: u64 = 50;
    for _ in 0..ITERS {
        apply_once(&ctx).await;
    }
    let after = dhat::HeapStats::get();

    // No leak: live blocks are flat across 50 runs (a real leak would grow ~50×).
    // A tiny slack absorbs incidental pool growth.
    dhat::assert!(
        after.curr_blocks <= base.curr_blocks + 8,
        "live blocks grew across runs (leak?): {} -> {}",
        base.curr_blocks,
        after.curr_blocks
    );
    // Allocation budget: bounded allocations per apply (catches a per-line/per-hunk
    // allocation blow-up).
    let per_iter = (after.total_blocks - base.total_blocks) / ITERS;
    dhat::assert!(per_iter < 100, "apply allocated {per_iter} blocks/run");
}
