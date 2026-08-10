//! Deterministic instruction-count bench for the fork engine's **dispatch**
//! path — everything `BranchingProvider::complete` does around the LLM calls:
//! per-branch request cloning + lens tailing, task spawn, policy join,
//! fate bookkeeping, and a no-LLM `concat` merge (so the measurement is the
//! engine, not a model double). Three instant branches under `all`, driven by
//! a current-thread runtime. Ceiling is an absolute `Ir` `hard_limits`; a
//! regression fails `cargo bench`.

use std::hint::black_box;
use std::sync::Arc;

use agent_core::{
    CompletionRequest, CompletionResponse, LlmProvider, Message, ModelCapabilities, Result,
};
use agent_providers::{BranchCfg, BranchSpec, BranchingProvider, MergeStrategy};
use async_trait::async_trait;
use iai_callgrind::{
    library_benchmark, library_benchmark_group, main, Callgrind, EventKind, LibraryBenchmarkConfig,
};

struct Fixed(&'static str);
#[async_trait]
impl LlmProvider for Fixed {
    fn capabilities(&self) -> ModelCapabilities {
        ModelCapabilities::default()
    }
    async fn complete(&self, _r: CompletionRequest) -> Result<CompletionResponse> {
        Ok(CompletionResponse {
            message: Message::assistant(self.0),
            finish_reason: "stop".into(),
            usage: None,
        })
    }
}

fn setup() -> (BranchingProvider, CompletionRequest) {
    let provider = BranchingProvider::new(
        "split_impl",
        Arc::new(Fixed("base")),
        vec![
            BranchSpec {
                label: "safe".into(),
                lens: "correctness and strict safety".into(),
                provider: Arc::new(Fixed("safe answer")),
            },
            BranchSpec {
                label: "perf".into(),
                lens: "performance optimization".into(),
                provider: Arc::new(Fixed("fast answer")),
            },
            BranchSpec {
                label: "read".into(),
                lens: "readability".into(),
                provider: Arc::new(Fixed("clear answer")),
            },
        ],
    )
    .with_cfg(BranchCfg {
        strategy: MergeStrategy::Concat,
        ..BranchCfg::default()
    });
    let req = CompletionRequest {
        messages: vec![Message::user("implement a ring buffer")],
        tools: Vec::new(),
        max_tokens: 256,
        temperature: 0.0,
        response_format: None,
        route: None,
    };
    (provider, req)
}

// Observed 48,496 Ir (2026-08-09 optimization pass: runtime setup + three
// spawns + clones + concat + fate bookkeeping — the guarded assert proves the
// full three-section merge ran. Runs once per forked completion; real forks
// are dominated by N LLM calls — no fruit worth taking). Ceiling ~2.5×.
#[library_benchmark(config = LibraryBenchmarkConfig::default()
    .tool(Callgrind::default().hard_limits([(EventKind::Ir, 125_000u64)])))]
#[bench::three_branch_concat(setup = setup)]
fn fork_dispatch((provider, req): (BranchingProvider, CompletionRequest)) -> usize {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .expect("rt");
    let resp = rt.block_on(provider.complete(req)).expect("merged");
    let text = resp.message.content_text();
    // Path guard (the standing bench lesson): all three branches must actually
    // be in the concat — a fallback path would silently measure less.
    assert!(
        text.contains("## safe") && text.contains("## perf") && text.contains("## read"),
        "{text}"
    );
    black_box(text.len())
}

library_benchmark_group!(name = branch_dispatch; benchmarks = fork_dispatch);
main!(library_benchmark_groups = branch_dispatch);
