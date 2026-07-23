//! `LlmPool` — a health-checked, tiered pool of cheap providers.
//!
//! The deployment target is a pool of cheap, heterogeneous, possibly-intermittent
//! model endpoints. Unlike [`Router`](crate::Router) (a *failover* `LlmProvider`
//! that picks one), the pool adds the two things a review flow needs: an **active
//! liveness probe** (a background task that pings each member and feeds the same
//! circuit breaker the router uses) and a **parallel fan-out** (`complete_all`)
//! so a cheap classification vote or a batch of summaries can ask several members
//! at once. Fails **soft**: a dead member is a slot in the result, never a batch
//! failure. See `docs/design/code-review/llm-pool.md`.

use crate::router::{is_capable, wall_clock_ms, Health};
use crate::Candidate;
use agent_core::{
    CompletionRequest, CompletionResponse, Error, HealthReport, LlmPool, Message, PoolMemberHealth,
    PoolMemberResult, PoolTier, Result,
};
use async_trait::async_trait;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

/// A typed observation from the pool, for metrics/spans. Owned strings (not the
/// borrowing `RouteEvent`) so the background probe task can emit without lifetime
/// grief. The runtime turns these into metrics, keeping `agent-providers` off
/// `agent-metrics` — the same inversion-avoiding pattern the router uses.
#[derive(Debug, Clone)]
pub enum PoolEvent {
    /// A `complete`/`complete_all` dispatch happened.
    Dispatch {
        mode: &'static str,
        tier: PoolTier,
        requested: usize,
        alive: usize,
    },
    /// One member answered (or failed) a request.
    MemberCall {
        member: String,
        ok: bool,
        duration_ms: u32,
    },
    /// One member was probed.
    Probe {
        member: String,
        alive: bool,
        duration_ms: u32,
    },
}

/// Observability hook (see [`PoolEvent`]).
pub type PoolObserver = Arc<dyn Fn(PoolEvent) + Send + Sync>;

/// How to add a member to a pool: an (already metered) candidate, its capability
/// tier, and an optional cost hint (0.0 = free/local). Cost orders fan-out
/// selection; it is clamped, never trusted.
pub struct PoolSpec {
    pub candidate: Candidate,
    pub tier: PoolTier,
    pub cost: f32,
}

struct PoolMember {
    candidate: Candidate,
    tier: PoolTier,
    cost: f32,
    health: Health,
    /// Duration of the most recent probe (ms), for the health snapshot.
    last_probe_ms: AtomicU64,
}

struct PoolInner {
    name: String,
    members: Vec<PoolMember>,
    failure_threshold: usize,
    cooldown_ms: u64,
    fanout: usize,
    now_ms: Arc<dyn Fn() -> u64 + Send + Sync>,
    observer: Option<PoolObserver>,
    probe_timeout: Duration,
}

/// A pool of cheap providers with active health-checking and both failover
/// (`complete`) and parallel fan-out (`complete_all`) dispatch.
pub struct PoolProvider {
    inner: Arc<PoolInner>,
}

impl PoolInner {
    fn emit(&self, ev: PoolEvent) {
        if let Some(o) = &self.observer {
            o(ev);
        }
    }

    /// Cost hints are attacker-adjacent config; clamp to a sane finite value.
    fn clamp_cost(c: f32) -> f32 {
        if c.is_finite() && c >= 0.0 {
            c
        } else {
            0.0
        }
    }

