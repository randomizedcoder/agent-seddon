//! iai-callgrind bench for the `Route` introspection path: `decide` over a
//! 50-card fleet (the design's scale target) — the registry-served twin of the
//! task-router decision bench. Deterministic instruction counts under valgrind;
//! the absolute Ir ceiling is a `hard_limits` on the bench (run by
//! `nix/checks/bench.nix`).

use agent_core::{
    ModelRouterConfig, PoolTier, RouteHint, RouteMatchSpec, RoutePolicySpec, RoutePreferSpec,
    RouteRole, RouteRuleSpec, TaskMode, Upstream,
};
use agent_registry::decide;
use iai_callgrind::{
    library_benchmark, library_benchmark_group, main, Callgrind, EventKind, LibraryBenchmarkConfig,
};
use std::hint::black_box;

fn fleet(n: usize) -> ModelRouterConfig {
    let upstreams = (0..n)
        .map(|i| Upstream {
            id: format!("u{i:02}"),
            kind: "openai-compat".into(),
            enabled: true,
            base_url: "http://127.0.0.1:1/v1".into(),
            model: "m".into(),
            context_window: 32_768 + (i as u32 % 8) * 32_768,
            supports_tools: i % 3 != 0,
            supports_vision: i % 5 == 0,
            tags: match i % 4 {
                0 => vec!["reasoning".into()],
                1 => vec!["cheap".into()],
                2 => vec!["long-context".into(), "coding".into()],
                _ => vec![],
            },
            input_cost: (i % 7) as f32 * 0.3,
            tier: Some(match i % 3 {
                0 => PoolTier::Light,
                1 => PoolTier::Medium,
                _ => PoolTier::Heavy,
            }),
            ..Default::default()
        })
        .collect();
    let policy = RoutePolicySpec {
        rules: vec![
            RouteRuleSpec {
                match_: RouteMatchSpec {
                    role: Some(RouteRole::Judge),
                    ..Default::default()
                },
                prefer: RoutePreferSpec {
                    tags: vec!["reasoning".into()],
                    tier: Some(PoolTier::Heavy),
                    ..Default::default()
                },
            },
            RouteRuleSpec {
                match_: RouteMatchSpec {
                    task_mode: Some(TaskMode::Debug),
                    min_context: 4_096,
                    ..Default::default()
                },
                prefer: RoutePreferSpec {
                    upstreams: vec!["u07".into(), "u03".into()],
                    ..Default::default()
                },
            },
        ],
        default_prefer: RoutePreferSpec {
            tags: vec!["cheap".into()],
            ..Default::default()
        },
        failure_threshold: 0,
        cooldown_secs: 0,
    };
    ModelRouterConfig { upstreams, policy }
}

fn judge_setup() -> (ModelRouterConfig, RouteHint) {
    (
        fleet(50),
        RouteHint {
            role: Some(RouteRole::Judge),
            task_mode: Some(TaskMode::Debug),
            min_context: 8_192,
            ..Default::default()
        },
    )
}

// ~79k Ir measured (spec-to-engine mapping + the resolve itself over 50 cards).
// The ceiling (~3x) is a major-regression gate, not a micro-noise tripwire.
#[library_benchmark(config = LibraryBenchmarkConfig::default()
    .tool(Callgrind::default().hard_limits([(EventKind::Ir, 250_000u64)])))]
#[bench::fleet50(setup = judge_setup)]
fn route_over_50_cards(input: (ModelRouterConfig, RouteHint)) -> agent_core::RouteDecision {
    let (cfg, hint) = input;
    black_box(decide(&cfg, &hint))
}

library_benchmark_group!(name = registry_route; benchmarks = route_over_50_cards);
main!(library_benchmark_groups = registry_route);
