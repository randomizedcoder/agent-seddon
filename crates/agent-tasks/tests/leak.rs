//! Heap leak + allocation-budget assertion for the TaskTracker mutation path,
//! under dhat. The plan is cloned on every write/update (validate-on-copy) and on
//! `list`, so a regression that retained those clones would grow live blocks; this
//! pins that repeated write→update→clear cycles free everything. Compiled only
//! with `--features dhat-heap`; `nix/checks/leak.nix` runs it.
#![cfg(feature = "dhat-heap")]

use agent_core::{TaskTracker, Todo, TodoPatch, TodoPriority, TodoStatus};
use agent_tasks::MemoryTaskTracker;

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

fn plan() -> Vec<Todo> {
    vec![
        Todo {
            content: "a".into(),
            status: TodoStatus::Pending,
            priority: TodoPriority::High,
        },
        Todo {
            content: "b".into(),
            status: TodoStatus::Pending,
            priority: TodoPriority::Low,
        },
    ]
}

fn to_in_progress(content: &str) -> TodoPatch {
    TodoPatch {
        content: content.into(),
        status: Some(TodoStatus::InProgress),
        priority: None,
    }
}

#[tokio::test]
async fn tasks_do_not_leak() {
    let _profiler = dhat::Profiler::builder().testing().build();
    let t = MemoryTaskTracker::new();

    // Warm up to steady state, then measure a flat live-block count across N runs.
    t.write(plan()).await.unwrap();
    t.update(to_in_progress("a")).await.unwrap();
    t.clear().await.unwrap();
    let base = dhat::HeapStats::get();

    const ITERS: u64 = 200;
    for _ in 0..ITERS {
        t.write(plan()).await.unwrap();
        t.update(to_in_progress("a")).await.unwrap();
        let _ = t.list().await.unwrap();
        t.clear().await.unwrap();
    }
    let after = dhat::HeapStats::get();

    dhat::assert!(
        after.curr_blocks <= base.curr_blocks + 8,
        "live blocks grew across runs (leak?): {} -> {}",
        base.curr_blocks,
        after.curr_blocks
    );
    let per_iter = (after.total_blocks - base.total_blocks) / ITERS;
    dhat::assert!(per_iter < 64, "allocated {per_iter} blocks/run");
}
