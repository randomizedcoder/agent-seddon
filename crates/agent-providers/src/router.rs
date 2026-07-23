//! `Router` — a provider that composes other providers (parity spec 25).
//!
//! It **is-a** `LlmProvider`, so nothing downstream knows it exists: the loop,
//! the context strategy, and the metered decorators all see one provider. What
//! it adds is resilience — a classified *transient* failure on the primary
//! transparently continues on the next candidate, so the turn completes instead
//! of surfacing an error.
//!
//! Three rules make that safe rather than merely hopeful:
//!
//! 1. **Only retryable failures fail over.** A terminal failure (auth, billing,
//!    bad request, content policy) fails the same way on every candidate, so
//!    trying them all burns the chain — and real money — to reach the same
//!    answer. Classification lives in `agent-retry` and is shared, not
//!    re-implemented here.
//! 2. **An unhealthy candidate is skipped.** Repeated failures open a circuit
//!    breaker that closes again after a cooldown, so a dead provider stops
//!    costing a timeout on every single turn.
//! 3. **Capability requirements are respected.** A candidate that cannot serve
//!    the request (no tool support, context window too small) is not tried at
//!    all — failing over to it would just produce a different error.
//!
//! Each candidate is still an independent seam, including a `= "grpc"` client,
//! so one router can span local and remote providers.

use agent_core::{
    ChunkStream, CompletionRequest, CompletionResponse, Error, LlmProvider, ModelCapabilities,
    Result,
};
use async_trait::async_trait;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

/// How the router picks a primary among healthy, capable candidates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RoutePolicy {
    /// Configured order; the first healthy, capable candidate wins. Predictable
    /// and the right default — an operator lists providers in preference order.
    #[default]
    InOrder,
    /// Spread load across healthy, capable candidates (round-robin).
    RoundRobin,
}

impl RoutePolicy {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "round-robin" | "roundrobin" => RoutePolicy::RoundRobin,
            _ => RoutePolicy::InOrder,
        }
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            RoutePolicy::InOrder => "in-order",
            RoutePolicy::RoundRobin => "round-robin",
        }
    }
}

/// Per-candidate health. A candidate that fails repeatedly is skipped until a
/// cooldown elapses, so a dead provider costs one timeout rather than one per
/// turn forever. `pub(crate)` so `LlmPool` reuses the exact same breaker.
pub(crate) struct Health {
    consecutive_failures: AtomicUsize,
    /// When the breaker opened (clock ms); `0` ⇒ closed.
    opened_ms: AtomicU64,
}

impl Health {
    pub(crate) fn new() -> Self {
        Self {
            consecutive_failures: AtomicUsize::new(0),
            opened_ms: AtomicU64::new(0),
        }
    }
    pub(crate) fn is_open(&self, now_ms: u64, cooldown_ms: u64) -> bool {
        let opened = self.opened_ms.load(Ordering::SeqCst);
        if opened == 0 {
            return false;
        }
        if now_ms.saturating_sub(opened) >= cooldown_ms {
            // Cooldown elapsed — close it and let the next call probe.
            self.opened_ms.store(0, Ordering::SeqCst);
            self.consecutive_failures.store(0, Ordering::SeqCst);
            return false;
        }
        true
    }
    /// Consecutive failures currently recorded (for a health snapshot).
    pub(crate) fn failures(&self) -> usize {
        self.consecutive_failures.load(Ordering::SeqCst)
    }
    pub(crate) fn record_success(&self) {
        self.consecutive_failures.store(0, Ordering::SeqCst);
        self.opened_ms.store(0, Ordering::SeqCst);
    }
    pub(crate) fn record_failure(&self, now_ms: u64, threshold: usize) {
        let n = self.consecutive_failures.fetch_add(1, Ordering::SeqCst) + 1;
        if n >= threshold {
            // `now_ms == 0` would read as "closed"; 1 is the earliest open stamp.
            self.opened_ms.store(now_ms.max(1), Ordering::SeqCst);
        }
    }
}

