//! Declarative task-aware routing — the decision engine (model-router increment 02).
//!
//! A [`Policy`] you author maps a per-request [`Hint`] (which role the request plays
//! and what it requires) to an ordered set of eligible upstreams; the concrete pick is
//! the head of that order. Resolution is a pure, deterministic function over a
//! lightweight [`UpstreamMeta`] view of the fleet plus live health — *tell it how to
//! route (the rules), and it decides*: filter by the request's hard requirements,
//! match the first applicable rule, then order the survivors by that rule's
//! preference. No model call, no randomness.
//!
//! This is the ENGINE only; wiring it onto `CompletionRequest` and a `TaskRouter`
//! `LlmProvider` that builds [`UpstreamMeta`] from live pool members is a following
//! slice. See `docs/design/model-router/02-routing.md`.
//!
//! Named under a `route` module (not re-exported) so it doesn't collide with the
//! failover `RoutePolicy` in [`crate::router`].

use agent_core::{PoolTier, TaskMode};

/// The internal call-site a request originates from — one of the routing signals a
/// rule can match on, so each subsystem is steerable to a fit-for-purpose model
/// (a cheap model for `Classify`, a long-context one for `Summarize`, …).
/// Unified with the core taxonomy in 02b so a `CompletionRequest`'s
/// `RouteHint` carries the same closed set the rules match on.
pub use agent_core::RouteRole as Role;

/// Per-request routing signals: the request's hard requirements, its role and
/// classified task mode, and an optional explicit override.
#[derive(Debug, Clone, Default)]
pub struct Hint {
    pub role: Role,
    /// The classified task mode; `None` = not carried ⇒ mode-constrained rules
    /// don't match (fail to the default, never guess).
    pub task_mode: Option<TaskMode>,
    /// Minimum context window the request needs; `0` = unknown ⇒ not filtered on.
    pub min_context: u32,
    pub needs_vision: bool,
    pub needs_tools: bool,
    /// Per-Mtok input-cost ceiling; `None` = no cap.
    pub max_cost: Option<f32>,
    /// Tier floor; `None` = any tier.
    pub tier: Option<PoolTier>,
    /// An explicit upstream id that wins *when eligible* — it can never select an
    /// upstream the hard filter rejected (a model-supplied override can't escape).
    pub override_upstream: Option<String>,
}

/// A lightweight, already-clamped view of one upstream the policy filters and orders
/// over. The caller (the `TaskRouter`) builds these from live pool members + config,
/// clamping hostile numbers before they reach here. A **borrowed view** since the
/// low-hanging-fruit pass: building the fleet for a decision allocates nothing
/// (the decision runs on every routed LLM call).
#[derive(Debug, Clone, Copy)]
pub struct UpstreamMeta<'a> {
    pub id: &'a str,
    pub tags: &'a [String],
    pub tier: PoolTier,
    /// `0` = unknown window (never filtered out on context).
    pub context_window: u32,
    /// Per-Mtok input cost.
    pub input_cost: f32,
    pub healthy: bool,
    pub supports_vision: bool,
    pub supports_tools: bool,
    /// Live signals (model-router 04): requests currently in flight and the
    /// smoothed latency, fed by the caller from its own dispatch accounting (or
    /// a registry health snapshot — clamped there; a remote's numbers are
    /// untrusted). `0` = unknown, which is also the neutral ordering value.
    pub in_flight: u32,
    pub latency_ewma_ms: u32,
    /// Configured aggregate concurrency this upstream can absorb — for a
    /// multi-GPU gateway (one endpoint that internally load-balances N cards),
    /// ≈ GPUs × per-GPU slots. Used to normalise the `least-loaded` key so a
    /// higher-capacity endpoint draws proportionally more traffic. `0` =
    /// unknown/unbounded, treated as capacity `1` (the neutral value that keeps
    /// the raw-`in_flight` ordering — see [`Prefer::rank`]).
    pub max_concurrency: u32,
}

/// A cheap context floor for a request that carries no `min_context`: total text
/// chars / 4 (the usual ~4-chars-per-token heuristic), allocation-free, capped
/// to [`agent_core::MAX_ROUTE_MIN_CONTEXT`]. A floor *filter* only — an
/// upstream with an unknown (`0`) window is never filtered out, so an
/// over-estimate fails soft.
pub fn estimate_min_context(req: &agent_core::CompletionRequest) -> u32 {
    let chars: usize = req
        .messages
        .iter()
        .flat_map(|m| m.content.iter())
        .filter_map(|b| b.as_text())
        .map(str::len)
        .sum();
    u32::try_from(chars / 4)
        .unwrap_or(agent_core::MAX_ROUTE_MIN_CONTEXT)
        .min(agent_core::MAX_ROUTE_MIN_CONTEXT)
}

