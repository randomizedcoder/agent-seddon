//! Heap leak + allocation-budget assertions for the filesystem tools, under dhat.
//!
//! These tools are async and use `tokio::fs`, whose blocking-thread pool retains
//! buffers by design — so a one-shot "live blocks return to zero" check would
//! misread steady-state pooling as a leak. Instead each test runs the tool many
//! times and asserts **live blocks stay flat** across iterations (a real leak grows
//! linearly) plus a per-iteration allocation budget. Compiled only with
//! `--features dhat-heap`; `nix/checks/leak.nix` runs it (with the tool features on).
#![cfg(feature = "dhat-heap")]

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

use agent_core::{Tool, ToolContext};
use agent_testkit::tempdir;

/// Run `body` 50× after a warm-up and assert live blocks stay flat (no leak) and
/// allocations per run stay under `max_blocks_per_run`.
async fn assert_no_leak<F, Fut>(max_blocks_per_run: u64, mut body: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    body().await; // warm up tokio's blocking-pool buffers to steady state
    let base = dhat::HeapStats::get();
    const ITERS: u64 = 50;
    for _ in 0..ITERS {
        body().await;
    }
    let after = dhat::HeapStats::get();
    dhat::assert!(
        after.curr_blocks <= base.curr_blocks + 8,
        "live blocks grew across runs (leak?): {} -> {}",
        base.curr_blocks,
        after.curr_blocks
    );
    let per_iter = (after.total_blocks - base.total_blocks) / ITERS;
    dhat::assert!(
        per_iter < max_blocks_per_run,
        "allocated {per_iter} blocks/run (> {max_blocks_per_run})"
    );
}

// One test builds the single dhat `Profiler` (it is a process-global singleton, so
// two parallel leak tests would conflict) and checks each tool's hot path in turn.
#[tokio::test]
async fn tools_do_not_leak() {
    let _profiler = dhat::Profiler::builder().testing().build();
    let dir = tempdir();
    let ctx = ToolContext { cwd: dir.clone() };

    #[cfg(feature = "tool-patch")]
    assert_no_leak(100, || async {
        // Reset the file so every iteration does identical work.
        std::fs::write(ctx.cwd.join("f.txt"), "before\nkeep\n").unwrap();
        let obs = agent_tools::ApplyPatchTool
            .execute(
                serde_json::json!({
                    "patch": "*** Begin Patch\n*** Update File: f.txt\n@@\n-before\n+after\n keep\n*** End Patch"
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert!(!obs.is_error, "{}", obs.content);
    })
    .await;

    #[cfg(feature = "tool-core")]
    assert_no_leak(100, || async {
        let w = agent_tools::WriteFileTool
            .execute(
                serde_json::json!({ "path": "rw.txt", "content": "hello\nworld\n" }),
                &ctx,
            )
            .await
            .unwrap();
        assert!(!w.is_error, "{}", w.content);
        let r = agent_tools::ReadFileTool
            .execute(serde_json::json!({ "path": "rw.txt" }), &ctx)
            .await
            .unwrap();
        assert!(!r.is_error, "{}", r.content);
    })
    .await;

    // bash spawns a subprocess each run; assert the parent-side Command/pipe/output
    // allocations don't leak across runs.
    #[cfg(feature = "tool-core")]
    assert_no_leak(200, || async {
        let obs = agent_tools::BashTool
            .execute(serde_json::json!({ "command": "printf hi" }), &ctx)
            .await
            .unwrap();
        assert!(!obs.is_error, "{}", obs.content);
    })
    .await;
}
