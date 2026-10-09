//! Heap leak + allocation-budget assertion for the fork engine under dhat:
//! the full spawn → race → **cancel** → merge → report cycle must free
//! everything it allocates — an aborted laggard branch must not strand its
//! task, request clone, or queue slot. Compiled only with
//! `--features dhat-heap,provider-branching`; `nix/checks/leak.nix` runs it.
#![cfg(all(feature = "dhat-heap", feature = "provider-branching"))]

use std::sync::Arc;
use std::time::Duration;

use agent_core::{
    CompletionRequest, CompletionResponse, LlmProvider, Message, ModelCapabilities, Result,
};
use agent_providers::{BranchCfg, BranchSpec, BranchingProvider, JoinPolicy};

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

struct Fast;
#[async_trait::async_trait]
impl LlmProvider for Fast {
    fn capabilities(&self) -> ModelCapabilities {
        ModelCapabilities::default()
    }
    async fn complete(&self, _r: CompletionRequest) -> Result<CompletionResponse> {
        Ok(CompletionResponse {
            message: Message::assistant("quick"),
            finish_reason: "stop".into(),
            usage: None,
        })
    }
}

/// Never finishes inside the test — every cycle aborts it mid-sleep.
struct Laggard;
#[async_trait::async_trait]
impl LlmProvider for Laggard {
    fn capabilities(&self) -> ModelCapabilities {
        ModelCapabilities::default()
    }
    async fn complete(&self, _r: CompletionRequest) -> Result<CompletionResponse> {
        tokio::time::sleep(Duration::from_secs(30)).await;
        Ok(CompletionResponse {
            message: Message::assistant("late"),
            finish_reason: "stop".into(),
            usage: None,
        })
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn fork_cancel_cycle_does_not_leak() {
    let _profiler = dhat::Profiler::builder().testing().build();
    let provider = BranchingProvider::new(
        "s",
        Arc::new(Fast),
        vec![
            BranchSpec {
                label: "fast".into(),
                lens: "a".into(),
                provider: Arc::new(Fast),
            },
            BranchSpec {
                label: "slow".into(),
                lens: "b".into(),
                provider: Arc::new(Laggard),
            },
        ],
    )
    .with_cfg(BranchCfg {
        policy: JoinPolicy::Any,
        ..BranchCfg::default()
    });
    let req = CompletionRequest {
        messages: vec![Message::user("task")],
        tools: Vec::new(),
        max_tokens: 64,
        temperature: 0.0,
        response_format: None,
        route: None,
    };

    // Warm-up: drive the runtime to STEADY STATE before baselining — runtime
    // working set counts as baseline, not growth. A dhat dump of the live-at-end
    // heap shows it is ~15 KB of one-time, process-global runtime structures (the
    // `parking_lot` park-bucket table + per-worker `stack_overflow::ThreadInfo`),
    // grown lazily as the worker threads first park on the aborted laggard. That
    // ramp is SCHEDULING-driven, not cycle-count driven: under the loaded gate
    // (every leak test runs back-to-back against heavy concurrent builds) a fixed
    // warm-up count can return before the threads have finished parking, leaving a
    // low baseline so the remaining one-time ramp is later misread as a per-cycle
    // leak (the recurring gate flake). So warm until the live heap stops growing —
    // convergence, not a magic number. The cap is a safety net, not the target: a
    // *real* unbounded strand never gives consecutive stable samples, trips the
    // cap with a still-climbing baseline, and then keeps growing through the
    // measured window below — so this cannot mask a leak, it only waits out the
    // runtime's one-time working set.
    const WARMUP_CAP: u64 = 400;
    const STABLE_EPSILON: usize = 256;
    const STABLE_RUN: u32 = 8;
    let mut prev = 0usize;
    let mut stable = 0u32;
    for _ in 0..WARMUP_CAP {
        let r = provider.complete(req.clone()).await.expect("winner");
        assert_eq!(r.message.content_text(), "quick");
        let cur = dhat::HeapStats::get().curr_bytes;
        if cur <= prev + STABLE_EPSILON {
            stable += 1;
            if stable >= STABLE_RUN {
                break;
            }
        } else {
            stable = 0;
        }
        prev = cur;
    }
    // Let any last aborted-laggard teardown settle before baselining.
    tokio::task::yield_now().await;

    let base = dhat::HeapStats::get();
    const ITERS: u64 = 30;
    for _ in 0..ITERS {
        let r = provider.complete(req.clone()).await.expect("winner");
        assert_eq!(r.message.content_text(), "quick");
    }
    // Every cycle spawned two tasks and aborted one mid-sleep: all of it must
    // be freed again (an abort that strands the branch would show up here).
    // The CONTRACT is eventually-freed — an aborted task's teardown runs at
    // the scheduler's leisure, so a single yield can sample mid-release under
    // load (live-observed flake: 10.7k -> 19.4k that a re-run freed). Poll for
    // the settle; a REAL strand never converges and still fails. The budget is
    // generous (~5s) because under the sequential gate leak derivation every leak
    // test runs back-to-back under load, and 1s occasionally sampled before the
    // aborted laggard's teardown completed (round7-2: same recurring gate flake).
    // A real strand won't free in 5s any more than in 50s, so this doesn't weaken
    // the assertion — it only stops sampling mid-release.
    let mut end = dhat::HeapStats::get();
    for _ in 0..500 {
        if end.curr_bytes <= base.curr_bytes + 4 * 1024 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        end = dhat::HeapStats::get();
    }
    assert!(
        end.curr_bytes <= base.curr_bytes + 4 * 1024,
        "live heap grew across {ITERS} fork/cancel cycles: {} -> {} bytes",
        base.curr_bytes,
        end.curr_bytes
    );
    let per_cycle = (end.total_bytes - base.total_bytes) / ITERS;
    assert!(
        per_cycle < 128 * 1024,
        "fork cycle allocates {per_cycle} bytes — budget blown"
    );
}
