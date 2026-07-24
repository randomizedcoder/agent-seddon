//! Heap leak + allocation-budget assertion for the mode classifier's free path
//! (the deterministic prefilter), under dhat. It runs every turn, so it must free
//! everything it allocates and stay under a per-iteration budget. Compiled only
//! with `--features dhat-heap`; `nix/checks/leak.nix` runs it.
#![cfg(feature = "dhat-heap")]

use agent_core::{ClassifyCtx, TaskClassifier};
use agent_mode::HybridClassifier;

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

#[tokio::test]
async fn classify_prefilter_does_not_leak() {
    let _profiler = dhat::Profiler::builder().testing().build();
    // No pool ⇒ only the deterministic prefilter runs (the every-turn hot path).
    let c = HybridClassifier::new(None);
    let prompt = "could you review this diff and tell me what you think about the shape";
    let ctx = ClassifyCtx {
        prompt,
        history: &[],
    };

    let _ = c.classify(&ctx).await; // warm up
    let base = dhat::HeapStats::get();
    const ITERS: u64 = 100;
    for _ in 0..ITERS {
        let _ = c.classify(&ctx).await;
    }
    let after = dhat::HeapStats::get();

    dhat::assert!(
        after.curr_blocks <= base.curr_blocks + 8,
        "live blocks grew (leak?): {} -> {}",
        base.curr_blocks,
        after.curr_blocks
    );
    let per_iter = (after.total_blocks - base.total_blocks) / ITERS;
    dhat::assert!(per_iter < 200, "allocated {per_iter} blocks/run");
}
