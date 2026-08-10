//! `TaskRouter` — a metadata-driven, declaratively-routed provider (model-router
//! increment 02).
//!
//! Like [`crate::router::Router`] it **is-a** `LlmProvider` composing others with the
//! same failover safety — only a retryable failure advances to the next upstream, a
//! terminal one (auth/billing/bad-request) stops, and an open circuit breaker is
//! skipped-then-tried-last so a total outage still attempts *something*. What it adds
//! is the *decision*: it runs a declarative [`route::Policy`] over each upstream's
//! **live** capabilities (context window, tools, vision — read from the provider) plus
//! its **configured** routing metadata (tags / tier / cost) against the request's
//! requirements, so a preferred model is used first and a request that needs a
//! capability lands only on an upstream that has it.
//!
//! The [`Hint`] merges **per-request** signals (a `CompletionRequest`'s
//! [`agent_core::RouteHint`]: classified task mode, per-call role, override,
//! cost/tier caps — model-router 02b) with **derived facts** (needs-tools /
//! needs-vision from the request shape, a cheap context estimate). Derived facts
//! always win: a hint can narrow the fleet but can never clear a real
//! requirement. See docs/design/model-router/02b-hint-threading.md.

use crate::route::{estimate_min_context, Hint, Policy, Role, UpstreamMeta};
use crate::router::{Health, RouteEvent, RouteObserver};
use agent_core::{
    ChunkStream, CompletionRequest, CompletionResponse, Error, LlmProvider, ModelCapabilities,
    PoolTier, Result,
};
use async_trait::async_trait;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

/// One routable upstream: the (already metered) provider plus the operator's routing
/// metadata. Capability facts (context window, tool/vision support) are read *live*
/// from the provider; `tags` / `tier` / `input_cost` are the config-supplied signals
/// the policy matches and orders on.
pub struct RouterUpstream {
    pub id: String,
    pub tags: Vec<String>,
    pub tier: PoolTier,
    /// Per-Mtok input cost (a routing hint; clamped non-negative on build).
    pub input_cost: f32,
    pub provider: Arc<dyn LlmProvider>,
}

/// Per-upstream live dispatch accounting (model-router 04): requests currently
/// in flight and a smoothed latency, fed by the router's own dispatch path and
/// read by the [`crate::route::OrderPolicy`] live-signal ordering. Lock-free —
/// this sits on the per-call hot path.
#[derive(Default)]
pub(crate) struct LiveStats {
    in_flight: AtomicU32,
    latency_ewma_ms: AtomicU32,
}

impl LiveStats {
    pub(crate) fn snapshot(&self) -> (u32, u32) {
        (
            self.in_flight.load(Ordering::Relaxed),
            self.latency_ewma_ms.load(Ordering::Relaxed),
        )
    }
    /// EWMA with α=0.3 in integer math; the first sample seeds the average.
    fn record_latency(&self, sample_ms: u32) {
        let old = self.latency_ewma_ms.load(Ordering::Relaxed);
        let new = if old == 0 {
            sample_ms
        } else {
            (old.saturating_mul(7) + sample_ms.saturating_mul(3)) / 10
        };
        self.latency_ewma_ms.store(new, Ordering::Relaxed);
    }
}

/// RAII in-flight guard: decrements on every exit path (incl. panic/cancel), so
/// the least-loaded signal can never drift upward from a lost decrement.
struct InFlightGuard<'a>(&'a AtomicU32);
impl<'a> InFlightGuard<'a> {
    fn enter(counter: &'a AtomicU32) -> Self {
        counter.fetch_add(1, Ordering::Relaxed);
        Self(counter)
    }
}
impl Drop for InFlightGuard<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

/// A provider that routes each request to a declaratively-preferred, capable upstream
/// and fails over on a retryable error — the drop-in generator for task-aware routing.
pub struct TaskRouter {
    upstreams: Vec<RouterUpstream>,
    health: Vec<Health>,
    live: Vec<LiveStats>,
    policy: Policy,
    role: Role,
    failure_threshold: usize,
    cooldown_ms: u64,
    now_ms: Arc<dyn Fn() -> u64 + Send + Sync>,
    observer: Option<RouteObserver>,
}

