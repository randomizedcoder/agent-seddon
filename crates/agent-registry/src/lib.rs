//! `ProviderRegistry` seam implementations (model-router increment 03): the
//! control plane holding the model router's upstream fleet + routing policy.
//!
//! Three backends, selected by config like every other seam:
//! - [`MemoryRegistry`] — in-process, the base for tests/benches and the shape
//!   a serve-only process falls back to with no file configured.
//! - [`FileRegistry`] — one `ModelRouterConfig` **textproto** bundle on disk:
//!   the same file `agent --model-router-config <file>` loads at startup,
//!   hand-edited *or* rewritten by a control-plane `Put` (one format for both).
//! - `SqliteRegistry` (feature `registry-sqlite`) — cards/policy as
//!   prost-encoded blobs in embedded SQLite.
//!
//! **Untrusted input, fail closed.** Every id may become a storage path
//! segment; every card arrives from a gRPC peer or a hand-edited file. Stores
//! validate ids (`safe_segment`), clamp hostile numbers (`sanitize`), and cap
//! sizes/counts *before* anything is persisted. `api_key_ref` is stored and
//! served verbatim as a **reference** — no store ever resolves it (there is no
//! key to leak).

use agent_core::{
    safe_segment, Error, ModelRouterConfig, PoolMemberState, PoolTier, ProviderRegistry, Result,
    RouteDecision, RouteHint, RoutePolicySpec, RouteRole, Upstream, UpstreamHealth,
    MAX_REGISTRY_UPSTREAMS,
};
use agent_providers::route;
use async_trait::async_trait;
use std::sync::Mutex;

pub mod file;
pub mod textproto;
pub use file::FileRegistry;
#[cfg(feature = "registry-sqlite")]
pub mod sqlite;
#[cfg(feature = "registry-sqlite")]
pub use sqlite::SqliteRegistry;

/// Answer the `Route` introspection question over a config snapshot: what would
/// the `TaskRouter` pick for `hint`, and why. Pure and deterministic — it runs
/// the same `route::Policy` engine the router runs (same ordering, same
/// override-only-when-eligible rule), with the store's static view of health
/// (every enabled card healthy; live signals arrive with increment 04).
pub fn decide(cfg: &ModelRouterConfig, hint: &RouteHint) -> RouteDecision {
    let mut hint = hint.clone();
    hint.sanitize(); // the hint is attacker-controlled; defense in depth
    let policy = engine_policy(&cfg.policy);
    let fleet: Vec<route::UpstreamMeta<'_>> = cfg
        .upstreams
        .iter()
        .filter(|u| u.enabled)
        .map(|u| route::UpstreamMeta {
            id: &u.id,
            tags: &u.tags,
            tier: u.tier.unwrap_or(PoolTier::Medium),
            context_window: u.context_window,
            input_cost: u.input_cost,
            healthy: true,
            supports_vision: u.supports_vision,
            supports_tools: u.supports_tools,
            // A plain store has no live traffic view; 0 is the neutral
            // ordering value (the router's own dispatch accounting feeds real
            // numbers in the registry-backed path, model-router 04).
            in_flight: 0,
            latency_ewma_ms: 0,
            // Carried so this preview matches the live router's capacity-normalised
            // ordering; inert here (in_flight 0 ⇒ ratio 0 for all).
            max_concurrency: u.max_concurrency,
        })
        .collect();
    let engine_hint = route::Hint {
        role: hint.role.unwrap_or(RouteRole::Main),
        task_mode: hint.task_mode,
        min_context: hint.min_context,
        // Introspection routes on the hint alone — there is no request body to
        // derive capability requirements from (the router itself always derives
        // them; a hint can neither assert nor clear them).
        needs_vision: false,
        needs_tools: false,
        max_cost: hint.max_cost,
        tier: hint.tier,
        override_upstream: hint.override_upstream.clone(),
    };
    let (order, rule) = policy.resolve_with_rule(&engine_hint, &fleet);
    let overridden = matches!(
        (&hint.override_upstream, order.first()),
        (Some(ov), Some(first)) if ov == first && order.len() == 1
    );
    let label = if overridden {
        "override".to_string()
    } else {
        rule.map_or_else(|| "default".to_string(), |i| format!("rule{i}"))
    };
    let chosen = order.first().cloned().unwrap_or_default();
    let why = format!(
        "role={} mode={} {}: {} eligible of {} enabled",
        engine_hint.role.as_str(),
        engine_hint.task_mode.map_or("none", |m| m.as_str()),
        label,
        order.len(),
        fleet.len(),
    );
    RouteDecision {
        chosen,
        order,
        rule: label,
        why,
    }
}