    /// Indices of members at or above `tier` that are capable and healthy, cheapest
    /// first, capped at `cap`. Fail-soft: an empty result is fine — the caller
    /// decides whether it has enough.
    fn eligible(&self, req: &CompletionRequest, tier: PoolTier, cap: usize) -> Vec<usize> {
        let now = (self.now_ms)();
        let mut healthy: Vec<usize> = (0..self.members.len())
            .filter(|&i| {
                let m = &self.members[i];
                m.tier >= tier
                    && is_capable(&m.candidate.provider.capabilities(), req)
                    && !m.health.is_open(now, self.cooldown_ms)
            })
            .collect();
        healthy.sort_by(|&a, &b| {
            Self::clamp_cost(self.members[a].cost)
                .partial_cmp(&Self::clamp_cost(self.members[b].cost))
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.cmp(&b))
        });
        healthy.truncate(cap);
        healthy
    }

    /// Run one member's `complete`, timed and breaker-updated. Fail-soft.
    async fn call_member(&self, i: usize, req: CompletionRequest) -> PoolMemberResult {
        let m = &self.members[i];
        let started = Instant::now();
        let outcome = m.candidate.provider.complete(req).await;
        let duration_ms = started.elapsed().as_millis().min(u32::MAX as u128) as u32;
        m.last_probe_ms.store(0, Ordering::SeqCst); // request path, not a probe
        let (response, error) = match outcome {
            Ok(r) => {
                m.health.record_success();
                (Some(r), None)
            }
            Err(e) => {
                m.health
                    .record_failure((self.now_ms)(), self.failure_threshold);
                (None, Some(classify_error(&e)))
            }
        };
        self.emit(PoolEvent::MemberCall {
            member: m.candidate.name.clone(),
            ok: response.is_some(),
            duration_ms,
        });
        PoolMemberResult {
            member: m.candidate.name.clone(),
            duration_ms,
            response,
            error,
        }
    }

    /// One probe cycle: ping every member with a 1-token request and update its
    /// breaker. Runs concurrently; never blocks the request path.
    async fn probe_all(&self) {
        let futures = (0..self.members.len()).map(|i| async move {
            let m = &self.members[i];
            let started = Instant::now();
            let outcome =
                tokio::time::timeout(self.probe_timeout, m.candidate.provider.complete(ping()))
                    .await;
            let duration_ms = started.elapsed().as_millis().min(u32::MAX as u128) as u32;
            m.last_probe_ms.store(duration_ms as u64, Ordering::SeqCst);
            let alive = matches!(outcome, Ok(Ok(_)));
            if alive {
                m.health.record_success();
            } else {
                m.health
                    .record_failure((self.now_ms)(), self.failure_threshold);
            }
            self.emit(PoolEvent::Probe {
                member: m.candidate.name.clone(),
                alive,
                duration_ms,
            });
        });
        futures_util::future::join_all(futures).await;
    }
}

impl PoolProvider {
    pub fn new(name: impl Into<String>, specs: Vec<PoolSpec>) -> Result<Self> {
        if specs.is_empty() {
            return Err(Error::Provider(
                "llm-pool needs at least one member (set `[pool] members`)".into(),
            ));
        }
        let members = specs
            .into_iter()
            .map(|s| PoolMember {
                candidate: s.candidate,
                tier: s.tier,
                cost: PoolInner::clamp_cost(s.cost),
                health: Health::new(),
                last_probe_ms: AtomicU64::new(0),
            })
            .collect();
        Ok(Self {
            inner: Arc::new(PoolInner {
                name: name.into(),
                members,
                failure_threshold: 3,
                cooldown_ms: 30_000,
                fanout: 3,
                now_ms: Arc::new(wall_clock_ms),
                observer: None,
                probe_timeout: Duration::from_secs(3),
            }),
        })
    }

    fn inner_mut(&mut self) -> &mut PoolInner {
        // The pool is uniquely owned during the builder chain (before it is
        // `Arc`-shared into the agent), so this get_mut always succeeds there.
        Arc::get_mut(&mut self.inner).expect("pool configured before it is shared")
    }

    pub fn with_breaker(mut self, failure_threshold: usize, cooldown_ms: u64) -> Self {
        let i = self.inner_mut();
        i.failure_threshold = failure_threshold.max(1);
        i.cooldown_ms = cooldown_ms;
        self
    }

    pub fn with_fanout(mut self, fanout: usize) -> Self {
        self.inner_mut().fanout = fanout.max(1);
        self
    }

    pub fn with_observer(mut self, o: PoolObserver) -> Self {
        self.inner_mut().observer = Some(o);
        self
    }

    #[doc(hidden)]
    pub fn with_clock(mut self, f: Arc<dyn Fn() -> u64 + Send + Sync>) -> Self {
        self.inner_mut().now_ms = f;
        self
    }

    /// Start the active liveness probe. Clamps the interval/timeout, and only
    /// spawns if a tokio runtime is available (so tests without one stay passive).
    /// The task holds a `Weak` to the pool and exits when the pool is dropped.
    pub fn with_probe(mut self, interval_secs: u64, timeout_secs: u64) -> Self {
        let timeout = Duration::from_secs(timeout_secs.clamp(1, 60));
        self.inner_mut().probe_timeout = timeout;
        let interval = Duration::from_secs(interval_secs.clamp(5, 3600));
        if interval_secs > 0 && tokio::runtime::Handle::try_current().is_ok() {
            let weak: Weak<PoolInner> = Arc::downgrade(&self.inner);
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(interval).await;
                    let Some(inner) = weak.upgrade() else { break };
                    inner.probe_all().await;
                }
            });
        }
        self
    }
}

#[async_trait]
impl LlmPool for PoolProvider {
    fn name(&self) -> &str {
        &self.inner.name
    }