/// The `match` half of a rule — every present condition must hold for it to fire.
#[derive(Debug, Clone, Default)]
pub struct Match {
    pub role: Option<Role>,
    /// Fires only for this classified task mode. A hint that carries NO mode
    /// never matches a mode-constrained rule — absent evidence falls through to
    /// the default preference rather than guessing (02b).
    pub task_mode: Option<TaskMode>,
    /// Fires only when the request needs at least this much context.
    pub min_context: u32,
}

impl Match {
    fn matches(&self, hint: &Hint) -> bool {
        self.role.is_none_or(|r| r == hint.role)
            && self.task_mode.is_none_or(|m| hint.task_mode == Some(m))
            && hint.min_context >= self.min_context
    }
}

/// Live-signal ordering (model-router 04): after the explicit
/// position/tag/tier preferences, break remaining ties by a live number —
/// cheapest first, fastest first, or least-loaded first. `None` keeps the
/// stable id ordering. A *tie-break*, not an override: an explicitly preferred
/// upstream still wins regardless of its live numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderPolicy {
    Cost,
    Latency,
    LeastLoaded,
}

impl OrderPolicy {
    /// Parse the config string (`cost | latency | least-loaded`); unknown /
    /// empty ⇒ `None` (callers decide whether that warns or errors).
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.trim() {
            "cost" => OrderPolicy::Cost,
            "latency" => OrderPolicy::Latency,
            "least-loaded" => OrderPolicy::LeastLoaded,
            _ => return None,
        })
    }
}

/// Fixed-point scale for the capacity-normalised `least-loaded` key: it turns
/// the load *ratio* `in_flight / capacity` into an ordinal integer without a
/// float sort key. Overflow-safe — `in_flight ≤ u32::MAX` and the multiply is in
/// `i64`, so the worst case (`u32::MAX × SCALE`) stays well under `i64::MAX`.
const LEAST_LOADED_SCALE: i64 = 1_000_000;

/// The `prefer` half — how to ORDER the survivors once a rule matches.
#[derive(Debug, Clone, Default)]
pub struct Prefer {
    /// Upstreams carrying these tags sort first (more matches ⇒ earlier).
    pub tags: Vec<String>,
    /// Prefer members at/above this tier at the front.
    pub tier: Option<PoolTier>,
    /// Explicit id order — a listed id sorts by its position, ahead of the unlisted.
    pub upstreams: Vec<String>,
    /// Live-signal tie-break among equally-preferred survivors (04).
    pub policy: Option<OrderPolicy>,
}

impl Prefer {
    /// A total, deterministic sort key; **lower is more-preferred**:
    /// (explicit-position, −tag-overlap, −preferred-tier, live-signal, id) — the
    /// trailing id makes ties stable and reproducible (important for the bench +
    /// tests). The live-signal key (04) only separates survivors the explicit
    /// preferences left tied, and is `0` (neutral) with no `policy` — so a
    /// policy-less rank is byte-identical to the 02b ordering.
    fn rank(&self, u: &UpstreamMeta<'_>) -> (usize, i64, i64, i64) {
        let pos = self
            .upstreams
            .iter()
            .position(|id| id == u.id)
            .unwrap_or(self.upstreams.len());
        let tag_overlap = u.tags.iter().filter(|t| self.tags.contains(t)).count() as i64;
        let tier_bonus = match self.tier {
            Some(t) if u.tier >= t => u.tier as i64,
            _ => 0,
        };
        let live = match self.policy {
            // milli-dollar per Mtok: keeps sub-cent differences ordinal without
            // float keys (the cost was clamped finite + non-negative on build).
            Some(OrderPolicy::Cost) => (f64::from(u.input_cost) * 1_000.0) as i64,
            Some(OrderPolicy::Latency) => i64::from(u.latency_ewma_ms),
            // Capacity-normalised load: order by the *ratio* in_flight/capacity,
            // not raw in-flight, so a higher-capacity endpoint (e.g. a gateway
            // fronting 3 GPUs, `max_concurrency = 3×`) looks 1/3 as loaded at the
            // same in-flight and draws ~3× the traffic until the ratios equalise.
            // Capacity `0` (unknown) ⇒ 1, so a fleet that sets no capacity keys
            // on `in_flight × SCALE` — monotonic in in-flight, i.e. byte-identical
            // ordering to the pre-capacity behaviour.
            Some(OrderPolicy::LeastLoaded) => {
                let cap = i64::from(u.max_concurrency).max(1);
                i64::from(u.in_flight) * LEAST_LOADED_SCALE / cap
            }
            None => 0,
        };
        (pos, -tag_overlap, -tier_bonus, live)
    }
}

