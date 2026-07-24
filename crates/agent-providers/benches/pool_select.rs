//! Deterministic instruction-count bench for the pool's **selection** hot path
//! (`eligible`, ordered by policy) — it runs on every LLM call, so its cost
//! matters. Least-loaded is the most work (a load read + cost tie-break per
//! member). Input is built purely from constants (a fixed clock, no randomness).
//! Ceiling is an absolute `Ir` `hard_limits`; a regression fails `cargo bench`.
//! Built only with `provider-pool` (`required-features` in Cargo.toml).

use std::hint::black_box;
use std::sync::Arc;

use agent_core::{
    CompletionRequest, CompletionResponse, LlmProvider, Message, ModelCapabilities, PoolTier,
    Result,
};
use agent_providers::{Candidate, PoolPolicy, PoolProvider, PoolSpec};
use iai_callgrind::{
    library_benchmark, library_benchmark_group, main, Callgrind, EventKind, LibraryBenchmarkConfig,
};

struct Trivial;
#[async_trait::async_trait]
impl LlmProvider for Trivial {
    fn capabilities(&self) -> ModelCapabilities {
        ModelCapabilities {
            supports_tools: true,
            context_window: 1000,
            supports_response_format: false,
            supports_vision: false,
        }
    }
    async fn complete(&self, _r: CompletionRequest) -> Result<CompletionResponse> {
        unreachable!("selection bench never dispatches")
    }
}

/// An `n`-member least-loaded pool with a fixed clock and staggered in-flight.
fn pool(n: usize) -> PoolProvider {
    let specs: Vec<PoolSpec> = (0..n)
        .map(|i| PoolSpec {
            candidate: Candidate {
                name: format!("m{i}"),
                provider: Arc::new(Trivial),
            },
            tier: PoolTier::Medium,
            cost: 0.0,
            weight: 1.0,
            max_concurrency: 0,
        })
        .collect();
    let p = PoolProvider::new("bench", specs)
        .expect("pool")
        .with_policy(PoolPolicy::LeastLoaded)
        .with_clock(Arc::new(|| 0));
    for i in 0..n {
        p.bench_set_in_flight(i, (n - i) * 2); // staggered load so the sort does work
    }
    p
}

fn req() -> CompletionRequest {
    CompletionRequest {
        messages: vec![Message::user("hi")],
        tools: vec![],
        max_tokens: 16,
        temperature: 0.0,
        response_format: None,
    }
}

// Select over an 8-member pool under least-loaded. Observed ~15.5k Ir (build 8
// trivial providers + filter + sort); ceiling ~2.5×.
#[library_benchmark(config = LibraryBenchmarkConfig::default()
    .tool(Callgrind::default().hard_limits([(EventKind::Ir, 40_000u64)])))]
fn select_least_loaded_8() -> Vec<usize> {
    let p = pool(black_box(8));
    black_box(p.bench_select(&req(), PoolTier::Light, 8))
}

library_benchmark_group!(name = pool_select; benchmarks = select_least_loaded_8);
main!(library_benchmark_groups = pool_select);