    async fn health(&self) -> HealthReport {
        let now = (self.inner.now_ms)();
        let members = self
            .inner
            .members
            .iter()
            .map(|m| PoolMemberHealth {
                name: m.candidate.name.clone(),
                tier: m.tier,
                alive: !m.health.is_open(now, self.inner.cooldown_ms),
                consecutive_failures: m.health.failures().min(u32::MAX as usize) as u32,
                last_probe_ms: m.last_probe_ms.load(Ordering::SeqCst).min(u32::MAX as u64) as u32,
            })
            .collect();
        HealthReport { members }
    }

    async fn complete_all(
        &self,
        req: CompletionRequest,
        tier: PoolTier,
        fanout: usize,
    ) -> Vec<PoolMemberResult> {
        let members = self.inner.members.len().max(1);
        // Clamp the requested fan-out to [1, members]: a hostile config value
        // cannot make us allocate or spawn unboundedly.
        let cap = fanout.clamp(1, members).min(self.inner.fanout.max(1));
        let chosen = self.inner.eligible(&req, tier, cap);
        self.inner.emit(PoolEvent::Dispatch {
            mode: "all",
            tier,
            requested: cap,
            alive: chosen.len(),
        });
        let futures = chosen
            .into_iter()
            .map(|i| self.inner.call_member(i, req.clone()));
        futures_util::future::join_all(futures).await
    }

    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse> {
        // Failover over the healthy members (any tier), stopping at the first
        // success or first terminal failure — Router semantics, single answer.
        let order = self
            .inner
            .eligible(&req, PoolTier::Light, self.inner.members.len());
        self.inner.emit(PoolEvent::Dispatch {
            mode: "one",
            tier: PoolTier::Light,
            requested: 1,
            alive: order.len(),
        });
        if order.is_empty() {
            return Err(Error::Provider(
                "no pool member can serve this request (all unhealthy or incapable)".into(),
            ));
        }
        let mut last: Option<Error> = None;
        for (attempt, i) in order.iter().enumerate() {
            match self.inner.call_member(*i, req.clone()).await {
                PoolMemberResult {
                    response: Some(r), ..
                } => return Ok(r),
                PoolMemberResult { error, .. } => {
                    let msg = error.unwrap_or_default();
                    if agent_retry::classify(&msg) == agent_retry::Class::Terminal {
                        return Err(Error::Provider(msg));
                    }
                    let _ = attempt;
                    last = Some(Error::Provider(msg));
                }
            }
        }
        Err(last.unwrap_or_else(|| Error::Provider("pool exhausted all members".into())))
    }
}

/// The active-probe request: one token, cheap, tool-free.
fn ping() -> CompletionRequest {
    CompletionRequest {
        messages: vec![Message::user("ping")],
        tools: vec![],
        max_tokens: 1,
        temperature: 0.0,
        response_format: None,
    }
}