impl TaskRouter {
    /// Build a router over `upstreams` steered by `policy`. Errors on an empty fleet
    /// (a router with nothing to route to can never answer). Per-member `input_cost`
    /// is clamped finite + non-negative so a hostile config can't poison ordering.
    pub fn new(mut upstreams: Vec<RouterUpstream>, policy: Policy) -> Result<Self> {
        if upstreams.is_empty() {
            return Err(Error::Provider(
                "task-router needs at least one upstream".into(),
            ));
        }
        for u in &mut upstreams {
            if !u.input_cost.is_finite() || u.input_cost < 0.0 {
                u.input_cost = 0.0;
            }
        }
        let health = upstreams.iter().map(|_| Health::new()).collect();
        let live = upstreams.iter().map(|_| LiveStats::default()).collect();
        Ok(Self {
            upstreams,
            health,
            live,
            policy,
            role: Role::Main,
            failure_threshold: 3,
            cooldown_ms: 30_000,
            now_ms: Arc::new(crate::router::wall_clock_ms),
            observer: None,
        })
    }

    pub fn with_breaker(mut self, threshold: usize, cooldown_ms: u64) -> Self {
        self.failure_threshold = threshold.max(1);
        self.cooldown_ms = cooldown_ms;
        self
    }
    pub fn with_role(mut self, role: Role) -> Self {
        self.role = role;
        self
    }
    pub fn with_clock(mut self, now_ms: Arc<dyn Fn() -> u64 + Send + Sync>) -> Self {
        self.now_ms = now_ms;
        self
    }
    pub fn with_observer(mut self, observer: RouteObserver) -> Self {
        self.observer = Some(observer);
        self
    }

    fn emit(&self, ev: RouteEvent<'_>) {
        if let Some(o) = &self.observer {
            o(ev);
        }
    }

    /// The per-request hint: the request's carried [`agent_core::RouteHint`]
    /// merged with derived facts. The carried hint is re-sanitized here (defense
    /// in depth — wire decode sanitizes too, but an in-process caller may not);
    /// `needs_tools`/`needs_vision` are ALWAYS derived from the request itself,
    /// so a hostile hint can't steer a tool-call request onto a tool-less
    /// upstream; `min_context` falls back to a cheap chars/4 estimate.
    fn hint(&self, req: &CompletionRequest) -> Hint {
        let mut carried = req.route.clone().unwrap_or_default();
        carried.sanitize();
        Hint {
            role: carried.role.unwrap_or(self.role),
            task_mode: carried.task_mode,
            needs_tools: !req.tools.is_empty(),
            needs_vision: req.messages.iter().any(agent_core::Message::has_media),
            min_context: if carried.min_context > 0 {
                carried.min_context
            } else {
                estimate_min_context(req)
            },
            max_cost: carried.max_cost,
            tier: carried.tier,
            override_upstream: carried.override_upstream,
        }
    }

