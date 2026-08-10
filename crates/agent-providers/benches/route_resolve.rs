//! Deterministic instruction-count bench for the task-router's **decision** hot path
//! (`route::Policy::resolve`) — it runs on every routed LLM call over the whole fleet
//! (filter by requirements → match a rule → order the survivors), so its cost matters
//! at 10–50 upstreams. Input is built purely from constants (no clock, no randomness),
//! so the count is reproducible. Ceiling is an absolute `Ir` `hard_limits`; a
//! regression fails `cargo bench`. See docs/design/model-router/02-routing.md.

use std::hint::black_box;

use agent_core::{PoolTier, TaskMode};
use agent_providers::route::{Hint, Match, Policy, Prefer, Role, Rule, UpstreamMeta};
use iai_callgrind::{
    library_benchmark, library_benchmark_group, main, Callgrind, EventKind, LibraryBenchmarkConfig,
};

/// An `n`-upstream fleet with varied tags / tiers / windows / costs / health — the
/// mix the filter and the tag/tier/cost ordering actually have to chew through.
fn fleet(n: usize) -> Vec<UpstreamMeta> {
    (0..n)
        .map(|i| UpstreamMeta {
            id: format!("u{i}"),
            tags: match i % 3 {
                0 => vec!["reasoning".into(), "long-context".into()],
                1 => vec!["cheap".into()],
                _ => vec!["coding".into()],
            },
            tier: match i % 3 {
                0 => PoolTier::Heavy,
                1 => PoolTier::Light,
                _ => PoolTier::Medium,
            },
            context_window: 8_192 + (i as u32) * 2_048,
            input_cost: (i % 5) as f32,
            healthy: i % 7 != 0, // a few unhealthy, filtered out
            supports_vision: i % 4 == 0,
            supports_tools: true,
        })
        .collect()
}

/// An 8-rule policy across the roles, each with a tag/tier preference.
fn policy() -> Policy {
    let roles = [
        Role::Review,
        Role::Classify,
        Role::Summarize,
        Role::Verify,
        Role::Judge,
        Role::Main,
        Role::Review,
        Role::Summarize,
    ];
    let rules = roles
        .iter()
        .enumerate()
        .map(|(i, role)| Rule {
            match_: Match {
                role: Some(*role),
                task_mode: None,
                min_context: (i as u32) * 4_096,
            },
            prefer: Prefer {
                tags: vec!["reasoning".into()],
                tier: Some(PoolTier::Heavy),
                upstreams: vec![],
            },
        })
        .collect();
    Policy {
        rules,
        default_prefer: Prefer {
            tags: vec!["cheap".into()],
            tier: None,
            upstreams: vec![],
        },
    }
}

// Resolve a Review hint (needs 16k context) over a 50-upstream fleet + 8 rules — the
// full filter+match+order path (incl. building the 50-member fleet + 8 rules). Observed
// ~132k Ir; ceiling ~2.5×. Bump deliberately when a change legitimately moves it.
#[library_benchmark(config = LibraryBenchmarkConfig::default()
    .tool(Callgrind::default().hard_limits([(EventKind::Ir, 330_000u64)])))]
fn resolve_50_upstreams_8_rules() -> Vec<String> {
    let f = fleet(black_box(50));
    let p = policy();
    let hint = Hint {
        role: Role::Review,
        min_context: 16_000,
        ..Default::default()
    };
    black_box(p.resolve(black_box(&hint), black_box(&f)))
}

/// The 02b shape: half the rules also constrain the classified task mode, and the
/// hint carries one — the added per-rule comparison is the cost under test. The
/// mode-mismatched Review rules are skipped, so the walk reaches deeper into the
/// rule list than the role-only bench.
fn policy_with_modes() -> Policy {
    let mut p = policy();
    for (i, r) in p.rules.iter_mut().enumerate() {
        if i % 2 == 0 {
            r.match_.task_mode = Some(if i % 4 == 0 {
                TaskMode::Review
            } else {
                TaskMode::Debug
            });
        }
    }
    p
}

#[library_benchmark(config = LibraryBenchmarkConfig::default()
    .tool(Callgrind::default().hard_limits([(EventKind::Ir, 330_000u64)])))]
fn resolve_with_mode_constrained_rules() -> Vec<String> {
    let f = fleet(black_box(50));
    let p = policy_with_modes();
    let hint = Hint {
        role: Role::Review,
        task_mode: Some(TaskMode::Debug),
        min_context: 16_000,
        ..Default::default()
    };
    black_box(p.resolve(black_box(&hint), black_box(&f)))
}

library_benchmark_group!(name = route_resolve;
    benchmarks = resolve_50_upstreams_8_rules, resolve_with_mode_constrained_rules);
main!(library_benchmark_groups = route_resolve);