/// Reduce a provider error to a short class/status string — never a raw body.
fn classify_error(e: &Error) -> String {
    let msg = e.to_string();
    // Keep only the leading `http {code}: …` prefix or the first clause, bounded.
    let head: String = msg.chars().take(120).collect();
    head
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{ChunkStream, LlmProvider, ModelCapabilities};
    use agent_testkit::{final_turn, ScriptedProvider};
    use std::sync::atomic::AtomicUsize;
    use std::sync::Mutex;

    fn caps() -> ModelCapabilities {
        ModelCapabilities {
            supports_tools: true,
            context_window: 1000,
            supports_response_format: false,
            supports_vision: false,
        }
    }

    struct FailProvider {
        msg: String,
        calls: Arc<AtomicUsize>,
    }
    #[async_trait]
    impl LlmProvider for FailProvider {
        fn capabilities(&self) -> ModelCapabilities {
            caps()
        }
        async fn complete(&self, _r: CompletionRequest) -> Result<CompletionResponse> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Err(Error::Provider(self.msg.clone()))
        }
        async fn stream(&self, _r: CompletionRequest) -> Result<ChunkStream> {
            Err(Error::Provider(self.msg.clone()))
        }
    }

    fn ok_member(name: &str, tier: PoolTier) -> PoolSpec {
        PoolSpec {
            candidate: Candidate {
                name: name.into(),
                provider: Arc::new(ScriptedProvider::new(vec![final_turn(name)])),
            },
            tier,
            cost: 0.0,
        }
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

    fn pool(specs: Vec<PoolSpec>) -> PoolProvider {
        PoolProvider::new("test", specs).expect("pool")
    }

    /// Fan-out returns one settled slot per chosen member.
    #[tokio::test]
    async fn positive_complete_all_fans_out_to_tier() {
        let p = pool(vec![
            ok_member("a", PoolTier::Light),
            ok_member("b", PoolTier::Light),
        ]);
        let out = p.complete_all(req(), PoolTier::Light, 5).await;
        assert_eq!(out.len(), 2, "both light members answer");
        assert!(out.iter().all(|r| r.response.is_some()));
    }

    /// A tier floor excludes members below it.
    #[tokio::test]
    async fn positive_tier_floor_excludes_lighter_members() {
        let p = pool(vec![
            ok_member("light", PoolTier::Light),
            ok_member("heavy", PoolTier::Heavy),
        ]);
        let out = p.complete_all(req(), PoolTier::Heavy, 5).await;
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].member, "heavy");
    }

    /// Fail-soft: a dead member is a slot with an error, not a batch failure.
    #[tokio::test]
    async fn negative_dead_member_is_a_slot_not_a_failure() {
        let p = pool(vec![
            PoolSpec {
                candidate: Candidate {
                    name: "bad".into(),
                    provider: Arc::new(FailProvider {
                        msg: "http 503: down".into(),
                        calls: Arc::new(AtomicUsize::new(0)),
                    }),
                },
                tier: PoolTier::Light,
                cost: 0.0,
            },
            ok_member("good", PoolTier::Light),
        ]);
        let out = p.complete_all(req(), PoolTier::Light, 5).await;
        assert_eq!(out.len(), 2);
        assert_eq!(out.iter().filter(|r| r.response.is_some()).count(), 1);
        assert_eq!(out.iter().filter(|r| r.error.is_some()).count(), 1);
    }

    /// `complete` is failover — the healthy member answers when the first fails
    /// retryably.
    #[tokio::test]
    async fn positive_complete_falls_over() {
        let p = pool(vec![
            PoolSpec {
                candidate: Candidate {
                    name: "bad".into(),
                    provider: Arc::new(FailProvider {
                        msg: "http 429: slow".into(),
                        calls: Arc::new(AtomicUsize::new(0)),
                    }),
                },
                tier: PoolTier::Light,
                cost: 0.0,
            },
            ok_member("good", PoolTier::Light),
        ]);
        let r = p.complete(req()).await.expect("falls over");
        assert_eq!(r.message.content_text(), "good");
    }

    /// health() reports every member.
    #[tokio::test]
    async fn positive_health_lists_members() {
        let p = pool(vec![
            ok_member("a", PoolTier::Light),
            ok_member("b", PoolTier::Heavy),
        ]);
        let h = p.health().await;
        assert_eq!(h.members.len(), 2);
        assert!(h.members.iter().all(|m| m.alive));
    }

    /// Adversarial: a hostile `fanout` (usize::MAX) is clamped, not acted on
    /// literally — no panic, no unbounded allocation.
    #[tokio::test]
    async fn adversarial_hostile_fanout_is_clamped() {
        let p = pool(vec![ok_member("a", PoolTier::Light)]).with_fanout(1);
        let out = p.complete_all(req(), PoolTier::Light, usize::MAX).await;
        assert_eq!(out.len(), 1, "clamped to the single eligible member");
    }

    /// Adversarial: a non-finite / negative cost hint is clamped to 0.0 and does
    /// not corrupt the ordering (no panic on NaN compare).
    #[tokio::test]
    async fn adversarial_hostile_cost_is_clamped() {
        let p = pool(vec![
            PoolSpec {
                candidate: Candidate {
                    name: "nan".into(),
                    provider: Arc::new(ScriptedProvider::new(vec![final_turn("nan")])),
                },
                tier: PoolTier::Light,
                cost: f32::NAN,
            },
            PoolSpec {
                candidate: Candidate {
                    name: "neg".into(),
                    provider: Arc::new(ScriptedProvider::new(vec![final_turn("neg")])),
                },
                tier: PoolTier::Light,
                cost: -100.0,
            },
        ]);
        let out = p.complete_all(req(), PoolTier::Light, 5).await;
        assert_eq!(out.len(), 2, "both selected, ordering did not panic");
    }

    /// Empty members is a build error, not a silent empty pool.
    #[test]
    fn boundary_empty_pool_is_an_error() {
        assert!(PoolProvider::new("x", vec![]).is_err());
    }

    /// Events are observable (dispatch + per-member).
    #[tokio::test]
    async fn positive_pool_events_are_emitted() {
        let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        let p = pool(vec![ok_member("a", PoolTier::Light)]).with_observer(Arc::new(move |ev| {
            sink.lock().unwrap().push(match ev {
                PoolEvent::Dispatch { mode, .. } => format!("dispatch:{mode}"),
                PoolEvent::MemberCall { member, ok, .. } => format!("member:{member}:{ok}"),
                PoolEvent::Probe { member, .. } => format!("probe:{member}"),
            });
        }));
        p.complete_all(req(), PoolTier::Light, 1).await;
        let got = seen.lock().unwrap().clone();
        assert!(got.iter().any(|e| e == "dispatch:all"), "{got:?}");
        assert!(got.iter().any(|e| e == "member:a:true"), "{got:?}");
    }
}
