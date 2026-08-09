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

use agent_core::PoolTier;

/// The internal call-site a request originates from — one of the routing signals a
/// rule can match on, so each subsystem is steerable to a fit-for-purpose model
/// (a cheap model for `Classify`, a long-context one for `Summarize`, …).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Role {
    #[default]
    Main,
    Judge,
    Classify,
    Summarize,
    Verify,
    Review,
}

impl Role {
    /// Parse a config role name; unknown / empty ⇒ `None` (a rule with no role
    /// constraint matches any role rather than silently defaulting to `Main`).
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "main" => Some(Role::Main),
            "judge" => Some(Role::Judge),
            "classify" => Some(Role::Classify),
            "summarize" => Some(Role::Summarize),
            "verify" => Some(Role::Verify),
            "review" => Some(Role::Review),
            _ => None,
        }
    }
}

/// Per-request routing signals: the request's hard requirements, its role, and an
/// optional explicit override. (The classified `TaskMode` joins this once it is
/// threaded onto the request in a later slice.)
#[derive(Debug, Clone, Default)]
pub struct Hint {
    pub role: Role,
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
/// clamping hostile numbers before they reach here.
#[derive(Debug, Clone)]
pub struct UpstreamMeta {
    pub id: String,
    pub tags: Vec<String>,
    pub tier: PoolTier,
    /// `0` = unknown window (never filtered out on context).
    pub context_window: u32,
    /// Per-Mtok input cost.
    pub input_cost: f32,
    pub healthy: bool,
    pub supports_vision: bool,
    pub supports_tools: bool,
}

/// The `match` half of a rule — every present condition must hold for it to fire.
#[derive(Debug, Clone, Default)]
pub struct Match {
    pub role: Option<Role>,
    /// Fires only when the request needs at least this much context.
    pub min_context: u32,
}

impl Match {
    fn matches(&self, hint: &Hint) -> bool {
        self.role.is_none_or(|r| r == hint.role) && hint.min_context >= self.min_context
    }
}

/// The `prefer` half — how to ORDER the survivors once a rule matches.
#[derive(Debug, Clone, Default)]
pub struct Prefer {
    /// Upstreams carrying these tags sort first (more matches ⇒ earlier).
    pub tags: Vec<String>,
    /// Prefer members at/above this tier at the front.
    pub tier: Option<PoolTier>,
    /// Explicit id order — a listed id sorts by its position, ahead of the unlisted.
    pub upstreams: Vec<String>,
}

impl Prefer {
    /// A total, deterministic sort key; **lower is more-preferred**:
    /// (explicit-position, −tag-overlap, −preferred-tier, id) — the trailing id makes
    /// ties stable and reproducible (important for the bench + tests).
    fn rank(&self, u: &UpstreamMeta) -> (usize, i64, i64, String) {
        let pos = self
            .upstreams
            .iter()
            .position(|id| id == &u.id)
            .unwrap_or(self.upstreams.len());
        let tag_overlap = u.tags.iter().filter(|t| self.tags.contains(t)).count() as i64;
        let tier_bonus = match self.tier {
            Some(t) if u.tier >= t => u.tier as i64,
            _ => 0,
        };
        (pos, -tag_overlap, -tier_bonus, u.id.clone())
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
    /// Resolve `hint` against the `fleet`, returning eligible upstream ids
    /// most-preferred first. Pure, deterministic, and total: an empty result means
    /// *no upstream can serve this request* (fail-soft — the caller decides), never a
    /// panic, even on a degenerate fleet or hostile requirement.
    pub fn resolve(&self, hint: &Hint, fleet: &[UpstreamMeta]) -> Vec<String> {
        // 1. Hard filter: only upstreams that CAN serve the request survive.
        let mut eligible: Vec<&UpstreamMeta> = fleet
            .iter()
            .filter(|u| u.healthy)
            .filter(|u| !hint.needs_vision || u.supports_vision)
            .filter(|u| !hint.needs_tools || u.supports_tools)
            .filter(|u| {
                hint.min_context == 0
                    || u.context_window == 0
                    || u.context_window >= hint.min_context
            })
            .filter(|u| hint.tier.is_none_or(|t| u.tier >= t))
            .filter(|u| hint.max_cost.is_none_or(|mc| u.input_cost <= mc))
            .collect();

        // An explicit override wins outright — but only if it passed the filter, so a
        // model-supplied id can never dial an ineligible/unknown upstream.
        if let Some(ov) = &hint.override_upstream {
            if let Some(u) = eligible.iter().find(|u| &u.id == ov) {
                return vec![u.id.clone()];
            }
        }

        // 2. The first matching rule sets the ordering (else the default preference).
        let prefer = self
            .rules
            .iter()
            .find(|r| r.match_.matches(hint))
            .map_or(&self.default_prefer, |r| &r.prefer);

        // 3. Order the survivors by preference (stable, deterministic). Cached-key so
        // each upstream's rank (which allocates its id for the tie-break) is built once.
        eligible.sort_by_cached_key(|u| prefer.rank(u));
        eligible.into_iter().map(|u| u.id.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn up(id: &str, tags: &[&str], tier: PoolTier, window: u32, cost: f32) -> UpstreamMeta {
        UpstreamMeta {
            id: id.into(),
            tags: tags.iter().map(|s| (*s).to_string()).collect(),
            tier,
            context_window: window,
            input_cost: cost,
            healthy: true,
            supports_vision: false,
            supports_tools: true,
        }
    }

    fn fleet() -> Vec<UpstreamMeta> {
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
                min_context: 0,
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
}
