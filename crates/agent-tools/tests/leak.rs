//! Heap leak + allocation-budget assertions for the filesystem tools, under dhat.
//!
//! These tools are async and use `tokio::fs`, whose blocking-thread pool retains
//! buffers by design — so a one-shot "live blocks return to zero" check would
//! misread steady-state pooling as a leak. Instead each test runs the tool many
//! times and asserts **live blocks stay flat** across iterations (a real leak grows
//! linearly) plus a per-iteration allocation budget. Compiled only with
//! `--features dhat-heap`; `nix/checks/leak.nix` runs it (with the tool features on).
//!
//! The flatness check is done over **two consecutive windows**, not a single absolute
//! delta from a one-shot warm-up. One-time lazy initialization (tokio spinning up a
//! blocking-pool thread on demand, `tracing` callsite interning, a regex/`ignore`
//! global cache) allocates permanent blocks the *first* time a code path fires; under
//! the sequential gate leak derivation that first fire can land inside the measured
//! window and read as a leak (this caused a recurring `+8` flake). By definition a
//! real leak grows *every* window, while one-time init only shows up in the first —
//! so we absorb window 1 (base→mid may grow) and assert the **second** window
//! (mid→after) is flat. That is the semantically correct leak test and is immune to
//! where one-time init happens to land.
#![cfg(feature = "dhat-heap")]

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

use agent_core::{Tool, ToolContext};
use agent_testkit::tempdir;

/// Run `body` over two consecutive windows of `ITERS` runs each (after a warm-up) and
/// assert the **second** window's live blocks stay flat (a real leak grows every
/// window; one-time lazy init only shows up in the first) and allocations per run stay
/// under `max_blocks_per_run`.
async fn assert_no_leak<F, Fut>(max_blocks_per_run: u64, mut body: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    // Small absolute slack for the flat *second* window. It is not a per-iteration
    // budget (that's `max_blocks_per_run` below) — it only absorbs a stray one-time
    // allocation that happens to fire in window 2 rather than window 1.
    const WINDOW_SLACK: usize = 8;
    const ITERS: u64 = 50;

    body().await; // warm up tokio's blocking-pool buffers toward steady state
    let base = dhat::HeapStats::get();
    for _ in 0..ITERS {
        body().await;
    }
    // Window 1 (base -> mid) absorbs one-time lazy init (pool threads, tracing
    // callsites, regex/ignore caches); we deliberately do NOT assert on its growth.
    let mid = dhat::HeapStats::get();
    for _ in 0..ITERS {
        body().await;
    }
    let after = dhat::HeapStats::get();

    // Window 2 (mid -> after): with all one-time init behind us, live blocks must be
    // flat. A genuine per-iteration leak keeps growing here and still fails.
    let window2_growth = after.curr_blocks.saturating_sub(mid.curr_blocks);
    dhat::assert!(
        window2_growth <= WINDOW_SLACK,
        "live blocks still growing in the second window (leak?): base {} -> mid {} -> after {}",
        base.curr_blocks,
        mid.curr_blocks,
        after.curr_blocks
    );

    // Per-run allocation budget over both windows (cumulative allocations, not live).
    let per_iter = (after.total_blocks - base.total_blocks) / (2 * ITERS);
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

    // bash spawns a subprocess each run (through the Sandbox seam); assert the
    // parent-side Command/pipe/output allocations don't leak across runs. The tool
    // (and its Sandbox Arc) is built once — only the per-exec path repeats.
    #[cfg(feature = "tool-core")]
    {
        let bash = agent_tools::BashTool::default();
        assert_no_leak(200, || {
            let bash = &bash;
            let ctx = &ctx;
            async move {
                let obs = bash
                    .execute(serde_json::json!({ "command": "printf hi" }), ctx)
                    .await
                    .unwrap();
                assert!(!obs.is_error, "{}", obs.content);
            }
        })
        .await;
    }

    // edit: exercise the read → match/replace → write path (fuzzy fallback).
    #[cfg(feature = "tool-edit")]
    assert_no_leak(100, || async {
        std::fs::write(ctx.cwd.join("e.txt"), "before\nkeep\n").unwrap();
        let obs = agent_tools::EditTool
            .execute(
                serde_json::json!({ "path": "e.txt", "old_string": "before", "new_string": "after", "fuzzy": true }),
                &ctx,
            )
            .await
            .unwrap();
        assert!(!obs.is_error, "{}", obs.content);
    })
    .await;

    // grep walks the tree (spawn_blocking + WalkBuilder + regex) each call; assert
    // the walker/output allocations don't leak across runs.
    #[cfg(feature = "tool-search")]
    {
        std::fs::write(ctx.cwd.join("s.txt"), "needle here\nother line\n").unwrap();
        assert_no_leak(400, || async {
            let obs = agent_tools::GrepTool::default()
                .execute(serde_json::json!({ "pattern": "needle" }), &ctx)
                .await
                .unwrap();
            assert!(!obs.is_error, "{}", obs.content);
        })
        .await;
    }

    // web_fetch: the fetch → MIME-gate → HTML→markdown sanitize → truncate path.
    // The sanitizer builds a token vec + output strings each run; assert those
    // buffers are freed (flat live blocks) across iterations. A local, *non*-
    // recording backend is used so the double's own state can't read as a leak.
    #[cfg(feature = "tool-web")]
    {
        use agent_core::{Result, WebBackend, WebRequest, WebResponse};
        use async_trait::async_trait;
        use std::sync::Arc;

        struct StaticWeb(String);
        #[async_trait]
        impl WebBackend for StaticWeb {
            async fn fetch(&self, req: &WebRequest) -> Result<WebResponse> {
                Ok(WebResponse {
                    final_url: req.url.clone(),
                    status: 200,
                    content_type: "text/html".into(),
                    format: req.format,
                    body: self.0.clone(),
                    bytes: self.0.len() as u64,
                })
            }
        }

        let html = agent_testkit::bench::html_document(60);
        let tool = agent_tools::WebFetchTool::new(Arc::new(StaticWeb(html)), 5 << 20, 30, 120, 5);
        assert_no_leak(6000, || {
            let tool = &tool;
            let ctx = &ctx;
            async move {
                let obs = tool
                    .execute(serde_json::json!({ "url": "https://example.com/doc" }), ctx)
                    .await
                    .unwrap();
                assert!(!obs.is_error, "{}", obs.content);
            }
        })
        .await;
    }
}