/// The engine mapping lives with the engine ([`route::Policy::from_spec`]) so
/// the registry's `Route` introspection and the registry-backed router can
/// never diverge.
fn engine_policy(spec: &RoutePolicySpec) -> route::Policy {
    route::Policy::from_spec(spec)
}

/// The static health view a plain store reports: every enabled card `Healthy`
/// with zero counters (live numbers arrive when the router feeds the registry,
/// increment 04).
fn static_health(cfg: &ModelRouterConfig) -> Vec<UpstreamHealth> {
    cfg.upstreams
        .iter()
        .filter(|u| u.enabled)
        .map(|u| UpstreamHealth {
            id: u.id.clone(),
            state: PoolMemberState::Healthy,
            ..Default::default()
        })
        .collect()
}

/// Shared fail-closed id gate for lookups: a hostile id is rejected before it
/// is compared (and before it could reach a storage path in any backend).
fn check_id(id: &str) -> Result<()> {
    if safe_segment(id) {
        Ok(())
    } else {
        Err(Error::Registry("invalid upstream id".into()))
    }
}

fn not_found(id: &str) -> Error {
    // The `not found` prefix is the seam contract: the wire layer maps it to
    // gRPC NotFound (id is safe to echo — it passed `check_id`).
    Error::Registry(format!("not found: upstream `{id}`"))
}

/// Apply one CRUD mutation to a config snapshot. All three backends route their
/// writes through these, so validation/clamps/caps can never drift between them.
mod ops {
    use super::*;

    pub fn put(cfg: &mut ModelRouterConfig, mut card: Upstream) -> Result<Upstream> {
        card.sanitize();
        card.validate()?;
        if card.id == "task-router" {
            return Err(Error::Registry(
                "an upstream must not be named `task-router` (the router itself)".into(),
            ));
        }
        match cfg.upstreams.iter_mut().find(|u| u.id == card.id) {
            Some(slot) => *slot = card.clone(),
            None => {
                if cfg.upstreams.len() >= MAX_REGISTRY_UPSTREAMS {
                    return Err(Error::Registry(format!(
                        "registry is full ({MAX_REGISTRY_UPSTREAMS} upstreams)"
                    )));
                }
                cfg.upstreams.push(card.clone());
            }
        }
        Ok(card)
    }

    pub fn delete(cfg: &mut ModelRouterConfig, id: &str) -> Result<bool> {
        check_id(id)?;
        let before = cfg.upstreams.len();
        cfg.upstreams.retain(|u| u.id != id);
        Ok(cfg.upstreams.len() != before)
    }

    pub fn enable(cfg: &mut ModelRouterConfig, id: &str, enabled: bool) -> Result<Upstream> {
        check_id(id)?;
        let card = cfg
            .upstreams
            .iter_mut()
            .find(|u| u.id == id)
            .ok_or_else(|| not_found(id))?;
        card.enabled = enabled;
        Ok(card.clone())
    }

    pub fn put_policy(
        cfg: &mut ModelRouterConfig,
        mut p: RoutePolicySpec,
    ) -> Result<RoutePolicySpec> {
        p.sanitize();
        p.validate()?;
        cfg.policy = p.clone();
        Ok(p)
    }
}

/// The in-process [`ProviderRegistry`]: one config snapshot behind a mutex.
/// The base for tests/benches, and what a serve-only process runs with no
/// backing file configured.
pub struct MemoryRegistry {
    cfg: Mutex<ModelRouterConfig>,
}

impl MemoryRegistry {
    /// Start from a config (sanitized + validated — a bad initial config is a
    /// construction error, never a partially-loaded registry).
    pub fn new(mut cfg: ModelRouterConfig) -> Result<Self> {
        cfg.sanitize();
        cfg.validate()?;
        Ok(Self {
            cfg: Mutex::new(cfg),
        })
    }