    /// A live view of one upstream: capability facts from the provider, routing
    /// metadata **borrowed** from config (the decision path allocates no id/tag
    /// clones — it runs on every routed call). `healthy = true` here
    /// (config-enabled); the circuit breaker is applied as a reorder in
    /// [`Self::order`], not as a hard filter, so a dead upstream is tried last
    /// rather than dropped.
    fn meta(&self, i: usize) -> UpstreamMeta<'_> {
        let u = &self.upstreams[i];
        let caps = u.provider.capabilities();
        let (in_flight, latency_ewma_ms) = self.live[i].snapshot();
        UpstreamMeta {
            id: &u.id,
            tags: &u.tags,
            tier: u.tier,
            context_window: caps.context_window,
            input_cost: u.input_cost,
            healthy: true,
            supports_vision: caps.supports_vision,
            supports_tools: caps.supports_tools,
            in_flight,
            latency_ewma_ms,
        }
    }

    /// Indices to try, in order: the policy's preferred-and-capable order, with open
    /// breakers moved to the back (skipped-then-tried-last). Also returns which
    /// rule decided (for the `Decided` event). The engine resolves straight to
    /// fleet indices (same order as `self.upstreams`) — no by-id re-lookup.
    fn order(&self, hint: &Hint) -> (Vec<usize>, Option<usize>) {
        let now = (self.now_ms)();
        let fleet: Vec<UpstreamMeta<'_>> =
            (0..self.upstreams.len()).map(|i| self.meta(i)).collect();
        let (ordered, rule) = self.policy.resolve_indices(hint, &fleet);

        let mut healthy = Vec::new();
        let mut unhealthy = Vec::new();
        for i in ordered {
            if self.health[i].is_open(now, self.cooldown_ms) {
                self.emit(RouteEvent::SkippedUnhealthy {
                    target: &self.upstreams[i].id,
                });
                unhealthy.push(i);
            } else {
                healthy.push(i);
            }
        }
        healthy.extend(unhealthy);
        (healthy, rule)
    }

    /// Try each chosen upstream in turn, stopping at the first success or the first
    /// **terminal** failure (mirrors `Router::route`).
    async fn route<T, F, Fut>(&self, req: &CompletionRequest, op: F) -> Result<T>
    where
        F: Fn(Arc<dyn LlmProvider>) -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        let hint = self.hint(req);
        let (order, rule) = self.order(&hint);
        let mode = hint.task_mode.map_or("-", |m| m.as_str());
        if order.is_empty() {
            self.emit(RouteEvent::NoCandidate {
                role: hint.role.as_str(),
            });
            return Err(Error::Provider(
                "no upstream can serve this request (capability/requirement mismatch)".into(),
            ));
        }
        self.emit(RouteEvent::Decided {
            role: hint.role.as_str(),
            task_mode: mode,
            rule,
            chosen: &self.upstreams[order[0]].id,
        });
        let mut last: Option<Error> = None;
        for (attempt, &i) in order.iter().enumerate() {
            let u = &self.upstreams[i];
            self.emit(RouteEvent::Routed { target: &u.id });
            let started = (self.now_ms)();
            let outcome = {
                let _in_flight = InFlightGuard::enter(&self.live[i].in_flight);
                op(u.provider.clone()).await
            };
            match outcome {
                Ok(v) => {
                    self.health[i].record_success();
                    let elapsed = (self.now_ms)().saturating_sub(started);
                    self.live[i].record_latency(u32::try_from(elapsed).unwrap_or(u32::MAX));
                    return Ok(v);
                }
                Err(e) => {
                    let msg = e.to_string();
                    self.health[i].record_failure((self.now_ms)(), self.failure_threshold);
                    if agent_retry::classify(&msg) == agent_retry::Class::Terminal {
                        return Err(e);
                    }
                    if attempt + 1 < order.len() {
                        self.emit(RouteEvent::FellOver {
                            from: &u.id,
                            reason: "retryable",
                        });
                    }
                    last = Some(e);
                }
            }
        }
        self.emit(RouteEvent::Exhausted);
        Err(last.unwrap_or_else(|| Error::Provider("task-router exhausted all upstreams".into())))
    }
}

#[async_trait]
impl LlmProvider for TaskRouter {
    /// The union of what the upstreams can do — the loop must not disable a feature
    /// just because one upstream lacks it. Context window is the **minimum**, since a
    /// request must fit whichever upstream serves it (mirrors `Router`).
    fn capabilities(&self) -> ModelCapabilities {
        let mut out = ModelCapabilities {
            supports_tools: false,
            context_window: u32::MAX,
            supports_response_format: false,
            supports_vision: false,
        };
        for u in &self.upstreams {
            let caps = u.provider.capabilities();
            out.supports_tools |= caps.supports_tools;
            out.supports_response_format |= caps.supports_response_format;
            out.supports_vision |= caps.supports_vision;
            out.context_window = out.context_window.min(caps.context_window);
        }
        if out.context_window == u32::MAX {
            out.context_window = 0;
        }
        out
    }

    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse> {
        let r = req.clone();
        self.route(&req, move |p| {
            let r = r.clone();
            async move { p.complete(r).await }
        })
        .await
    }