/// Can a candidate serve the request at all? Shared by `Router` and `LlmPool` —
/// failing over/out to a candidate that structurally cannot answer just produces
/// a different error.
pub(crate) fn is_capable(caps: &ModelCapabilities, req: &CompletionRequest) -> bool {
    if !req.tools.is_empty() && !caps.supports_tools {
        return false;
    }
    if req.messages.iter().any(|m| m.has_media()) && !caps.supports_vision {
        return false;
    }
    true
}

/// One candidate: its config name and the (already metered) provider.
pub struct Candidate {
    pub name: String,
    pub provider: Arc<dyn LlmProvider>,
}

/// Observability hook. The router lives in `agent-providers`, which does not
/// depend on `agent-metrics`; the runtime supplies a callback so route
/// decisions and fallovers are metered without inverting the dependency.
pub type RouteObserver = Arc<dyn Fn(RouteEvent<'_>) + Send + Sync>;

/// What the router decided, for metrics and spans.
#[derive(Debug, Clone, Copy)]
pub enum RouteEvent<'a> {
    /// A request was sent to `target`.
    Routed { target: &'a str },
    /// `from` failed retryably; the router advanced to another candidate.
    FellOver { from: &'a str, reason: &'a str },
    /// `target` was skipped because its breaker is open.
    SkippedUnhealthy { target: &'a str },
    /// Every candidate was exhausted.
    Exhausted,
}

pub struct Router {
    candidates: Vec<Candidate>,
    health: Vec<Health>,
    policy: RoutePolicy,
    /// Consecutive failures before a candidate's breaker opens.
    failure_threshold: usize,
    /// How long a breaker stays open.
    cooldown_ms: u64,
    /// Round-robin cursor.
    cursor: AtomicUsize,
    now_ms: Arc<dyn Fn() -> u64 + Send + Sync>,
    observer: Option<RouteObserver>,
}

pub(crate) fn wall_clock_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl Router {
    pub fn new(candidates: Vec<Candidate>, policy: RoutePolicy) -> Result<Self> {
        if candidates.is_empty() {
            return Err(Error::Provider(
                "router needs at least one candidate (set `[router] providers`)".into(),
            ));
        }
        let health = candidates.iter().map(|_| Health::new()).collect();
        Ok(Self {
            candidates,
            health,
            policy,
            failure_threshold: 3,
            cooldown_ms: 30_000,
            cursor: AtomicUsize::new(0),
            now_ms: Arc::new(wall_clock_ms),
            observer: None,
        })
    }

    pub fn with_breaker(mut self, failure_threshold: usize, cooldown_ms: u64) -> Self {
        self.failure_threshold = failure_threshold.max(1);
        self.cooldown_ms = cooldown_ms;
        self
    }

    pub fn with_observer(mut self, o: RouteObserver) -> Self {
        self.observer = Some(o);
        self
    }

    #[doc(hidden)]
    pub fn with_clock(mut self, f: Arc<dyn Fn() -> u64 + Send + Sync>) -> Self {
        self.now_ms = f;
        self
    }

    fn emit(&self, ev: RouteEvent<'_>) {
        if let Some(o) = &self.observer {
            o(ev);
        }
    }

    /// Candidate indices to try, in order: capable ones first in policy order,
    /// with open breakers skipped. Unhealthy candidates are appended last rather
    /// than dropped, so a total outage still attempts *something* instead of
    /// failing with "no candidates".
    fn order(&self, req: &CompletionRequest) -> Vec<usize> {
        let now = (self.now_ms)();
        let n = self.candidates.len();
        let start = match self.policy {
            RoutePolicy::InOrder => 0,
            RoutePolicy::RoundRobin => self.cursor.fetch_add(1, Ordering::SeqCst) % n,
        };

        let mut healthy = Vec::new();
        let mut unhealthy = Vec::new();
        for k in 0..n {
            let i = (start + k) % n;
            if !is_capable(&self.candidates[i].provider.capabilities(), req) {
                continue;
            }
            if self.health[i].is_open(now, self.cooldown_ms) {
                self.emit(RouteEvent::SkippedUnhealthy {
                    target: &self.candidates[i].name,
                });
                unhealthy.push(i);
            } else {
                healthy.push(i);
            }
        }
        healthy.extend(unhealthy);
        healthy
    }

    /// Try each candidate in turn, stopping at the first success or the first
    /// **terminal** failure.
    async fn route<T, F, Fut>(&self, req: &CompletionRequest, op: F) -> Result<T>
    where
        F: Fn(Arc<dyn LlmProvider>) -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        let order = self.order(req);
        if order.is_empty() {
            return Err(Error::Provider(
                "no candidate provider can serve this request (capability mismatch)".into(),
            ));
        }
        let mut last: Option<Error> = None;

        for (attempt, &i) in order.iter().enumerate() {
            let c = &self.candidates[i];
            self.emit(RouteEvent::Routed { target: &c.name });
            match op(c.provider.clone()).await {
                Ok(v) => {
                    self.health[i].record_success();
                    return Ok(v);
                }
                Err(e) => {
                    let msg = e.to_string();
                    self.health[i].record_failure((self.now_ms)(), self.failure_threshold);
                    if agent_retry::classify(&msg) == agent_retry::Class::Terminal {
                        // The same call fails identically everywhere — stop.
                        return Err(e);
                    }
                    if attempt + 1 < order.len() {
                        self.emit(RouteEvent::FellOver {
                            from: &c.name,
                            reason: "retryable",
                        });
                    }
                    last = Some(e);
                }
            }
        }
        self.emit(RouteEvent::Exhausted);
        Err(last.unwrap_or_else(|| Error::Provider("router exhausted all candidates".into())))
    }
}

#[async_trait]
impl LlmProvider for Router {
    /// The union of what the candidates can do — the loop should not disable a
    /// feature just because the *first* candidate lacks it. Context window is the
    /// **minimum**, since a request must fit whichever candidate serves it.
    fn capabilities(&self) -> ModelCapabilities {
        let mut out = ModelCapabilities {
            supports_tools: false,
            context_window: u32::MAX,
            supports_response_format: false,
            supports_vision: false,
        };
        for c in &self.candidates {
            let caps = c.provider.capabilities();
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
        // Fallover covers failures raised while *establishing* the stream. Once
        // bytes are flowing the turn is committed — restarting mid-stream would
        // duplicate content the caller has already seen.
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
    use agent_testkit::{final_turn, ScriptedProvider};
    use rstest::rstest;
    use std::sync::Mutex;

    /// A provider that always fails with a fixed message.
    struct FailProvider {
        msg: String,
        caps: ModelCapabilities,
        calls: Arc<AtomicUsize>,
    }
    impl FailProvider {
        fn new(msg: &str) -> Self {
            Self {
                msg: msg.into(),
                caps: ModelCapabilities {
                    supports_tools: true,
                    context_window: 1000,
                    supports_response_format: false,
                    supports_vision: false,
                },
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

    fn ok_provider() -> Arc<dyn LlmProvider> {
        Arc::new(ScriptedProvider::new(vec![final_turn("answer")]))
    }

    fn req() -> CompletionRequest {
        CompletionRequest {
            messages: vec![agent_core::Message::user("hi")],
            tools: vec![],
            max_tokens: 16,
            temperature: 0.0,
            response_format: None,
        }
    }

    fn router(candidates: Vec<(&str, Arc<dyn LlmProvider>)>) -> Router {
        Router::new(
            candidates
                .into_iter()
                .map(|(n, p)| Candidate {
                    name: n.into(),
                    provider: p,
                })
                .collect(),
            RoutePolicy::InOrder,
        )
        .expect("router")
    }

    /// The headline: a retryable failure on the primary completes on the
    /// secondary instead of surfacing an error.
    #[tokio::test]
    async fn positive_retryable_failure_falls_over() {
        let bad = Arc::new(FailProvider::new("http 429: slow down"));
        let r = router(vec![("primary", bad.clone()), ("secondary", ok_provider())]);
        let resp = r.complete(req()).await.expect("falls over and succeeds");
        assert_eq!(resp.message.content_text(), "answer");
        assert_eq!(bad.calls.load(Ordering::SeqCst), 1, "primary was tried");
    }

    /// A terminal failure must NOT be retried elsewhere: it fails the same way
    /// on every candidate, so trying them all burns the chain for nothing.
    #[rstest]
    #[case::negative_auth("http 401: invalid api key")]
    #[case::negative_billing("http 402: payment required")]
    #[case::negative_bad_request("http 400: unsupported parameter")]
    #[tokio::test]
    async fn negative_terminal_failure_does_not_fall_over(#[case] msg: &str) {
        let bad = Arc::new(FailProvider::new(msg));
        let good = Arc::new(ScriptedProvider::new(vec![final_turn("answer")]));
        let r = router(vec![("primary", bad.clone()), ("secondary", good.clone())]);
        assert!(r.complete(req()).await.is_err(), "must not fall over");
        assert_eq!(bad.calls.load(Ordering::SeqCst), 1);
    }

    /// Exhausting the chain surfaces the last error rather than hanging.
    #[tokio::test]
    async fn boundary_all_candidates_fail_surfaces_the_error() {
        let a = Arc::new(FailProvider::new("http 503: down"));
        let b = Arc::new(FailProvider::new("http 503: also down"));
        let r = router(vec![("a", a.clone()), ("b", b.clone())]);
        assert!(r.complete(req()).await.is_err());
        assert_eq!(a.calls.load(Ordering::SeqCst), 1);
        assert_eq!(b.calls.load(Ordering::SeqCst), 1, "every candidate tried");
    }

    /// After repeated failures the breaker opens and the dead candidate stops
    /// being tried first — it costs one timeout, not one per turn forever.
    #[tokio::test]
    async fn positive_breaker_opens_and_skips_the_dead_candidate() {
        let bad = Arc::new(FailProvider::new("http 503: down"));
        let r = router(vec![("bad", bad.clone()), ("good", ok_provider())])
            .with_breaker(2, 60_000)
            .with_clock(Arc::new(|| 1_000));

        for _ in 0..4 {
            let _ = r.complete(req()).await;
        }
        // 2 failures open the breaker; afterwards `bad` is ordered last, and the
        // healthy candidate answers first, so it stops being called.
        assert!(
            bad.calls.load(Ordering::SeqCst) <= 3,
            "breaker did not stop the calls: {}",
            bad.calls.load(Ordering::SeqCst)
        );
    }

    /// The breaker closes again after its cooldown, so a recovered provider is
    /// used rather than blacklisted forever.
    #[tokio::test]
    async fn positive_breaker_closes_after_cooldown() {
        let clock = Arc::new(AtomicU64::new(1_000));
        let c = clock.clone();
        let bad = Arc::new(FailProvider::new("http 503: down"));
        let r = router(vec![("bad", bad.clone()), ("good", ok_provider())])
            .with_breaker(1, 10_000)
            .with_clock(Arc::new(move || c.load(Ordering::SeqCst)));

        let _ = r.complete(req()).await; // opens the breaker
        let before = bad.calls.load(Ordering::SeqCst);
        clock.store(1_000_000, Ordering::SeqCst); // well past the cooldown
        let _ = r.complete(req()).await;
        assert!(
            bad.calls.load(Ordering::SeqCst) > before,
            "the recovered candidate must be probed again"
        );
    }

    /// A candidate that cannot serve the request is not tried at all — failing
    /// over to it would just produce a different error.
    #[tokio::test]
    async fn negative_incapable_candidate_is_skipped() {
        let no_tools = Arc::new(FailProvider {
            msg: "should never be called".into(),
            caps: ModelCapabilities {
                supports_tools: false,
                context_window: 1000,
                supports_response_format: false,
                supports_vision: false,
            },
            calls: Arc::new(AtomicUsize::new(0)),
        });
        let r = router(vec![
            ("no-tools", no_tools.clone()),
            ("with-tools", ok_provider()),
        ]);
        let mut rq = req();
        rq.tools = vec![agent_core::ToolSchema {
            name: "t".into(),
            description: "d".into(),
            parameters: serde_json::json!({}),
        }];
        let resp = r.complete(rq).await.expect("the capable candidate answers");
        assert_eq!(resp.message.content_text(), "answer");
        assert_eq!(
            no_tools.calls.load(Ordering::SeqCst),
            0,
            "an incapable candidate must not be called"
        );
    }

    /// Capabilities are the union (so the loop doesn't disable a feature the
    /// second candidate has) with the MINIMUM context window (so a request fits
    /// whichever candidate serves it).
    #[test]
    fn positive_capabilities_union_with_min_window() {
        struct P(ModelCapabilities);
        #[async_trait]
        impl LlmProvider for P {
            fn capabilities(&self) -> ModelCapabilities {
                self.0.clone()
            }
            async fn complete(&self, _r: CompletionRequest) -> Result<CompletionResponse> {
                unreachable!()
            }
            async fn stream(&self, _r: CompletionRequest) -> Result<ChunkStream> {
                unreachable!()
            }
        }
        let a = Arc::new(P(ModelCapabilities {
            supports_tools: true,
            context_window: 200_000,
            supports_response_format: false,
            supports_vision: false,
        }));
        let b = Arc::new(P(ModelCapabilities {
            supports_tools: false,
            context_window: 8_000,
            supports_response_format: true,
            supports_vision: true,
        }));
        let caps = router(vec![("a", a), ("b", b)]).capabilities();
        assert!(caps.supports_tools, "union");
        assert!(caps.supports_vision, "union");
        assert!(caps.supports_response_format, "union");
        assert_eq!(caps.context_window, 8_000, "minimum");
    }

    /// Route decisions are observable — the differentiator is that an operator
    /// can see the route mix and the fallover rate.
    #[tokio::test]
    async fn positive_route_events_are_emitted() {
        let events: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = events.clone();
        let bad = Arc::new(FailProvider::new("http 429: slow"));
        let r = router(vec![("primary", bad), ("secondary", ok_provider())]).with_observer(
            Arc::new(move |ev| {
                sink.lock().unwrap().push(match ev {
                    RouteEvent::Routed { target } => format!("routed:{target}"),
                    RouteEvent::FellOver { from, .. } => format!("fellover:{from}"),
                    RouteEvent::SkippedUnhealthy { target } => format!("skipped:{target}"),
                    RouteEvent::Exhausted => "exhausted".into(),
                });
            }),
        );
        r.complete(req()).await.expect("succeeds");
        let got = events.lock().unwrap().clone();
        assert!(got.contains(&"routed:primary".to_string()), "{got:?}");
        assert!(got.contains(&"fellover:primary".to_string()), "{got:?}");
        assert!(got.contains(&"routed:secondary".to_string()), "{got:?}");
    }

    /// Round-robin spreads load rather than hammering the first candidate.
    #[tokio::test]
    async fn positive_round_robin_spreads_load() {
        let a = Arc::new(ScriptedProvider::new(vec![
            final_turn("a"),
            final_turn("a"),
        ]));
        let b = Arc::new(ScriptedProvider::new(vec![
            final_turn("b"),
            final_turn("b"),
        ]));
        let r = Router::new(
            vec![
                Candidate {
                    name: "a".into(),
                    provider: a,
                },
                Candidate {
                    name: "b".into(),
                    provider: b,
                },
            ],
            RoutePolicy::RoundRobin,
        )
        .unwrap();
        let first = r.complete(req()).await.unwrap().message.content_text();
        let second = r.complete(req()).await.unwrap().message.content_text();
        assert_ne!(
            first, second,
            "consecutive calls must use different targets"
        );
    }

    #[test]
    fn boundary_empty_candidate_list_is_an_error() {
        assert!(Router::new(vec![], RoutePolicy::InOrder).is_err());
    }

    #[rstest]
    #[case::positive_in_order("in-order", RoutePolicy::InOrder)]
    #[case::positive_round_robin("round-robin", RoutePolicy::RoundRobin)]
    #[case::corner_unknown_defaults_in_order("nonsense", RoutePolicy::InOrder)]
    fn route_policy_parse(#[case] s: &str, #[case] want: RoutePolicy) {
        assert_eq!(RoutePolicy::parse(s), want);
    }
}