    pub fn empty() -> Self {
        Self {
            cfg: Mutex::new(ModelRouterConfig::default()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, ModelRouterConfig> {
        // A poisoned lock means a panic mid-mutation; the snapshot is still a
        // consistent value (mutations build the new state before storing it).
        self.cfg
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[async_trait]
impl ProviderRegistry for MemoryRegistry {
    async fn list(&self) -> Result<Vec<Upstream>> {
        Ok(self.lock().upstreams.clone())
    }
    async fn get(&self, id: &str) -> Result<Upstream> {
        check_id(id)?;
        self.lock()
            .upstreams
            .iter()
            .find(|u| u.id == id)
            .cloned()
            .ok_or_else(|| not_found(id))
    }
    async fn put(&self, card: Upstream) -> Result<Upstream> {
        ops::put(&mut self.lock(), card)
    }
    async fn delete(&self, id: &str) -> Result<bool> {
        ops::delete(&mut self.lock(), id)
    }
    async fn enable(&self, id: &str, enabled: bool) -> Result<Upstream> {
        ops::enable(&mut self.lock(), id, enabled)
    }
    async fn get_policy(&self) -> Result<RoutePolicySpec> {
        Ok(self.lock().policy.clone())
    }
    async fn put_policy(&self, policy: RoutePolicySpec) -> Result<RoutePolicySpec> {
        ops::put_policy(&mut self.lock(), policy)
    }
    async fn route(&self, hint: &RouteHint) -> Result<RouteDecision> {
        Ok(decide(&self.lock(), hint))
    }
    async fn health(&self) -> Result<Vec<UpstreamHealth>> {
        Ok(static_health(&self.lock()))
    }
}

#[cfg(test)]
pub(crate) mod testdata {
    use super::*;

    pub fn card(id: &str) -> Upstream {
        Upstream {
            id: id.into(),
            kind: "openai-compat".into(),
            enabled: true,
            base_url: format!("http://127.0.0.1:1/{id}"),
            model: "m".into(),
            api_key_ref: "env:TEST_KEY".into(),
            context_window: 128_000,
            supports_tools: true,
            tier: Some(PoolTier::Medium),
            ..Default::default()
        }
    }

    /// A two-upstream fleet with a judge rule preferring the heavy member —
    /// the canonical Kimi+GLM shape the design docs use.
    pub fn config() -> ModelRouterConfig {
        let mut kimi = card("kimi");
        kimi.tier = Some(PoolTier::Heavy);
        kimi.tags = vec!["reasoning".into()];
        let glm = card("glm");
        ModelRouterConfig {
            upstreams: vec![kimi, glm],
            policy: RoutePolicySpec {
                rules: vec![agent_core::RouteRuleSpec {
                    match_: agent_core::RouteMatchSpec {
                        role: Some(RouteRole::Judge),
                        ..Default::default()
                    },
                    prefer: agent_core::RoutePreferSpec {
                        tags: vec!["reasoning".into()],
                        tier: Some(PoolTier::Heavy),
                        ..Default::default()
                    },
                }],
                default_prefer: agent_core::RoutePreferSpec {
                    upstreams: vec!["glm".into(), "kimi".into()],
                    ..Default::default()
                },
                failure_threshold: 0,
                cooldown_secs: 0,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testdata::{card, config};
    use super::*;
    use rstest::rstest;

    fn hint(role: RouteRole) -> RouteHint {
        RouteHint {
            role: Some(role),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn positive_put_get_roundtrips_and_lists() {
        let reg = MemoryRegistry::empty();
        let stored = reg.put(card("kimi")).await.expect("put");
        assert_eq!(stored, card("kimi"));
        assert_eq!(reg.get("kimi").await.expect("get"), card("kimi"));
        assert_eq!(reg.list().await.expect("list").len(), 1);
    }

    #[tokio::test]
    async fn positive_delete_true_then_false() {
        let reg = MemoryRegistry::new(config()).expect("valid");
        assert!(reg.delete("glm").await.expect("first delete"));
        assert!(!reg.delete("glm").await.expect("second delete"));
    }

    #[tokio::test]
    async fn positive_enable_toggles_routing_but_keeps_the_card() {
        let reg = MemoryRegistry::new(config()).expect("valid");
        let off = reg.enable("kimi", false).await.expect("disable");
        assert!(!off.enabled);
        // Still listed (the definition survives) …
        assert_eq!(reg.list().await.unwrap().len(), 2);
        // … but no longer routed or reported healthy.
        let d = reg.route(&hint(RouteRole::Judge)).await.unwrap();
        assert_eq!(d.chosen, "glm");
        assert!(reg.health().await.unwrap().iter().all(|h| h.id != "kimi"));
        // Re-enable restores it.
        assert!(reg.enable("kimi", true).await.expect("enable").enabled);
        let d = reg.route(&hint(RouteRole::Judge)).await.unwrap();
        assert_eq!(d.chosen, "kimi");
    }

    #[tokio::test]
    async fn negative_get_unknown_is_not_found() {
        let reg = MemoryRegistry::empty();
        let err = reg.get("ghost").await.expect_err("unknown id");
        assert!(err.to_string().contains("not found"), "{err}");
    }

    #[rstest]
    #[case::judge_rule_prefers_heavy(RouteRole::Judge, "kimi", "rule0")]
    #[case::main_falls_to_default(RouteRole::Main, "glm", "default")]
    fn positive_route_matches_engine(
        #[case] role: RouteRole,
        #[case] chosen: &str,
        #[case] rule: &str,
    ) {
        let d = decide(&config(), &hint(role));
        assert_eq!(d.chosen, chosen);
        assert_eq!(d.rule, rule);
        assert_eq!(d.order.len(), 2);
        assert!(d.why.contains(rule), "{}", d.why);
    }

    #[test]
    fn positive_route_override_when_eligible() {
        let d = decide(
            &config(),
            &RouteHint {
                role: Some(RouteRole::Judge),
                override_upstream: Some("glm".into()),
                ..Default::default()
            },
        );
        assert_eq!(d.chosen, "glm");
        assert_eq!(d.rule, "override");
    }

    #[test]
    fn corner_empty_registry_routes_to_no_candidate() {
        let d = decide(&ModelRouterConfig::default(), &RouteHint::default());
        assert_eq!(d.chosen, "");
        assert!(d.order.is_empty());
        assert_eq!(d.rule, "default");
    }

    #[test]
    fn corner_zero_rule_policy_is_default_only() {
        let mut cfg = config();
        cfg.policy.rules.clear();
        let d = decide(&cfg, &hint(RouteRole::Judge));
        assert_eq!(d.chosen, "glm"); // default prefer order
        assert_eq!(d.rule, "default");
    }

    #[tokio::test]
    async fn adversarial_hostile_ids_rejected_everywhere() {
        let reg = MemoryRegistry::new(config()).expect("valid");
        for id in ["../../etc/passwd", "a/b", "-rf", "", &"x".repeat(300)] {
            assert!(reg.get(id).await.is_err(), "get {id:?}");
            assert!(reg.delete(id).await.is_err(), "delete {id:?}");
            assert!(reg.enable(id, true).await.is_err(), "enable {id:?}");
            let mut bad = card("ok");
            bad.id = id.into();
            assert!(reg.put(bad).await.is_err(), "put {id:?}");
        }
        // Nothing was mutated by the rejected calls.
        assert_eq!(reg.list().await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn adversarial_put_clamps_hostile_numbers_before_storing() {
        let reg = MemoryRegistry::empty();
        let mut evil = card("evil");
        evil.input_cost = f32::NAN;
        evil.weight = f32::INFINITY;
        evil.context_window = u32::MAX;
        let stored = reg.put(evil).await.expect("stored, clamped");
        assert_eq!(stored.input_cost, 0.0);
        assert_eq!(stored.weight, 0.0);
        assert_eq!(stored.context_window, agent_core::MAX_ROUTE_MIN_CONTEXT);
    }

    #[tokio::test]
    async fn adversarial_raw_secret_in_api_key_ref_rejected() {
        let reg = MemoryRegistry::empty();
        let mut bad = card("x");
        bad.api_key_ref = "sk-live-secret".into();
        let err = reg.put(bad).await.expect_err("raw key rejected");
        assert!(!err.to_string().contains("sk-live"), "no echo: {err}");
    }

    #[tokio::test]
    async fn adversarial_route_hint_hostility_fails_soft() {
        let reg = MemoryRegistry::new(config()).expect("valid");
        let d = reg
            .route(&RouteHint {
                min_context: u32::MAX,
                max_cost: Some(f32::NAN),
                override_upstream: Some("x".repeat(64 * 1024)),
                ..Default::default()
            })
            .await
            .expect("never errors");
        // Hostile values sanitized: the huge min_context is capped (above every
        // card's window ⇒ nothing eligible is fine, but never a panic).
        assert!(d.order.len() <= 2);
    }

    #[tokio::test]
    async fn boundary_registry_full_rejects_insert_but_allows_update() {
        let reg = MemoryRegistry::empty();
        for i in 0..MAX_REGISTRY_UPSTREAMS {
            reg.put(card(&format!("u{i}"))).await.expect("fits");
        }
        assert!(reg.put(card("one-too-many")).await.is_err());
        // Updating an existing card is not an insert.
        let mut upd = card("u0");
        upd.model = "m2".into();
        assert_eq!(reg.put(upd).await.expect("update").model, "m2");
    }

    #[tokio::test]
    async fn negative_bad_policy_rejected_and_previous_kept() {
        let reg = MemoryRegistry::new(config()).expect("valid");
        let mut bad = RoutePolicySpec::default();
        bad.default_prefer.policy = "ask-a-magic-8-ball".into();
        assert!(reg.put_policy(bad).await.is_err());
        assert_eq!(reg.get_policy().await.unwrap(), config().policy);
    }

    #[test]
    fn boundary_min_context_filters_by_window() {
        let mut cfg = config();
        cfg.upstreams[0].context_window = 200_000; // kimi
        cfg.upstreams[1].context_window = 8_000; // glm
        let d = decide(
            &cfg,
            &RouteHint {
                min_context: 100_000,
                ..Default::default()
            },
        );
        assert_eq!(d.order, vec!["kimi".to_string()]);
    }
}
