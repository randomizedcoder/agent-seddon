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
//! This slice derives the [`Hint`] from the request's EXACT requirements
//! (needs-tools / needs-vision) with a fixed role; threading the classified task mode,
//! per-call role, and explicit override onto the request is a following slice. See
//! docs/design/model-router/02-routing.md.

use crate::route::{Hint, Policy, Role, UpstreamMeta};
use crate::router::{Health, RouteEvent, RouteObserver};
use agent_core::{
    ChunkStream, CompletionRequest, CompletionResponse, Error, LlmProvider, ModelCapabilities,
    PoolTier, Result,
};
use async_trait::async_trait;
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

/// A provider that routes each request to a declaratively-preferred, capable upstream
/// and fails over on a retryable error — the drop-in generator for task-aware routing.
pub struct TaskRouter {
    upstreams: Vec<RouterUpstream>,
    health: Vec<Health>,
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
        Ok(Self {
            upstreams,
            health,
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

    /// The per-request hint, from the request's EXACT requirements (no estimation):
    /// tool/media presence force capability filters; the role is fixed for now.
    fn hint(&self, req: &CompletionRequest) -> Hint {
        Hint {
            role: self.role,
            needs_tools: !req.tools.is_empty(),
            needs_vision: req.messages.iter().any(agent_core::Message::has_media),
            ..Default::default()
        }
    }

    /// A live view of one upstream: capability facts from the provider, routing
    /// metadata from config. `healthy = true` here (config-enabled); the circuit
    /// breaker is applied as a reorder in [`Self::order`], not as a hard filter, so a
    /// dead upstream is tried last rather than dropped.
    fn meta(&self, u: &RouterUpstream) -> UpstreamMeta {
        let caps = u.provider.capabilities();
        UpstreamMeta {
            id: u.id.clone(),
            tags: u.tags.clone(),
            tier: u.tier,
            context_window: caps.context_window,
            input_cost: u.input_cost,
            healthy: true,
            supports_vision: caps.supports_vision,
            supports_tools: caps.supports_tools,
        }
    }

    /// Indices to try, in order: the policy's preferred-and-capable order, with open
    /// breakers moved to the back (skipped-then-tried-last).
    fn order(&self, req: &CompletionRequest) -> Vec<usize> {
        let now = (self.now_ms)();
        let fleet: Vec<UpstreamMeta> = self.upstreams.iter().map(|u| self.meta(u)).collect();
        let ordered = self.policy.resolve(&self.hint(req), &fleet);

        let mut healthy = Vec::new();
        let mut unhealthy = Vec::new();
        for id in ordered {
            if let Some(i) = self.upstreams.iter().position(|u| u.id == id) {
                if self.health[i].is_open(now, self.cooldown_ms) {
                    self.emit(RouteEvent::SkippedUnhealthy {
                        target: &self.upstreams[i].id,
                    });
                    unhealthy.push(i);
                } else {
                    healthy.push(i);
                }
            }
        }
        healthy.extend(unhealthy);
        healthy
    }

    /// Try each chosen upstream in turn, stopping at the first success or the first
    /// **terminal** failure (mirrors `Router::route`).
    async fn route<T, F, Fut>(&self, req: &CompletionRequest, op: F) -> Result<T>
    where
        F: Fn(Arc<dyn LlmProvider>) -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        let order = self.order(req);
        if order.is_empty() {
            return Err(Error::Provider(
                "no upstream can serve this request (capability/requirement mismatch)".into(),
            ));
        }
        let mut last: Option<Error> = None;
        for (attempt, &i) in order.iter().enumerate() {
            let u = &self.upstreams[i];
            self.emit(RouteEvent::Routed { target: &u.id });
            match op(u.provider.clone()).await {
                Ok(v) => {
                    self.health[i].record_success();
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
}