    async fn stream(&self, req: CompletionRequest) -> Result<ChunkStream> {
        // Fallover covers failures raised while *establishing* the stream; once bytes
        // flow the turn is committed (mirrors `Router::stream`).
        let r = req.clone();
        self.route(&req, move |p| {
            let r = r.clone();
            async move { p.stream(r).await }
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::route::Prefer;
    use agent_testkit::{final_turn, ScriptedProvider};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn caps(tools: bool, vision: bool, window: u32) -> ModelCapabilities {
        ModelCapabilities {
            supports_tools: tools,
            context_window: window,
            supports_response_format: false,
            supports_vision: vision,
        }
    }

    /// A provider that always fails with a fixed message, counting its calls.
    struct FailProvider {
        msg: String,
        caps: ModelCapabilities,
        calls: Arc<AtomicUsize>,
    }
    impl FailProvider {
        fn new(msg: &str) -> Self {
            Self {
                msg: msg.into(),
                caps: caps(true, false, 1000),
                calls: Arc::new(AtomicUsize::new(0)),
            }
        }
    }
    #[async_trait]
    impl LlmProvider for FailProvider {
        fn capabilities(&self) -> ModelCapabilities {
            self.caps.clone()
        }
        async fn complete(&self, _r: CompletionRequest) -> Result<CompletionResponse> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Err(Error::Provider(self.msg.clone()))
        }
        async fn stream(&self, _r: CompletionRequest) -> Result<ChunkStream> {
            Err(Error::Provider(self.msg.clone()))
        }
    }

    /// A provider that succeeds with a fixed answer + advertises given caps.
    struct OkProvider {
        answer: String,
        caps: ModelCapabilities,
    }
    #[async_trait]
    impl LlmProvider for OkProvider {
        fn capabilities(&self) -> ModelCapabilities {
            self.caps.clone()
        }
        async fn complete(&self, _r: CompletionRequest) -> Result<CompletionResponse> {
            Ok(ScriptedProvider::new(vec![final_turn(&self.answer)])
                .complete(_r)
                .await
                .unwrap())
        }
        async fn stream(&self, _r: CompletionRequest) -> Result<ChunkStream> {
            Err(Error::Provider("no stream".into()))
        }
    }

    fn ok(answer: &str, tools: bool, vision: bool) -> Arc<dyn LlmProvider> {
        Arc::new(OkProvider {
            answer: answer.into(),
            caps: caps(tools, vision, 1000),
        })
    }

    fn up(id: &str, provider: Arc<dyn LlmProvider>) -> RouterUpstream {
        RouterUpstream {
            id: id.into(),
            tags: vec![],
            tier: PoolTier::Heavy,
            input_cost: 0.0,
            provider,
        }
    }

    fn req() -> CompletionRequest {
        CompletionRequest {
            messages: vec![agent_core::Message::user("hi")],
            tools: vec![],
            max_tokens: 16,
            temperature: 0.0,
            response_format: None,
            route: None,
        }
    }

    fn req_with_tools() -> CompletionRequest {
        let mut r = req();
        r.tools = vec![agent_core::ToolSchema {
            name: "t".into(),
            description: String::new(),
            parameters: serde_json::json!({}),
        }];
        r
    }

    /// A policy whose default preference is an explicit id order.
    fn prefer(ids: &[&str]) -> Policy {
        Policy {
            rules: vec![],
            default_prefer: Prefer {
                tags: vec![],
                tier: None,
                upstreams: ids.iter().map(|s| (*s).to_string()).collect(),
                policy: None,
            },
        }
    }

    fn router(upstreams: Vec<RouterUpstream>, policy: Policy) -> TaskRouter {
        TaskRouter::new(upstreams, policy)
            .expect("router")
            .with_clock(Arc::new(|| 0))
    }

    // --- positive -----------------------------------------------------------
    #[tokio::test]
    async fn positive_routes_to_the_preferred_upstream() {
        // default_prefer lists kimi first → kimi answers, glm is the fallback.
        let r = router(
            vec![
                up("glm", ok("from-glm", true, false)),
                up("kimi", ok("from-kimi", true, false)),
            ],
            prefer(&["kimi", "glm"]),
        );
        let resp = r.complete(req()).await.expect("routes");
        assert_eq!(resp.message.content_text(), "from-kimi");
    }

    #[tokio::test]
    async fn positive_retryable_failure_falls_over_to_next() {
        let bad = Arc::new(FailProvider::new("http 429: slow down"));
        let r = router(
            vec![
                up("kimi", bad.clone()),
                up("glm", ok("from-glm", true, false)),
            ],
            prefer(&["kimi", "glm"]),
        );
        let resp = r.complete(req()).await.expect("falls over");
        assert_eq!(resp.message.content_text(), "from-glm");
        assert_eq!(
            bad.calls.load(Ordering::SeqCst),
            1,
            "primary was tried once"
        );
    }

    // --- negative -----------------------------------------------------------
    #[tokio::test]
    async fn negative_terminal_failure_does_not_fall_over() {
        let primary = Arc::new(FailProvider::new("http 401: unauthorized"));
        let secondary = Arc::new(FailProvider::new("should-not-be-reached"));
        let r = router(
            vec![up("kimi", primary.clone()), up("glm", secondary.clone())],
            prefer(&["kimi", "glm"]),
        );
        assert!(r.complete(req()).await.is_err());
        assert_eq!(
            secondary.calls.load(Ordering::SeqCst),
            0,
            "a terminal failure must not burn the fallback"
        );
    }

    // --- corner -------------------------------------------------------------
    #[tokio::test]
    async fn corner_no_rules_empty_prefer_is_deterministic_by_id() {
        // No rules, no preference → ordered by the id tie-break (glm < kimi).
        let r = router(
            vec![
                up("kimi", ok("from-kimi", true, false)),
                up("glm", ok("from-glm", true, false)),
            ],
            Policy::default(),
        );
        assert_eq!(
            r.complete(req()).await.unwrap().message.content_text(),
            "from-glm"
        );
    }

    // --- boundary -----------------------------------------------------------
    #[tokio::test]
    async fn boundary_single_upstream_always_chosen() {
        let r = router(vec![up("only", ok("solo", true, false))], Policy::default());
        assert_eq!(
            r.complete(req()).await.unwrap().message.content_text(),
            "solo"
        );
    }

    #[test]
    fn boundary_empty_fleet_is_rejected() {
        assert!(TaskRouter::new(vec![], Policy::default()).is_err());
    }

    // --- adversarial --------------------------------------------------------
    #[tokio::test]
    async fn adversarial_capability_filter_excludes_incapable_upstream() {
        // The request needs tools; kimi can't do tools → glm (which can) serves it.
        let r = router(
            vec![
                up("kimi", ok("from-kimi", false, false)), // no tools
                up("glm", ok("from-glm", true, false)),    // tools
            ],
            prefer(&["kimi", "glm"]),
        );
        let resp = r
            .complete(req_with_tools())
            .await
            .expect("routes to capable");
        assert_eq!(resp.message.content_text(), "from-glm");
    }

    #[tokio::test]
    async fn adversarial_no_capable_upstream_fails_soft() {
        // The request needs tools; NO upstream supports them → a clear error, not a
        // dispatch to an incapable model, not a panic.
        let r = router(vec![up("kimi", ok("x", false, false))], Policy::default());
        let err = r
            .complete(req_with_tools())
            .await
            .expect_err("no capable upstream");
        assert!(err.to_string().contains("capability"));
    }

    #[tokio::test]
    async fn adversarial_hostile_cost_is_clamped_on_build() {
        let mut u = up("x", ok("y", true, false));
        u.input_cost = f32::NAN;
        let r = TaskRouter::new(vec![u], Policy::default()).unwrap();
        // NaN cost would poison any cost ordering; it is zeroed on build.
        assert_eq!(r.upstreams[0].input_cost, 0.0);
    }

    // --- 02b: the per-request RouteHint --------------------------------------

    /// A rule steering `role` to an explicit upstream order.
    fn role_rule(role: Role, ids: &[&str]) -> crate::route::Rule {
        crate::route::Rule {
            match_: crate::route::Match {
                role: Some(role),
                ..Default::default()
            },
            prefer: Prefer {
                upstreams: ids.iter().map(|s| (*s).to_string()).collect(),
                ..Default::default()
            },
        }
    }

    fn hinted(mut r: CompletionRequest, hint: agent_core::RouteHint) -> CompletionRequest {
        r.route = Some(hint);
        r
    }

    #[tokio::test]
    async fn positive_carried_role_beats_the_fixed_default() {
        let policy = Policy {
            rules: vec![role_rule(Role::Judge, &["glm", "kimi"])],
            default_prefer: Prefer {
                upstreams: vec!["kimi".into(), "glm".into()],
                ..Default::default()
            },
        };
        let r = router(
            vec![
                up("kimi", ok("from-kimi", true, false)),
                up("glm", ok("from-glm", true, false)),
            ],
            policy,
        );
        // No hint ⇒ the router's fixed role (Main) ⇒ the default preference.
        assert_eq!(
            r.complete(req()).await.unwrap().message.content_text(),
            "from-kimi"
        );
        // A carried Judge role fires the Judge rule per call, same router.
        let judged = hinted(
            req(),
            agent_core::RouteHint {
                role: Some(agent_core::RouteRole::Judge),
                ..Default::default()
            },
        );
        assert_eq!(
            r.complete(judged).await.unwrap().message.content_text(),
            "from-glm"
        );
    }

    #[tokio::test]
    async fn positive_carried_task_mode_fires_mode_rule() {
        let policy = Policy {
            rules: vec![crate::route::Rule {
                match_: crate::route::Match {
                    task_mode: Some(agent_core::TaskMode::Review),
                    ..Default::default()
                },
                prefer: Prefer {
                    upstreams: vec!["glm".into()],
                    ..Default::default()
                },
            }],
            default_prefer: Prefer {
                upstreams: vec!["kimi".into(), "glm".into()],
                ..Default::default()
            },
        };
        let r = router(
            vec![
                up("kimi", ok("from-kimi", true, false)),
                up("glm", ok("from-glm", true, false)),
            ],
            policy,
        );
        let review = hinted(
            req(),
            agent_core::RouteHint {
                task_mode: Some(agent_core::TaskMode::Review),
                ..Default::default()
            },
        );
        assert_eq!(
            r.complete(review).await.unwrap().message.content_text(),
            "from-glm"
        );
        // Without the mode the rule must not fire.
        assert_eq!(
            r.complete(req()).await.unwrap().message.content_text(),
            "from-kimi"
        );
    }

    #[tokio::test]
    async fn positive_decided_event_carries_role_mode_rule_and_choice() {
        let events: Arc<std::sync::Mutex<Vec<String>>> = Arc::default();
        let sink = events.clone();
        let policy = Policy {
            rules: vec![role_rule(Role::Judge, &["glm"])],
            default_prefer: Prefer::default(),
        };
        let r = router(
            vec![
                up("kimi", ok("k", true, false)),
                up("glm", ok("g", true, false)),
            ],
            policy,
        )
        .with_observer(Arc::new(move |ev| {
            if let RouteEvent::Decided {
                role,
                task_mode,
                rule,
                chosen,
            } = ev
            {
                sink.lock()
                    .unwrap()
                    .push(format!("{role}/{task_mode}/{rule:?}/{chosen}"));
            }
        }));
        let judged = hinted(
            req(),
            agent_core::RouteHint {
                role: Some(agent_core::RouteRole::Judge),
                task_mode: Some(agent_core::TaskMode::Debug),
                ..Default::default()
            },
        );
        r.complete(judged).await.unwrap();
        assert_eq!(
            events.lock().unwrap().clone(),
            vec!["judge/debug/Some(0)/glm".to_string()]
        );
    }

    #[tokio::test]
    async fn corner_min_context_is_estimated_when_unset() {
        // ~40k chars ⇒ ~10k token floor: the 1k-window upstream is filtered out,
        // the roomy one serves it even though the preference lists "small" first.
        let small = Arc::new(OkProvider {
            answer: "from-small".into(),
            caps: caps(true, false, 1_000),
        });
        let big = Arc::new(OkProvider {
            answer: "from-big".into(),
            caps: caps(true, false, 100_000),
        });
        let r = router(
            vec![up("small", small), up("big", big)],
            prefer(&["small", "big"]),
        );
        let mut long = req();
        long.messages = vec![agent_core::Message::user("x".repeat(40_000))];
        assert_eq!(
            r.complete(long).await.unwrap().message.content_text(),
            "from-big"
        );
        // A short prompt keeps the preferred small upstream eligible.
        assert_eq!(
            r.complete(req()).await.unwrap().message.content_text(),
            "from-small"
        );
    }

    #[tokio::test]
    async fn boundary_carried_min_context_overrides_the_estimate() {
        let small = Arc::new(OkProvider {
            answer: "from-small".into(),
            caps: caps(true, false, 1_000),
        });
        let big = Arc::new(OkProvider {
            answer: "from-big".into(),
            caps: caps(true, false, 100_000),
        });
        let r = router(
            vec![up("small", small), up("big", big)],
            prefer(&["small", "big"]),
        );
        // A short prompt but an asserted 50k floor ⇒ only the big one fits.
        let asserted = hinted(
            req(),
            agent_core::RouteHint {
                min_context: 50_000,
                ..Default::default()
            },
        );
        assert_eq!(
            r.complete(asserted).await.unwrap().message.content_text(),
            "from-big"
        );
    }

    #[tokio::test]
    async fn adversarial_hint_cannot_clear_derived_needs_tools() {
        // The request carries tools; a hint (whatever it says) cannot steer it
        // onto a tool-less upstream — needs_tools is derived, never hint-set.
        let r = router(
            vec![
                up("kimi", ok("from-kimi", false, false)), // no tools
                up("glm", ok("from-glm", true, false)),
            ],
            prefer(&["kimi", "glm"]),
        );
        let sneaky = hinted(
            req_with_tools(),
            agent_core::RouteHint {
                override_upstream: Some("kimi".into()),
                ..Default::default()
            },
        );
        // Even the explicit override can't select the ineligible upstream.
        assert_eq!(
            r.complete(sneaky).await.unwrap().message.content_text(),
            "from-glm"
        );
    }

    #[tokio::test]
    async fn adversarial_hostile_hint_numbers_fail_soft_with_no_candidate() {
        let events: Arc<std::sync::Mutex<Vec<String>>> = Arc::default();
        let sink = events.clone();
        let r = router(
            vec![up("kimi", ok("k", true, false))], // window 1000
            Policy::default(),
        )
        .with_observer(Arc::new(move |ev| {
            if let RouteEvent::NoCandidate { role } = ev {
                sink.lock().unwrap().push(role.to_string());
            }
        }));
        let hostile = hinted(
            req(),
            agent_core::RouteHint {
                min_context: u32::MAX, // sanitized to the cap, still unservable
                max_cost: Some(f32::NAN),
                override_upstream: Some("z".repeat(4096)),
                ..Default::default()
            },
        );
        let err = r.complete(hostile).await.expect_err("no candidate");
        assert!(err.to_string().contains("capability"), "{err}");
        assert_eq!(events.lock().unwrap().clone(), vec!["main".to_string()]);
    }

    #[tokio::test]
    async fn adversarial_overlong_override_is_dropped_not_dialed() {
        // An over-long override id is dropped wholesale; routing proceeds
        // normally instead of comparing (or logging) a hostile 4KiB string.
        let r = router(
            vec![
                up("kimi", ok("from-kimi", true, false)),
                up("glm", ok("from-glm", true, false)),
            ],
            prefer(&["kimi", "glm"]),
        );
        let sneaky = hinted(
            req(),
            agent_core::RouteHint {
                override_upstream: Some("k".repeat(4096)),
                ..Default::default()
            },
        );
        assert_eq!(
            r.complete(sneaky).await.unwrap().message.content_text(),
            "from-kimi"
        );
    }

    // --- live-signal dispatch accounting (model-router 04) ------------------

    /// A provider that advances a shared fake clock by `cost_ms` per call and
    /// records which ids served (for latency-policy steering assertions).
    struct TimedProvider {
        id: &'static str,
        cost_ms: u64,
        clock: Arc<std::sync::atomic::AtomicU64>,
        served: Arc<std::sync::Mutex<Vec<&'static str>>>,
    }
    #[async_trait]
    impl LlmProvider for TimedProvider {
        fn capabilities(&self) -> ModelCapabilities {
            caps(true, false, 100_000)
        }
        async fn complete(&self, r: CompletionRequest) -> Result<CompletionResponse> {
            self.clock
                .fetch_add(self.cost_ms, std::sync::atomic::Ordering::SeqCst);
            self.served.lock().unwrap().push(self.id);
            ScriptedProvider::new(vec![final_turn("ok")])
                .complete(r)
                .await
        }
        async fn stream(&self, _r: CompletionRequest) -> Result<ChunkStream> {
            Err(Error::Provider("no stream".into()))
        }
    }

    #[tokio::test]
    async fn positive_latency_policy_steers_to_the_faster_upstream() {
        let clock = Arc::new(std::sync::atomic::AtomicU64::new(1));
        let served = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mk = |id: &'static str, cost_ms: u64| RouterUpstream {
            id: id.into(),
            tags: vec![],
            tier: PoolTier::Medium,
            input_cost: 0.0,
            provider: Arc::new(TimedProvider {
                id,
                cost_ms,
                clock: clock.clone(),
                served: served.clone(),
            }),
        };
        let policy = Policy {
            rules: vec![],
            default_prefer: Prefer {
                policy: Some(crate::route::OrderPolicy::Latency),
                ..Default::default()
            },
        };
        let c = clock.clone();
        let router = TaskRouter::new(vec![mk("slow", 500), mk("fast", 10)], policy)
            .unwrap()
            .with_clock(Arc::new(move || -> u64 {
                c.load(std::sync::atomic::Ordering::SeqCst)
            }));
        let req = CompletionRequest::default();
        // 1st: both unknown (0 = neutral) -> id order picks "fast"; it records 10ms.
        // 2nd: fast has 10ms, slow has 0 (unknown = neutral-best) -> "slow"; 500ms.
        // 3rd+: both known -> the measured-faster "fast" wins from here on.
        for _ in 0..4 {
            router.complete(req.clone()).await.expect("completes");
        }
        let got = served.lock().unwrap().clone();
        assert_eq!(got, vec!["fast", "slow", "fast", "fast"]);
    }

    #[tokio::test]
    async fn positive_in_flight_guard_returns_to_zero_after_every_outcome() {
        let ok = RouterUpstream {
            id: "ok".into(),
            tags: vec![],
            tier: PoolTier::Medium,
            input_cost: 0.0,
            provider: Arc::new(OkProvider {
                answer: "fine".into(),
                caps: caps(true, false, 1000),
            }),
        };
        let fail = FailProvider::new("http 500: transient");
        let failing = RouterUpstream {
            id: "bad".into(),
            tags: vec![],
            tier: PoolTier::Medium,
            input_cost: 0.0,
            provider: Arc::new(fail),
        };
        let policy = Policy {
            rules: vec![],
            default_prefer: Prefer {
                upstreams: vec!["bad".into(), "ok".into()],
                policy: Some(crate::route::OrderPolicy::LeastLoaded),
                ..Default::default()
            },
        };
        let router = TaskRouter::new(vec![failing, ok], policy).unwrap();
        // Success after a failover: both the failed and the successful attempt
        // must release their in-flight slots.
        router
            .complete(CompletionRequest::default())
            .await
            .expect("fails over to ok");
        for live in &router.live {
            assert_eq!(live.snapshot().0, 0, "in-flight must return to zero");
        }
        // And the successful upstream recorded a latency sample (wall clock:
        // >= 0 is all we can assert deterministically; the seeded value only
        // matters to ordering, covered by the fake-clock test above).
        assert_eq!(
            router.live[0].snapshot().1,
            0,
            "failed attempt records no latency"
        );
    }
}
