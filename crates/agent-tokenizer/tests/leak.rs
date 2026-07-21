//! dhat allocation-budget / leak assertion for the tokenizer count path, behind
//! the per-crate `dhat-heap` feature so its `#[global_allocator]` never lands in a
//! normal build. `count_text` is allocation-free by design; this pins that so a
//! future BPE backend (which allocates merge tables / token buffers) can't
//! silently start leaking. Run: `cargo test -p agent-tokenizer --features dhat-heap --test leak`.
#![cfg(feature = "dhat-heap")]

use agent_core::Tokenizer;
use agent_tokenizer::ApproxTokenizer;

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

#[tokio::test]
async fn approx_count_does_not_leak() {
    let _profiler = dhat::Profiler::builder().testing().build();
    let tok = ApproxTokenizer::new();
    let text = "fn compute(really_long_identifier: i32) -> i32 { café + 42 }\n".repeat(40);

    // Warm up (tokio blocking pool / first-touch buffers), then measure a flat
    // live-block count across N iterations — the iteration-based pattern the other
    // crates use so pooled buffers don't read as a leak.
    let _ = tok.count(&text, "m").await.unwrap();
    let base = dhat::HeapStats::get();

    const ITERS: u64 = 200;
    for _ in 0..ITERS {
        let _ = tok.count(&text, "m").await.unwrap();
    }
    let after = dhat::HeapStats::get();

    dhat::assert!(after.curr_blocks <= base.curr_blocks + 8);
    // Generous per-iteration allocation ceiling (count_text itself allocates none).
    let per_iter = (after.total_blocks - base.total_blocks) / ITERS;
    dhat::assert!(per_iter < 16);
}
