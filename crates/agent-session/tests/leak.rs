//! Heap leak + allocation-budget assertion for the checkpoint path (serialize →
//! content-hash → write object → move head), under dhat. Each checkpoint allocates
//! the serialized object + id; this pins that they free across turns. Compiled only
//! with `--features dhat-heap`; `nix/checks/leak.nix` runs it.
#![cfg(feature = "dhat-heap")]

use agent_core::{Message, Role, SessionStore, WorkingSet};
use agent_session::FileSessionStore;
use agent_testkit::tempdir;

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

fn ws(n: usize) -> WorkingSet {
    WorkingSet {
        messages: (0..n)
            .map(|i| Message {
                role: if i % 2 == 0 {
                    Role::User
                } else {
                    Role::Assistant
                },
                content: format!("turn {i}"),
                tool_calls: vec![],
                tool_call_id: None,
            })
            .collect(),
    }
}

#[tokio::test]
async fn checkpoint_does_not_leak() {
    let _profiler = dhat::Profiler::builder().testing().build();
    let dir = tempdir();
    let store = FileSessionStore::new(dir.clone());

    // Warm up, then measure a flat live-block count across N checkpoints (each on a
    // fresh session so content-addressing writes a distinct object every time).
    let _ = store.checkpoint("warm", &ws(4), "t").await.unwrap();
    let base = dhat::HeapStats::get();

    const ITERS: u64 = 100;
    for i in 0..ITERS {
        let _ = store
            .checkpoint(&format!("s{i}"), &ws(4), "t")
            .await
            .unwrap();
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
