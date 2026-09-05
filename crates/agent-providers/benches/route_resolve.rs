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
/// Backing storage is leaked (bench-only, bounded) for the borrowed-view metas.
fn fleet(n: usize) -> Vec<UpstreamMeta<'static>> {
    (0..n)
        .map(|i| UpstreamMeta {
            id: Box::leak(format!("u{i}").into_boxed_str()),
            tags: Box::leak(
                match i % 3 {
                    0 => vec!["reasoning".to_string(), "long-context".to_string()],
                    1 => vec!["cheap".to_string()],
                    _ => vec!["coding".to_string()],
                }
                .into_boxed_slice(),
            ),
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
            in_flight: 0,
            latency_ewma_ms: 0,
            // Varied capacity (incl. 0 = unknown) so the bench exercises both
            // the capacity-normalised and the pass-through least-loaded paths.
            max_concurrency: (i as u32 % 4) * 8,
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
                policy: None,
            },
        })
        .collect();
    Policy {
        rules,
        default_prefer: Prefer {
            tags: vec!["cheap".into()],
            tier: None,
            upstreams: vec![],
            policy: None,
        },
    }
}

// Resolve a Review hint (needs 16k context) over a 50-upstream fleet + 8 rules — the
// full filter+match+order path (incl. building the 50-member fleet + 8 rules). Observed
// ~132k Ir at 02 landing; ~115k after the borrowed-view/index pass (the decision alone
// dropped 58k → 22.7k — see `resolve_indices_only`). Ceiling kept at the original
// ~2.5×. Bump deliberately when a change legitimately moves it.
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

/// Setup-excluded inputs, so the measurement is the DECISION alone (the two
/// whole-path benches above keep their history; this one answers "where do the
/// instructions actually go" — input construction vs resolve).
fn inputs() -> (Vec<UpstreamMeta<'static>>, Policy, Hint) {
    (
        fleet(50),
        policy_with_modes(),
        Hint {
            role: Role::Review,
            task_mode: Some(TaskMode::Debug),
            min_context: 16_000,
            ..Default::default()
        },
    )
}

#[library_benchmark(config = LibraryBenchmarkConfig::default()
    .tool(Callgrind::default().hard_limits([(EventKind::Ir, 150_000u64)])))]
#[bench::decision_only(setup = inputs)]
fn resolve_only((f, p, hint): (Vec<UpstreamMeta<'static>>, Policy, Hint)) -> Vec<String> {
    black_box(p.resolve(black_box(&hint), black_box(&f)))
}

// The PRODUCTION per-call path (what `TaskRouter::order` actually runs):
// indices out, no id `String` mapping. The gap to `resolve_only` is the cost
// of the owned-id compatibility wrapper.
#[library_benchmark(config = LibraryBenchmarkConfig::default()
    .tool(Callgrind::default().hard_limits([(EventKind::Ir, 90_000u64)])))]
#[bench::decision_indices(setup = inputs)]
fn resolve_indices_only(
    (f, p, hint): (Vec<UpstreamMeta<'static>>, Policy, Hint),
) -> (Vec<usize>, Option<usize>) {
    black_box(p.resolve_indices(black_box(&hint), black_box(&f)))
}

/// The 02b per-call context estimate over a realistic multi-message request
/// (~24KiB of text across 12 messages) — it runs on every routed call that
/// doesn't assert its own floor.
fn estimate_input() -> agent_core::CompletionRequest {
    agent_core::CompletionRequest {
        messages: (0..12)
            .map(|i| agent_core::Message::user("x".repeat(2_048 + i)))
            .collect(),
        ..Default::default()
    }
}

#[library_benchmark(config = LibraryBenchmarkConfig::default()
    .tool(Callgrind::default().hard_limits([(EventKind::Ir, 10_000u64)])))]
#[bench::twelve_messages(setup = estimate_input)]
fn estimate(req: agent_core::CompletionRequest) -> u32 {
    black_box(agent_providers::route::estimate_min_context(black_box(
        &req,
    )))
}

library_benchmark_group!(name = route_resolve;
    benchmarks = resolve_50_upstreams_8_rules, resolve_with_mode_constrained_rules,
    resolve_only, resolve_indices_only, estimate);
main!(library_benchmark_groups = route_resolve);