/// A single routing rule: when `match` holds, order survivors by `prefer`.
#[derive(Debug, Clone, Default)]
pub struct Rule {
    pub match_: Match,
    pub prefer: Prefer,
}

/// The declarative routing policy — an ordered rule list plus a default preference
/// used when no rule matches. This is the "tell it how to route" surface.
#[derive(Debug, Clone, Default)]
pub struct Policy {
    pub rules: Vec<Rule>,
    pub default_prefer: Prefer,
}

impl Policy {
    /// Map the seam-currency policy spec (`agent_core::RoutePolicySpec` — what
    /// the provider registry stores and serves) onto the engine. Typed on both
    /// sides — nothing can fail; the spec's `prefer.policy` string was
    /// validated to the closed set on ingest.
    pub fn from_spec(spec: &agent_core::RoutePolicySpec) -> Self {
        let prefer = |p: &agent_core::RoutePreferSpec| Prefer {
            tags: p.tags.clone(),
            tier: p.tier,
            upstreams: p.upstreams.clone(),
            policy: OrderPolicy::parse(&p.policy),
        };
        Policy {
            rules: spec
                .rules
                .iter()
                .map(|r| Rule {
                    match_: Match {
                        role: r.match_.role,
                        task_mode: r.match_.task_mode,
                        min_context: r.match_.min_context,
                    },
                    prefer: prefer(&r.prefer),
                })
                .collect(),
            default_prefer: prefer(&spec.default_prefer),
        }
    }

