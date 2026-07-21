//! Heap leak + allocation-budget assertion for `@`-reference resolution, under
//! dhat. Each `resolve` parses the prompt, reads the referenced files, slices +
//! injection-scans + budget-checks each block; this pins that repeated resolves
//! free everything and stay under a per-iteration budget. Compiled only with
//! `--features dhat-heap`; `nix/checks/leak.nix` runs it.
#![cfg(feature = "dhat-heap")]

use agent_core::ReferenceResolver;
use agent_reference::LocalResolver;
use agent_testkit::tempdir;

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

#[tokio::test]
async fn resolve_does_not_leak() {
    let _profiler = dhat::Profiler::builder().testing().build();
    let dir = tempdir();
    std::fs::write(dir.join("a.rs"), "fn a() {}\nfn b() {}\nfn c() {}").unwrap();
    std::fs::write(dir.join("b.rs"), "struct S;\nimpl S {}").unwrap();
    let resolver = LocalResolver::new(dir.clone());
    let prompt = "look at @file:a.rs:1-2 and @file:b.rs and @dir:.";

    let _ = resolver.resolve(prompt, 0).await; // warm up
    let base = dhat::HeapStats::get();
    const ITERS: u64 = 100;
    for _ in 0..ITERS {
        let _ = resolver.resolve(prompt, 0).await;
    }
    let after = dhat::HeapStats::get();

    dhat::assert!(
        after.curr_blocks <= base.curr_blocks + 8,
        "live blocks grew (leak?): {} -> {}",
        base.curr_blocks,
        after.curr_blocks
    );
    let per_iter = (after.total_blocks - base.total_blocks) / ITERS;
    dhat::assert!(per_iter < 400, "allocated {per_iter} blocks/run");
}