    /// Resolve `hint` against the `fleet`, returning eligible upstream ids
    /// most-preferred first. Pure, deterministic, and total: an empty result means
    /// *no upstream can serve this request* (fail-soft — the caller decides), never a
    /// panic, even on a degenerate fleet or hostile requirement.
    pub fn resolve(&self, hint: &Hint, fleet: &[UpstreamMeta<'_>]) -> Vec<String> {
        self.resolve_with_rule(hint, fleet).0
    }

    /// [`Self::resolve`] plus *which* rule ordered the result — see
    /// [`Self::resolve_indices`].
    pub fn resolve_with_rule(
        &self,
        hint: &Hint,
        fleet: &[UpstreamMeta<'_>],
    ) -> (Vec<String>, Option<usize>) {
        let (idx, rule) = self.resolve_indices(hint, fleet);
        (
            idx.into_iter().map(|i| fleet[i].id.to_string()).collect(),
            rule,
        )
    }

    /// The allocation-light core: eligible **fleet indices** most-preferred
    /// first, plus *which* rule ordered them — `Some(index)` of the first
    /// matching rule, `None` for the default preference (or an override win,
    /// where no rule was consulted; the index is a bounded metric label for the
    /// 02b decision observability). Index-based so the per-call hot path (this
    /// runs on every routed LLM call) allocates only the index vectors — no id
    /// `String`s — and the caller (the `TaskRouter`) maps indices straight onto
    /// its upstream slots without a by-id search.
    pub fn resolve_indices(
        &self,
        hint: &Hint,
        fleet: &[UpstreamMeta<'_>],
    ) -> (Vec<usize>, Option<usize>) {
        // 1. Hard filter: only upstreams that CAN serve the request survive.
        let eligible = (0..fleet.len()).filter(|&i| {
            let u = &fleet[i];
            u.healthy
                && (!hint.needs_vision || u.supports_vision)
                && (!hint.needs_tools || u.supports_tools)
                && (hint.min_context == 0
                    || u.context_window == 0
                    || u.context_window >= hint.min_context)
                && hint.tier.is_none_or(|t| u.tier >= t)
                && hint.max_cost.is_none_or(|mc| u.input_cost <= mc)
        });

        // 2. The first matching rule sets the ordering (else the default
        // preference). Computed before the override check only to keep this
        // pure-per-hint; an override win reports `rule = None` (no rule ordered it).
        let rule = self.rules.iter().position(|r| r.match_.matches(hint));
        let prefer = rule.map_or(&self.default_prefer, |i| &self.rules[i].prefer);

        // 3. Rank each survivor once (the non-id key), then order — ties broken
        // by the borrowed id, so the result is byte-identical to the old
        // String-keyed sort without its per-member allocation.
        let mut ranked: Vec<((usize, i64, i64, i64), usize)> =
            eligible.map(|i| (prefer.rank(&fleet[i]), i)).collect();

        // An explicit override wins outright — but only if it passed the filter, so a
        // model-supplied id can never dial an ineligible/unknown upstream.
        if let Some(ov) = &hint.override_upstream {
            if let Some(&(_, i)) = ranked.iter().find(|&&(_, i)| fleet[i].id == ov) {
                return (vec![i], None);
            }
        }

        ranked
            .sort_unstable_by(|a, b| a.0.cmp(&b.0).then_with(|| fleet[a.1].id.cmp(fleet[b.1].id)));
        (ranked.into_iter().map(|(_, i)| i).collect(), rule)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test fixture for the borrowed view: backing storage is leaked (bounded,
    /// test-only) so the metas can be `'static`.
    fn up(
        id: &'static str,
        tags: &[&str],
        tier: PoolTier,
        window: u32,
        cost: f32,
    ) -> UpstreamMeta<'static> {
        UpstreamMeta {
            id,
            tags: Box::leak(
                tags.iter()
                    .map(|s| (*s).to_string())
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            ),
            tier,
            context_window: window,
            input_cost: cost,
            healthy: true,
            supports_vision: false,
            supports_tools: true,
            in_flight: 0,
            latency_ewma_ms: 0,
            max_concurrency: 0,
        }
    }

    fn fleet() -> Vec<UpstreamMeta<'static>> {
        vec![
            up(
                "kimi",
                &["reasoning", "long-context"],
                PoolTier::Heavy,
                131_072,
                3.0,
            ),
            up("glm", &["reasoning"], PoolTier::Heavy, 32_768, 1.0),
            up("mi50", &["cheap"], PoolTier::Light, 8_192, 0.0),
        ]
    }

    fn rule_role(role: Role, prefer: Prefer) -> Rule {
        Rule {
            match_: Match {
                role: Some(role),
                ..Default::default()
            },
            prefer,
        }
    }

    // --- positive -----------------------------------------------------------
    #[test]
    fn positive_role_rule_prefers_tagged_upstream() {
        let policy = Policy {
            rules: vec![rule_role(
                Role::Review,
                Prefer {
                    tags: vec!["reasoning".into()],
                    tier: Some(PoolTier::Heavy),
                    upstreams: vec![],
                    policy: None,
                },
            )],
            default_prefer: Prefer::default(),
        };
        let hint = Hint {
            role: Role::Review,
            ..Default::default()
        };
        let order = policy.resolve(&hint, &fleet());
        // reasoning+heavy upstreams sort ahead of the cheap light one; the two
        // reasoning members tie on tags+tier, so the id breaks it deterministically.
        assert_eq!(order, vec!["glm", "kimi", "mi50"]);
    }

    #[test]
    fn positive_task_mode_rule_fires_for_carried_mode() {
        let policy = Policy {
            rules: vec![Rule {
                match_: Match {
                    task_mode: Some(TaskMode::Review),
                    ..Default::default()
                },
                prefer: Prefer {
                    upstreams: vec!["kimi".into()],
                    ..Default::default()
                },
            }],
            default_prefer: Prefer {
                upstreams: vec!["mi50".into()],
                ..Default::default()
            },
        };
        let hint = Hint {
            task_mode: Some(TaskMode::Review),
            ..Default::default()
        };
        assert_eq!(policy.resolve(&hint, &fleet())[0], "kimi");
    }

    #[test]
    fn negative_mode_rule_never_fires_without_a_carried_mode() {
        let policy = Policy {
            rules: vec![Rule {
                match_: Match {
                    task_mode: Some(TaskMode::Review),
                    ..Default::default()
                },
                prefer: Prefer {
                    upstreams: vec!["kimi".into()],
                    ..Default::default()
                },
            }],
            default_prefer: Prefer {
                upstreams: vec!["mi50".into()],
                ..Default::default()
            },
        };
        // No mode on the hint ⇒ fall to the default, never guess.
        assert_eq!(policy.resolve(&Hint::default(), &fleet())[0], "mi50");
        // A different mode ⇒ same.
        let other = Hint {
            task_mode: Some(TaskMode::Debug),
            ..Default::default()
        };
        assert_eq!(policy.resolve(&other, &fleet())[0], "mi50");
    }

    #[test]
    fn corner_role_and_mode_constraints_must_both_hold() {
        let policy = Policy {
            rules: vec![Rule {
                match_: Match {
                    role: Some(Role::Judge),
                    task_mode: Some(TaskMode::Review),
                    min_context: 0,
                },
                prefer: Prefer {
                    upstreams: vec!["kimi".into()],
                    ..Default::default()
                },
            }],
            default_prefer: Prefer {
                upstreams: vec!["mi50".into()],
                ..Default::default()
            },
        };
        let both = Hint {
            role: Role::Judge,
            task_mode: Some(TaskMode::Review),
            ..Default::default()
        };
        assert_eq!(policy.resolve(&both, &fleet())[0], "kimi");
        let role_only = Hint {
            role: Role::Judge,
            ..Default::default()
        };
        assert_eq!(policy.resolve(&role_only, &fleet())[0], "mi50");
    }

    #[test]
    fn positive_explicit_override_wins_when_eligible() {
        let hint = Hint {
            override_upstream: Some("glm".into()),
            ..Default::default()
        };
        assert_eq!(Policy::default().resolve(&hint, &fleet()), vec!["glm"]);
    }

    #[test]
    fn positive_max_cost_excludes_the_pricey_one() {
        let hint = Hint {
            max_cost: Some(1.5),
            ..Default::default()
        };
        let order = Policy::default().resolve(&hint, &fleet());
        assert!(!order.contains(&"kimi".to_string())); // 3.0 > 1.5
        assert!(order.contains(&"glm".to_string()) && order.contains(&"mi50".to_string()));
    }

    // --- negative -----------------------------------------------------------
    #[test]
    fn negative_no_rule_matches_uses_default_prefer() {
        // A policy whose only rule needs Role::Classify; a Main hint falls to default.
        let policy = Policy {
            rules: vec![rule_role(Role::Classify, Prefer::default())],
            default_prefer: Prefer {
                upstreams: vec!["mi50".into()],
                ..Default::default()
            },
        };
        let order = policy.resolve(&Hint::default(), &fleet());
        assert_eq!(order.first().map(String::as_str), Some("mi50")); // default put mi50 first
    }

    #[test]
    fn negative_ineligible_override_is_ignored_not_forced() {
        // override names a member the filter rejects (too small a window) → ignored,
        // normal ordering applies (never dispatch to an ineligible upstream).
        let hint = Hint {
            min_context: 64_000,
            override_upstream: Some("mi50".into()), // 8k window, filtered out
            ..Default::default()
        };
        let order = Policy::default().resolve(&hint, &fleet());
        assert!(!order.contains(&"mi50".to_string()));
        assert_eq!(order, vec!["kimi"]); // only the 131k window survives 64k requirement
    }

    // --- corner -------------------------------------------------------------
    #[test]
    fn corner_first_matching_rule_wins() {
        let policy = Policy {
            rules: vec![
                rule_role(
                    Role::Review,
                    Prefer {
                        upstreams: vec!["kimi".into()],
                        ..Default::default()
                    },
                ),
                rule_role(
                    Role::Review,
                    Prefer {
                        upstreams: vec!["glm".into()],
                        ..Default::default()
                    },
                ),
            ],
            default_prefer: Prefer::default(),
        };
        let hint = Hint {
            role: Role::Review,
            ..Default::default()
        };
        // first rule (prefer kimi) wins over the second (prefer glm).
        assert_eq!(
            policy.resolve(&hint, &fleet()).first().map(String::as_str),
            Some("kimi")
        );
    }

    // --- boundary -----------------------------------------------------------
    #[test]
    fn boundary_min_context_exactly_equals_window_passes() {
        let hint = Hint {
            min_context: 32_768, // exactly glm's window
            ..Default::default()
        };
        let order = Policy::default().resolve(&hint, &fleet());
        assert!(order.contains(&"glm".to_string()) && order.contains(&"kimi".to_string()));
        assert!(!order.contains(&"mi50".to_string())); // 8k < 32768
    }

    #[test]
    fn boundary_unknown_window_is_not_filtered() {
        let mut f = fleet();
        f[2].context_window = 0; // mi50 window unknown
        let hint = Hint {
            min_context: 64_000,
            ..Default::default()
        };
        // window=0 opts out of the context filter, so mi50 survives despite the need.
        assert!(Policy::default()
            .resolve(&hint, &f)
            .contains(&"mi50".to_string()));
    }

    // --- adversarial --------------------------------------------------------
    #[test]
    fn adversarial_impossible_requirement_yields_no_candidate_not_panic() {
        let hint = Hint {
            min_context: u32::MAX, // nothing can satisfy it
            ..Default::default()
        };
        assert!(Policy::default().resolve(&hint, &fleet()).is_empty());
    }

    #[test]
    fn adversarial_needs_vision_with_no_vision_model_is_empty() {
        let hint = Hint {
            needs_vision: true,
            ..Default::default()
        };
        assert!(Policy::default().resolve(&hint, &fleet()).is_empty());
    }

    #[test]
    fn adversarial_empty_and_all_unhealthy_fleet_are_fail_soft() {
        assert!(Policy::default().resolve(&Hint::default(), &[]).is_empty());
        let mut dead = fleet();
        dead.iter_mut().for_each(|u| u.healthy = false);
        assert!(Policy::default()
            .resolve(&Hint::default(), &dead)
            .is_empty());
    }

    // --- Role::parse --------------------------------------------------------
    #[test]
    fn positive_role_parse_roundtrips_known_names() {
        assert_eq!(Role::parse("review"), Some(Role::Review));
        assert_eq!(Role::parse("  CLASSIFY "), Some(Role::Classify));
        assert_eq!(Role::parse("main"), Some(Role::Main));
    }

    #[test]
    fn negative_role_parse_unknown_or_empty_is_none() {
        assert_eq!(Role::parse(""), None);
        assert_eq!(Role::parse("bogus"), None);
    }

    // --- resolve_indices: the rule report + index mapping --------------------

    fn two_rule_policy() -> Policy {
        Policy {
            rules: vec![
                rule_role(
                    Role::Judge,
                    Prefer {
                        upstreams: vec!["glm".into()],
                        ..Default::default()
                    },
                ),
                rule_role(
                    Role::Review,
                    Prefer {
                        upstreams: vec!["kimi".into()],
                        ..Default::default()
                    },
                ),
            ],
            default_prefer: Prefer::default(),
        }
    }

    #[test]
    fn positive_rule_index_reports_the_matched_rule_not_the_first() {
        let hint = Hint {
            role: Role::Review,
            ..Default::default()
        };
        let (order, rule) = two_rule_policy().resolve_indices(&hint, &fleet());
        assert_eq!(rule, Some(1), "the SECOND rule matched");
        assert_eq!(order[0], 0, "kimi is fleet index 0");
    }

    #[test]
    fn negative_no_match_reports_no_rule() {
        let hint = Hint {
            role: Role::Classify,
            ..Default::default()
        };
        let (_, rule) = two_rule_policy().resolve_indices(&hint, &fleet());
        assert_eq!(rule, None, "default preference ⇒ no rule index");
    }

    #[test]
    fn corner_override_win_reports_no_rule() {
        // The override bypasses rule ordering entirely, so the decision label
        // must not credit a rule that never ordered anything.
        let hint = Hint {
            role: Role::Judge, // rule 0 WOULD match…
            override_upstream: Some("mi50".into()),
            ..Default::default()
        };
        let (order, rule) = two_rule_policy().resolve_indices(&hint, &fleet());
        assert_eq!(order, vec![2], "mi50 is fleet index 2");
        assert_eq!(rule, None, "…but the override won, no rule ordered it");
    }

    #[test]
    fn boundary_indices_and_ids_agree() {
        // The owned-id wrapper and the index core must tell the same story
        // (the TaskRouter dispatches by index; tests/logs read ids).
        let hint = Hint {
            role: Role::Review,
            ..Default::default()
        };
        let f = fleet();
        let p = two_rule_policy();
        let (ids, r1) = p.resolve_with_rule(&hint, &f);
        let (idx, r2) = p.resolve_indices(&hint, &f);
        assert_eq!(r1, r2);
        assert_eq!(
            ids,
            idx.iter().map(|&i| f[i].id.to_string()).collect::<Vec<_>>()
        );
    }

    // --- filter boundaries ----------------------------------------------------

    #[test]
    fn boundary_max_cost_exactly_at_input_cost_passes() {
        // kimi costs 3.0; a ceiling of exactly 3.0 keeps it (≤, not <).
        let hint = Hint {
            max_cost: Some(3.0),
            ..Default::default()
        };
        let order = Policy::default().resolve(&hint, &fleet());
        assert!(order.contains(&"kimi".to_string()), "{order:?}");
    }

    #[test]
    fn boundary_tier_floor_exactly_at_member_tier_passes() {
        // mi50 is Light; a Light floor keeps it, a Medium floor drops it.
        let at = Hint {
            tier: Some(PoolTier::Light),
            ..Default::default()
        };
        assert!(Policy::default()
            .resolve(&at, &fleet())
            .contains(&"mi50".to_string()));
        let above = Hint {
            tier: Some(PoolTier::Medium),
            ..Default::default()
        };
        let order = Policy::default().resolve(&above, &fleet());
        assert!(!order.contains(&"mi50".to_string()), "{order:?}");
    }

    #[test]
    fn corner_tier_floor_and_cost_cap_compose() {
        // Heavy floor + cost ≤ 1.0 leaves exactly glm (kimi too pricey, mi50 too light).
        let hint = Hint {
            tier: Some(PoolTier::Heavy),
            max_cost: Some(1.0),
            ..Default::default()
        };
        assert_eq!(Policy::default().resolve(&hint, &fleet()), vec!["glm"]);
    }

    // --- estimate_min_context -------------------------------------------------

    #[rstest::rstest]
    #[case::corner_empty_request(vec![], 0)]
    #[case::positive_four_chars_per_token(vec!["x".repeat(4_000)], 1_000)]
    #[case::boundary_three_chars_rounds_down(vec!["abc".to_string()], 0)]
    #[case::positive_sums_across_messages(vec!["x".repeat(2_000), "y".repeat(2_000)], 1_000)]
    fn estimate_min_context_cases(#[case] texts: Vec<String>, #[case] expect: u32) {
        let req = agent_core::CompletionRequest {
            messages: texts.into_iter().map(agent_core::Message::user).collect(),
            ..Default::default()
        };
        assert_eq!(estimate_min_context(&req), expect);
    }

    #[test]
    fn corner_estimate_ignores_media_blocks() {
        // A media-only message contributes nothing (media has no text length);
        // the estimate must not filter windowed upstreams on its behalf.
        let msg = agent_core::Message::with_blocks(
            agent_core::Role::User,
            vec![agent_core::ContentBlock::Image {
                media_type: "image/png".into(),
                data: vec![0u8; 100_000],
            }],
        );
        let req = agent_core::CompletionRequest {
            messages: vec![msg],
            ..Default::default()
        };
        assert_eq!(estimate_min_context(&req), 0);
    }

    #[test]
    fn adversarial_estimate_clamps_at_the_cap() {
        // A pathologically huge prompt cannot report a floor beyond the cap
        // (which would look hostile in logs/metrics); selection stays fail-soft.
        let req = agent_core::CompletionRequest {
            messages: vec![agent_core::Message::user(
                "x".repeat((agent_core::MAX_ROUTE_MIN_CONTEXT as usize) * 4 + 64),
            )],
            ..Default::default()
        };
        assert_eq!(
            estimate_min_context(&req),
            agent_core::MAX_ROUTE_MIN_CONTEXT
        );
    }

    // --- live-signal ordering (model-router 04) -----------------------------

    /// Fixture with live numbers: equal tags/tier so ONLY the policy separates.
    fn live(id: &'static str, cost: f32, in_flight: u32, ewma: u32) -> UpstreamMeta<'static> {
        UpstreamMeta {
            in_flight,
            latency_ewma_ms: ewma,
            input_cost: cost,
            ..up(id, &[], PoolTier::Medium, 100_000, cost)
        }
    }

    fn policy_only(policy: Option<OrderPolicy>) -> Policy {
        Policy {
            rules: vec![],
            default_prefer: Prefer {
                policy,
                ..Default::default()
            },
        }
    }

    #[rstest::rstest]
    #[case::positive_cost_cheapest_first(
        Some(OrderPolicy::Cost), vec!["cheap", "mid", "dear"])]
    #[case::positive_latency_fastest_first(
        Some(OrderPolicy::Latency), vec!["dear", "cheap", "mid"])]
    #[case::positive_least_loaded_first(
        Some(OrderPolicy::LeastLoaded), vec!["mid", "dear", "cheap"])]
    #[case::corner_no_policy_keeps_stable_id_order(
        None, vec!["cheap", "dear", "mid"])]
    fn live_policy_orders_equally_preferred_survivors(
        #[case] policy: Option<OrderPolicy>,
        #[case] want: Vec<&str>,
    ) {
        // cheap: cost 0.1, 9 in flight, 80ms · mid: cost 0.5, 1 in flight,
        // 200ms · dear: cost 2.0, 4 in flight, 20ms — each policy picks a
        // different winner, and no-policy keeps the id tie-break.
        let fleet = vec![
            live("cheap", 0.1, 9, 80),
            live("mid", 0.5, 1, 200),
            live("dear", 2.0, 4, 20),
        ];
        let got = policy_only(policy).resolve(&Hint::default(), &fleet);
        assert_eq!(got, want.into_iter().map(String::from).collect::<Vec<_>>());
    }

    #[test]
    fn positive_explicit_preference_beats_live_signal() {
        // The policy is a TIE-break: an explicitly listed id still wins even
        // with the worst live numbers.
        let fleet = vec![live("slow", 5.0, 50, 5_000), live("fast", 0.0, 0, 1)];
        let p = Policy {
            rules: vec![],
            default_prefer: Prefer {
                upstreams: vec!["slow".into()],
                policy: Some(OrderPolicy::Latency),
                ..Default::default()
            },
        };
        assert_eq!(p.resolve(&Hint::default(), &fleet)[0], "slow");
    }

    #[test]
    fn boundary_all_zero_live_values_are_neutral() {
        // Unknown (0) live numbers order exactly like no policy at all.
        let fleet = vec![live("b", 0.0, 0, 0), live("a", 0.0, 0, 0)];
        for pol in [
            Some(OrderPolicy::Cost),
            Some(OrderPolicy::Latency),
            Some(OrderPolicy::LeastLoaded),
            None,
        ] {
            assert_eq!(
                policy_only(pol).resolve(&Hint::default(), &fleet),
                vec!["a".to_string(), "b".to_string()],
                "{pol:?}"
            );
        }
    }

    #[test]
    fn adversarial_extreme_live_numbers_never_panic() {
        // A hostile/miscounted u32::MAX just sorts last — no overflow, no panic.
        let fleet = vec![
            live("evil", f32::MAX, u32::MAX, u32::MAX),
            live("ok", 0.1, 1, 10),
        ];
        for pol in [
            OrderPolicy::Cost,
            OrderPolicy::Latency,
            OrderPolicy::LeastLoaded,
        ] {
            let got = policy_only(Some(pol)).resolve(&Hint::default(), &fleet);
            assert_eq!(got[0], "ok", "{pol:?}");
        }
    }

    // --- capacity-normalised least-loaded (multi-GPU gateway) ---------------

    /// Equal tags/tier/cost so ONLY the capacity-normalised load separates —
    /// the case for a gateway that fronts several GPUs behind one endpoint.
    fn loaded(id: &'static str, in_flight: u32, capacity: u32) -> UpstreamMeta<'static> {
        UpstreamMeta {
            max_concurrency: capacity,
            ..live(id, 0.0, in_flight, 0)
        }
    }

    #[test]
    fn positive_capacity_normalises_least_loaded_toward_the_bigger_endpoint() {
        // Equal in-flight, unequal capacity: the 3×-capacity gateway looks 1/3 as
        // loaded (3/24 vs 3/8) and is preferred — the whole point of the feature.
        let fleet = vec![loaded("small", 3, 8), loaded("big", 3, 24)];
        let got = policy_only(Some(OrderPolicy::LeastLoaded)).resolve(&Hint::default(), &fleet);
        assert_eq!(got, vec!["big".to_string(), "small".to_string()]);
    }

    #[test]
    fn positive_bigger_endpoint_stays_preferred_while_carrying_proportionally_more() {
        // "big" (cap 24) keeps winning while it carries ~3× "small"'s (cap 8)
        // load: 8/24 = 0.33 < 3/8 = 0.375, so big is still first at 8-vs-3 in
        // flight — traffic tracks the ~3:1 the capacities imply.
        let fleet = vec![loaded("small", 3, 8), loaded("big", 8, 24)];
        let got = policy_only(Some(OrderPolicy::LeastLoaded)).resolve(&Hint::default(), &fleet);
        assert_eq!(got[0], "big");
    }

    #[test]
    fn boundary_zero_capacity_orders_exactly_like_raw_in_flight() {
        // Unknown capacity (0) ⇒ cap 1 ⇒ key is in_flight×SCALE: the pre-capacity
        // behaviour (fewest in-flight first), so an un-annotated fleet is unchanged.
        let fleet = vec![loaded("busy", 9, 0), loaded("idle", 1, 0)];
        let got = policy_only(Some(OrderPolicy::LeastLoaded)).resolve(&Hint::default(), &fleet);
        assert_eq!(got, vec!["idle".to_string(), "busy".to_string()]);
    }

    #[test]
    fn corner_idle_endpoints_tie_on_id_regardless_of_capacity() {
        // Capacity only matters under load: two idle endpoints (0 in flight) both
        // rank at ratio 0 and fall back to the stable id order — a 3× box gets no
        // head-start while idle.
        let fleet = vec![loaded("b", 0, 24), loaded("a", 0, 8)];
        let got = policy_only(Some(OrderPolicy::LeastLoaded)).resolve(&Hint::default(), &fleet);
        assert_eq!(got, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn adversarial_hostile_capacity_never_overflows_or_panics() {
        // u32::MAX capacity AND in-flight: the i64 multiply (in_flight × SCALE)
        // stays < i64::MAX and the divide is well-defined; a sane low-ratio "ok"
        // still wins (evil ≈ 1e6, ok = 10_000).
        let fleet = vec![loaded("evil", u32::MAX, u32::MAX), loaded("ok", 1, 100)];
        let got = policy_only(Some(OrderPolicy::LeastLoaded)).resolve(&Hint::default(), &fleet);
        assert_eq!(got[0], "ok");
    }

    #[rstest::rstest]
    #[case::positive_cost("cost", Some(OrderPolicy::Cost))]
    #[case::positive_latency("latency", Some(OrderPolicy::Latency))]
    #[case::positive_least_loaded("least-loaded", Some(OrderPolicy::LeastLoaded))]
    #[case::positive_padded("  cost  ", Some(OrderPolicy::Cost))]
    #[case::negative_unknown("weighted", None)]
    #[case::corner_empty("", None)]
    fn order_policy_parse(#[case] s: &str, #[case] want: Option<OrderPolicy>) {
        assert_eq!(OrderPolicy::parse(s), want);
    }
}
