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
    PoolMemberResult, PoolMemberState, PoolTier, Result,
};
use async_trait::async_trait;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

/// How the pool orders the eligible members before dispatch (GPU pool 01). A
/// **floor** filter (tier / capability / health) runs first; the policy only
/// orders the survivors. Mirrors the router's `RoutePolicy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PoolPolicy {
    /// Cheapest configured `cost` first, tie-break by index — the historical
    /// behaviour and the back-compat default.
    #[default]
    Cost,
    /// Rotate a cursor across the survivors — spread without a load signal.
    RoundRobin,
    /// Fewest in-flight requests first (tie-break cost, then index) — the
    /// capacity-aware default for a heterogeneous GPU pool.
    LeastLoaded,
    /// Bias the pick by configured `weight` (higher ⇒ chosen more often).
    Weighted,
}

impl PoolPolicy {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "least-loaded" | "leastloaded" => PoolPolicy::LeastLoaded,
            "round-robin" | "roundrobin" => PoolPolicy::RoundRobin,
            "weighted" => PoolPolicy::Weighted,
            _ => PoolPolicy::Cost,
        }
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            PoolPolicy::Cost => "cost",
            PoolPolicy::RoundRobin => "round-robin",
            PoolPolicy::LeastLoaded => "least-loaded",
            PoolPolicy::Weighted => "weighted",
        }
    }
}

/// What the pool does when every eligible member is at its concurrency cap
/// (GPU pool 02). Both are bounded and fail-soft — never an unbounded queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Saturation {
    /// Shed immediately: a fan-out returns fewer/zero slots; a single `complete`
    /// returns a `saturated` error the caller reads. The safe default.
    #[default]
    Shed,
    /// Wait a **bounded** time for a permit to free, then re-select; on timeout,
    /// fall through to `shed`.
    Wait,
}

impl Saturation {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "wait" => Saturation::Wait,
            _ => Saturation::Shed,
        }
    }
}

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
        policy: &'static str,
        requested: usize,
        alive: usize,
    },
    /// One member answered (or failed) a request.
    MemberCall {
        member: String,
        ok: bool,
        duration_ms: u32,
    },
    /// A member's in-flight count changed (a dispatch started or finished). Drives
    /// the per-member load gauge (GPU pool 01) and the saturation gauge (02).
    MemberState {
        member: String,
        in_flight: u32,
        saturated: bool,
    },
    /// A dispatch shed because every eligible member was at its concurrency cap
    /// (GPU pool 02).
    SaturationShed { mode: &'static str },
    /// A member's smoothed latency + graded state changed (GPU pool 03).
    MemberGraded {
        member: String,
        state: &'static str,
        latency_ms_ewma: u32,
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

/// Decrements a member's in-flight counter on drop, so the count is released on
/// every path — including an early return or a panic in the provider call.
struct InFlightGuard<'a>(&'a AtomicUsize);
impl Drop for InFlightGuard<'_> {
    fn drop(&mut self) {
        // Saturating: never wrap below zero even under a spurious double-drop.
        let prev = self.0.load(Ordering::Acquire);
        if prev > 0 {
            self.0.fetch_sub(1, Ordering::AcqRel);
        }
    }
}

/// How to add a member to a pool: an (already metered) candidate, its capability
/// tier, and an optional cost hint (0.0 = free/local). Cost orders fan-out
/// selection; it is clamped, never trusted.
pub struct PoolSpec {
    pub candidate: Candidate,
    pub tier: PoolTier,
    pub cost: f32,
    /// Selection weight for the `weighted` policy (higher ⇒ more often). Clamped,
    /// never trusted; a non-positive/non-finite value falls back to 1.0.
    pub weight: f32,
    /// Per-target concurrency cap; `0` ⇒ unbounded (GPU pool 02). A member at its
    /// cap is skipped in selection.
    pub max_concurrency: usize,
}

struct PoolMember {
    candidate: Candidate,
    tier: PoolTier,
    cost: f32,
    weight: f32,
    /// Concurrency cap; `0` ⇒ unbounded.
    max_concurrency: usize,
    health: Health,
    /// Requests currently in flight to this member — the live load signal.
    in_flight: AtomicUsize,
    /// Duration of the most recent probe (ms), for the health snapshot.
    last_probe_ms: AtomicU64,
    /// Smoothed request+probe latency (ms, EWMA), `0` until the first sample — the
    /// signal that grades a member `degraded` (GPU pool 03).
    latency_ewma_ms: AtomicU64,
}

struct PoolInner {
    name: String,
    members: Vec<PoolMember>,
    policy: PoolPolicy,
    /// Rotating cursor for the round-robin policy.
    rr_cursor: AtomicUsize,
    /// What to do when every eligible member is saturated (GPU pool 02).
    saturation: Saturation,
    /// Bounded wait budget (ms) for the `wait` saturation policy; clamped.
    saturation_wait_ms: u64,
    /// EWMA smoothing factor in (0,1] for latency grading (GPU pool 03).
    latency_alpha: f64,
    /// Latency-EWMA (ms) above which a member is `degraded`; `0` disables grading.
    degraded_threshold_ms: u64,
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

    /// Weights are attacker-adjacent config; a non-finite or non-positive value
    /// falls back to a neutral 1.0 so the weighted policy can't divide by zero or
    /// be steered by a hostile `inf`/`NaN`.
    fn clamp_weight(w: f32) -> f32 {
        if w.is_finite() && w > 0.0 {
            w.min(1_000_000.0)
        } else {
            1.0
        }
    }

    /// A member has room for another request (GPU pool 02). `max_concurrency == 0`
    /// is unbounded.
    fn has_capacity(&self, m: &PoolMember) -> bool {
        m.max_concurrency == 0 || m.in_flight.load(Ordering::Acquire) < m.max_concurrency
    }

    /// A member is alive + capable + at/above `tier` but **at its cap** — the only
    /// reason it was excluded is saturation, so waiting could admit it.
    fn saturated_but_eligible(&self, req: &CompletionRequest, tier: PoolTier) -> bool {
        let now = (self.now_ms)();
        self.members.iter().any(|m| {
            m.tier >= tier
                && is_capable(&m.candidate.provider.capabilities(), req)
                && !m.health.is_open(now, self.cooldown_ms)
                && !self.has_capacity(m)
        })
    }

    /// Fold a latency `sample_ms` into a member's EWMA (GPU pool 03). The first
    /// sample seeds it; thereafter `ewma = α·sample + (1-α)·ewma`. Saturating +
    /// bounded, so a hostile `u64::MAX` sample can't overflow the store.
    fn record_latency(&self, m: &PoolMember, sample_ms: u64) {
        let sample = sample_ms.min(u32::MAX as u64);
        let cur = m.latency_ewma_ms.load(Ordering::Acquire);
        let next = if cur == 0 {
            sample
        } else {
            let a = self.latency_alpha;
            ((a * sample as f64) + ((1.0 - a) * cur as f64)) as u64
        };
        m.latency_ewma_ms
            .store(next.min(u32::MAX as u64), Ordering::Release);
    }

    /// A member's grade for ordering (GPU pool 03): `0` healthy, `1` degraded
    /// (alive but its latency EWMA is over the threshold). Dead members are already
    /// filtered out by `eligible`. `degraded_threshold_ms == 0` disables grading.
    fn grade(&self, i: usize) -> u8 {
        if self.degraded_threshold_ms == 0 {
            return 0;
        }
        let ewma = self.members[i].latency_ewma_ms.load(Ordering::Acquire);
        u8::from(ewma > self.degraded_threshold_ms)
    }

    /// The member's current graded state for the health snapshot.
    fn member_state_grade(&self, m: &PoolMember, alive: bool) -> PoolMemberState {
        if !alive {
            PoolMemberState::Dead
        } else if self.degraded_threshold_ms > 0
            && m.latency_ewma_ms.load(Ordering::Acquire) > self.degraded_threshold_ms
        {
            PoolMemberState::Degraded
        } else {
            PoolMemberState::Healthy
        }
    }

    /// A `MemberGraded` event carrying the member's smoothed latency + state.
    fn member_graded(&self, m: &PoolMember) -> PoolEvent {
        let alive = !m.health.is_open((self.now_ms)(), self.cooldown_ms);
        PoolEvent::MemberGraded {
            member: m.candidate.name.clone(),
            state: self.member_state_grade(m, alive).as_str(),
            latency_ms_ewma: m
                .latency_ewma_ms
                .load(Ordering::Acquire)
                .min(u32::MAX as u64) as u32,
        }
    }

    /// A `MemberState` event carrying the member's live load + saturation.
    fn member_state(&self, m: &PoolMember) -> PoolEvent {
        PoolEvent::MemberState {
            member: m.candidate.name.clone(),
            in_flight: m.in_flight.load(Ordering::Acquire).min(u32::MAX as usize) as u32,
            saturated: !self.has_capacity(m),
        }
    }

    /// Indices of members at or above `tier` that are capable, healthy, and **have
    /// spare capacity** (GPU pool 02), **ordered by the selection policy** and capped
    /// at `cap`. Fail-soft: an empty result is fine — the caller decides whether it
    /// has enough. Runs on every call, so it is kept allocation-light (guarded by the
    /// `pool_select` bench).
    fn eligible(&self, req: &CompletionRequest, tier: PoolTier, cap: usize) -> Vec<usize> {
        let now = (self.now_ms)();
        let mut healthy: Vec<usize> = (0..self.members.len())
            .filter(|&i| {
                let m = &self.members[i];
                m.tier >= tier
                    && is_capable(&m.candidate.provider.capabilities(), req)
                    && !m.health.is_open(now, self.cooldown_ms)
                    && self.has_capacity(m)
            })
            .collect();
        self.order(&mut healthy);
        healthy.truncate(cap);
        healthy
    }

    /// Order the filtered survivors in place per the policy. Cost tie-break is
    /// shared by cost/least-loaded so ordering is always deterministic (matters for
    /// the bench and reproducible tests); weighted is the only randomised policy and
    /// it varies by a counter, never a clock, so it stays resume-safe.
    fn order(&self, ids: &mut [usize]) {
        let cost = |i: usize| Self::clamp_cost(self.members[i].cost);
        match self.policy {
            PoolPolicy::Cost => ids.sort_by(|&a, &b| {
                cost(a)
                    .partial_cmp(&cost(b))
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(a.cmp(&b))
            }),
            PoolPolicy::LeastLoaded => {
                let load = |i: usize| self.members[i].in_flight.load(Ordering::Acquire);
                ids.sort_by(|&a, &b| {
                    load(a)
                        .cmp(&load(b))
                        .then(
                            cost(a)
                                .partial_cmp(&cost(b))
                                .unwrap_or(std::cmp::Ordering::Equal),
                        )
                        .then(a.cmp(&b))
                });
            }
            PoolPolicy::RoundRobin => {
                ids.sort_unstable();
                if !ids.is_empty() {
                    // Rotate the (index-sorted) survivors by a per-dispatch cursor.
                    let start = self.rr_cursor.fetch_add(1, Ordering::Relaxed) % ids.len();
                    ids.rotate_left(start);
                }
            }
            PoolPolicy::Weighted => {
                // A weighted shuffle: order by `-ln(u)/weight` (the standard
                // weighted-random key), where `u` is a counter-derived pseudo-random
                // in (0,1] — deterministic per process, no clock, higher weight ⇒
                // earlier on average.
                ids.sort_unstable();
                let tick = self.rr_cursor.fetch_add(1, Ordering::Relaxed);
                let key = |i: usize| {
                    let w = Self::clamp_weight(self.members[i].weight);
                    // Cheap counter hash → (0,1]; varies per member and per dispatch.
                    let h = ((i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ tick as u64)
                        .wrapping_mul(0xD1B5_4A32_D192_ED03);
                    let u = ((h >> 11) as f64 / (1u64 << 53) as f64).max(1e-12);
                    (-u.ln()) / w as f64
                };
                ids.sort_by(|&a, &b| {
                    key(a)
                        .partial_cmp(&key(b))
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then(a.cmp(&b))
                });
            }
        }
        // GPU pool 03: prefer healthy over degraded (alive-but-slow) members. A
        // *stable* sort by grade preserves the policy order within each grade, so
        // this composes with every policy uniformly. Dead members are already gone.
        if self.degraded_threshold_ms > 0 {
            ids.sort_by_key(|&i| self.grade(i));
        }
    }

    /// Run one member's `complete`, timed and breaker-updated. Fail-soft. The
    /// in-flight count is raised for the duration via an RAII guard so it is
    /// released on *every* path — success, error, or a panic in the provider — and
    /// can never leak load that would exile a healthy member from selection.
    async fn call_member(&self, i: usize, req: CompletionRequest) -> PoolMemberResult {
        let m = &self.members[i];
        let _n_in = m.in_flight.fetch_add(1, Ordering::AcqRel) + 1;
        let _guard = InFlightGuard(&m.in_flight);
        self.emit(self.member_state(m));
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
        // Fold this call's latency into the EWMA and report the new grade (03).
        self.record_latency(m, duration_ms as u64);
        self.emit(self.member_graded(m));
        // Release the in-flight slot and report the settled count for the gauge.
        drop(_guard);
        self.emit(self.member_state(m));
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
            // A probe is also a latency sample (03) — fold it in + report the grade.
            self.record_latency(m, duration_ms as u64);
            self.emit(self.member_graded(m));
        });
        futures_util::future::join_all(futures).await;
    }

    /// Poll for spare capacity up to the bounded wait budget (GPU pool 02, `wait`
    /// saturation policy). Returns the first non-empty eligible order, or empty on
    /// timeout. The tick count is hard-capped, so this can never wait unboundedly.
    async fn wait_for_capacity(&self, req: &CompletionRequest) -> Vec<usize> {
        const TICK_MS: u64 = 25;
        let ticks = (self.saturation_wait_ms / TICK_MS).min(1_200); // ≤ 30s
        for _ in 0..ticks {
            tokio::time::sleep(Duration::from_millis(TICK_MS)).await;
            let order = self.eligible(req, PoolTier::Light, self.members.len());
            if !order.is_empty() {
                return order;
            }
        }
        Vec::new()
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
                weight: PoolInner::clamp_weight(s.weight),
                max_concurrency: s.max_concurrency,
                health: Health::new(),
                in_flight: AtomicUsize::new(0),
                last_probe_ms: AtomicU64::new(0),
                latency_ewma_ms: AtomicU64::new(0),
            })
            .collect();
        Ok(Self {
            inner: Arc::new(PoolInner {
                name: name.into(),
                members,
                policy: PoolPolicy::default(),
                rr_cursor: AtomicUsize::new(0),
                saturation: Saturation::default(),
                saturation_wait_ms: 0,
                latency_alpha: 0.3,
                degraded_threshold_ms: 0,
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

    /// Set the selection policy (default [`PoolPolicy::Cost`], the historical
    /// behaviour).
    pub fn with_policy(mut self, policy: PoolPolicy) -> Self {
        self.inner_mut().policy = policy;
        self
    }

    /// Set the saturation policy + bounded wait budget (GPU pool 02). The wait is
    /// clamped to a sane ceiling so a hostile config can't stall the loop.
    pub fn with_saturation(mut self, saturation: Saturation, wait_ms: u64) -> Self {
        let i = self.inner_mut();
        i.saturation = saturation;
        i.saturation_wait_ms = wait_ms.min(30_000);
        self
    }

    /// Enable latency-graded health (GPU pool 03): the EWMA smoothing factor
    /// (clamped to (0,1]) and the ms threshold above which an alive member is
    /// `degraded` and sorted after the healthy ones. A `0` threshold disables it.
    pub fn with_health_grading(mut self, alpha: f64, degraded_threshold_ms: u64) -> Self {
        let i = self.inner_mut();
        i.latency_alpha = if alpha.is_finite() && alpha > 0.0 {
            alpha.min(1.0)
        } else {
            0.3
        };
        i.degraded_threshold_ms = degraded_threshold_ms;
        self
    }

    /// Seed a member's in-flight count (for benches/tests of the selection order).
    #[doc(hidden)]
    pub fn bench_set_in_flight(&self, i: usize, n: usize) {
        if let Some(m) = self.inner.members.get(i) {
            m.in_flight.store(n, Ordering::SeqCst);
        }
    }

    /// Seed a member's latency EWMA (for tests/benches of the graded ordering).
    #[doc(hidden)]
    pub fn bench_set_latency(&self, i: usize, ms: u64) {
        if let Some(m) = self.inner.members.get(i) {
            m.latency_ewma_ms.store(ms, Ordering::SeqCst);
        }
    }

    /// Benchmark hook: the per-call selection (`eligible`, ordered by policy) is the
    /// hot path the `pool_select` Ir ceiling guards.
    #[doc(hidden)]
    pub fn bench_select(&self, req: &CompletionRequest, tier: PoolTier, cap: usize) -> Vec<usize> {
        self.inner.eligible(req, tier, cap)
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
            .map(|m| {
                let alive = !m.health.is_open(now, self.inner.cooldown_ms);
                PoolMemberHealth {
                    name: m.candidate.name.clone(),
                    tier: m.tier,
                    alive,
                    consecutive_failures: m.health.failures().min(u32::MAX as usize) as u32,
                    last_probe_ms: m.last_probe_ms.load(Ordering::SeqCst).min(u32::MAX as u64)
                        as u32,
                    in_flight: m.in_flight.load(Ordering::Acquire).min(u32::MAX as usize) as u32,
                    weight: m.weight,
                    max_concurrency: m.max_concurrency.min(u32::MAX as usize) as u32,
                    saturated: !self.inner.has_capacity(m),
                    state: self.inner.member_state_grade(m, alive),
                    latency_ms_ewma: m
                        .latency_ewma_ms
                        .load(Ordering::Acquire)
                        .min(u32::MAX as u64) as u32,
                }
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
            policy: self.inner.policy.as_str(),
            requested: cap,
            alive: chosen.len(),
        });
        // Fan-out is fail-soft: a saturated member is simply not dispatched to (the
        // set shrinks). If nothing was admitted *because* everything is saturated,
        // record a shed — the caller (e.g. the mode vote) degrades to fewer voters.
        if chosen.is_empty() && self.inner.saturated_but_eligible(&req, tier) {
            self.inner.emit(PoolEvent::SaturationShed { mode: "all" });
        }
        let futures = chosen
            .into_iter()
            .map(|i| self.inner.call_member(i, req.clone()));
        futures_util::future::join_all(futures).await
    }

    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse> {
        // Failover over the healthy members (any tier), stopping at the first
        // success or first terminal failure — Router semantics, single answer.
        let mut order = self
            .inner
            .eligible(&req, PoolTier::Light, self.inner.members.len());
        // Saturation backpressure (GPU pool 02): in `wait` mode, poll a *bounded*
        // time for a permit to free before giving up. Never an unbounded queue.
        if order.is_empty()
            && self.inner.saturation == Saturation::Wait
            && self.inner.saturated_but_eligible(&req, PoolTier::Light)
        {
            order = self.inner.wait_for_capacity(&req).await;
        }
        self.inner.emit(PoolEvent::Dispatch {
            mode: "one",
            tier: PoolTier::Light,
            policy: self.inner.policy.as_str(),
            requested: 1,
            alive: order.len(),
        });
        if order.is_empty() {
            // Distinguish "all saturated" (a transient QoS shed) from "all dead".
            if self.inner.saturated_but_eligible(&req, PoolTier::Light) {
                self.inner.emit(PoolEvent::SaturationShed { mode: "one" });
                return Err(Error::Provider(
                    "pool saturated: all eligible members at capacity".into(),
                ));
            }
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
        route: None,
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
            weight: 1.0,
            max_concurrency: 0,
        }
    }

    fn req() -> CompletionRequest {
        CompletionRequest {
            messages: vec![Message::user("hi")],
            tools: vec![],
            max_tokens: 16,
            temperature: 0.0,
            response_format: None,
            route: None,
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
                weight: 1.0,
                max_concurrency: 0,
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
                weight: 1.0,
                max_concurrency: 0,
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
                weight: 1.0,
                max_concurrency: 0,
            },
            PoolSpec {
                candidate: Candidate {
                    name: "neg".into(),
                    provider: Arc::new(ScriptedProvider::new(vec![final_turn("neg")])),
                },
                tier: PoolTier::Light,
                cost: -100.0,
                weight: 1.0,
                max_concurrency: 0,
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
                PoolEvent::Dispatch { mode, policy, .. } => format!("dispatch:{mode}:{policy}"),
                PoolEvent::MemberCall { member, ok, .. } => format!("member:{member}:{ok}"),
                PoolEvent::MemberState {
                    member, in_flight, ..
                } => {
                    format!("state:{member}:{in_flight}")
                }
                PoolEvent::SaturationShed { mode } => format!("shed:{mode}"),
                PoolEvent::MemberGraded { member, state, .. } => format!("grade:{member}:{state}"),
                PoolEvent::Probe { member, .. } => format!("probe:{member}"),
            });
        }));
        p.complete_all(req(), PoolTier::Light, 1).await;
        let got = seen.lock().unwrap().clone();
        assert!(got.iter().any(|e| e == "dispatch:all:cost"), "{got:?}");
        assert!(got.iter().any(|e| e == "member:a:true"), "{got:?}");
        // The in-flight gauge is driven up (1) then back down (0) around the call.
        assert!(got.iter().any(|e| e == "state:a:1"), "{got:?}");
        assert!(got.iter().any(|e| e == "state:a:0"), "{got:?}");
    }

    // --- policy ordering ---------------------------------------------------

    /// Build a pool whose members have set in-flight counts, for order tests.
    fn loaded_pool(policy: PoolPolicy, loads: &[(&str, usize)]) -> PoolProvider {
        let specs = loads
            .iter()
            .map(|(name, _)| ok_member(name, PoolTier::Light))
            .collect();
        let p = PoolProvider::new("test", specs)
            .expect("pool")
            .with_policy(policy);
        // Seed in-flight via the internal counter (uniquely owned here).
        for (i, (_, load)) in loads.iter().enumerate() {
            p.inner.members[i].in_flight.store(*load, Ordering::SeqCst);
        }
        p
    }

    /// least-loaded picks the idlest member first.
    #[test]
    fn positive_least_loaded_prefers_idle() {
        let p = loaded_pool(
            PoolPolicy::LeastLoaded,
            &[("busy", 5), ("idle", 0), ("mid", 2)],
        );
        let order = p.inner.eligible(&req(), PoolTier::Light, 3);
        assert_eq!(order, vec![1, 2, 0], "idle < mid < busy");
    }

    /// cost policy (default) keeps index order at equal cost — back-compat.
    #[test]
    fn positive_cost_policy_is_index_order() {
        let p = loaded_pool(PoolPolicy::Cost, &[("a", 9), ("b", 0), ("c", 3)]);
        // Ignores load entirely; equal cost ⇒ index order.
        assert_eq!(p.inner.eligible(&req(), PoolTier::Light, 3), vec![0, 1, 2]);
    }

    /// round-robin rotates the primary across dispatches.
    #[test]
    fn positive_round_robin_rotates() {
        let p = loaded_pool(PoolPolicy::RoundRobin, &[("a", 0), ("b", 0), ("c", 0)]);
        let first = p.inner.eligible(&req(), PoolTier::Light, 1)[0];
        let second = p.inner.eligible(&req(), PoolTier::Light, 1)[0];
        let third = p.inner.eligible(&req(), PoolTier::Light, 1)[0];
        assert_ne!(first, second, "primary rotates");
        assert_ne!(second, third);
    }

    /// corner: all-equal load under least-loaded falls back to deterministic order.
    #[test]
    fn corner_least_loaded_all_equal_is_deterministic() {
        let p = loaded_pool(PoolPolicy::LeastLoaded, &[("a", 2), ("b", 2), ("c", 2)]);
        assert_eq!(p.inner.eligible(&req(), PoolTier::Light, 3), vec![0, 1, 2]);
    }

    /// adversarial: a hostile weight (NaN / negative / inf) is clamped, and the
    /// weighted policy neither panics nor drops a member.
    #[tokio::test]
    async fn adversarial_hostile_weight_is_clamped() {
        let mk = |name: &str, w: f32| PoolSpec {
            candidate: Candidate {
                name: name.into(),
                provider: Arc::new(ScriptedProvider::new(vec![final_turn(name)])),
            },
            tier: PoolTier::Light,
            cost: 0.0,
            weight: w,
            max_concurrency: 0,
        };
        let p = PoolProvider::new(
            "test",
            vec![
                mk("nan", f32::NAN),
                mk("neg", -3.0),
                mk("inf", f32::INFINITY),
            ],
        )
        .expect("pool")
        .with_policy(PoolPolicy::Weighted);
        let order = p.inner.eligible(&req(), PoolTier::Light, 3);
        assert_eq!(order.len(), 3, "all members retained, no panic");
        // Clamped to the neutral weight, so health reports a sane finite value.
        let h = p.health().await;
        assert!(h
            .members
            .iter()
            .all(|m| m.weight.is_finite() && m.weight > 0.0));
    }

    /// adversarial: the in-flight guard releases even when the provider panics —
    /// no leaked load that would exile a healthy member.
    #[tokio::test]
    async fn adversarial_in_flight_released_on_panic() {
        struct PanicProvider;
        #[async_trait]
        impl LlmProvider for PanicProvider {
            fn capabilities(&self) -> ModelCapabilities {
                caps()
            }
            async fn complete(&self, _r: CompletionRequest) -> Result<CompletionResponse> {
                panic!("boom")
            }
        }
        let p = PoolProvider::new(
            "test",
            vec![PoolSpec {
                candidate: Candidate {
                    name: "panicky".into(),
                    provider: Arc::new(PanicProvider),
                },
                tier: PoolTier::Light,
                cost: 0.0,
                weight: 1.0,
                max_concurrency: 0,
            }],
        )
        .expect("pool");
        // A panic in the member call unwinds through the guard's Drop.
        use futures_util::future::FutureExt;
        let res = std::panic::AssertUnwindSafe(p.complete_all(req(), PoolTier::Light, 1))
            .catch_unwind()
            .await;
        assert!(res.is_err(), "the call panicked");
        assert_eq!(
            p.inner.members[0].in_flight.load(Ordering::SeqCst),
            0,
            "in-flight released despite the panic"
        );
    }

    // --- capacity / backpressure (GPU pool 02) -----------------------------

    fn capped_member(name: &str, cap: usize) -> PoolSpec {
        PoolSpec {
            candidate: Candidate {
                name: name.into(),
                provider: Arc::new(ScriptedProvider::new(vec![final_turn(name)])),
            },
            tier: PoolTier::Light,
            cost: 0.0,
            weight: 1.0,
            max_concurrency: cap,
        }
    }

    /// positive: a member at its cap is skipped; one with room is selected.
    #[test]
    fn positive_saturated_member_is_skipped() {
        let p = PoolProvider::new(
            "t",
            vec![capped_member("busy", 1), capped_member("free", 1)],
        )
        .expect("pool");
        p.bench_set_in_flight(0, 1); // `busy` at cap
        let order = p.bench_select(&req(), PoolTier::Light, 5);
        assert_eq!(order, vec![1], "only the member with spare capacity");
    }

    /// negative: an unbounded member (cap 0) is never skipped, however loaded.
    #[test]
    fn negative_unbounded_member_never_saturated() {
        let p = PoolProvider::new("t", vec![capped_member("unbounded", 0)]).expect("pool");
        p.bench_set_in_flight(0, 10_000);
        assert_eq!(p.bench_select(&req(), PoolTier::Light, 5), vec![0]);
    }

    /// boundary: exactly at cap is skipped; one below is admitted.
    #[test]
    fn boundary_at_cap_vs_below() {
        let p = PoolProvider::new("t", vec![capped_member("m", 2)]).expect("pool");
        p.bench_set_in_flight(0, 1); // below cap → admitted
        assert_eq!(p.bench_select(&req(), PoolTier::Light, 5), vec![0]);
        p.bench_set_in_flight(0, 2); // at cap → skipped
        assert!(p.bench_select(&req(), PoolTier::Light, 5).is_empty());
    }

    /// boundary: all members saturated → `complete` sheds a `saturated` error
    /// (never a hang), and `complete_all` returns empty + emits a shed event.
    #[tokio::test]
    async fn boundary_all_saturated_sheds() {
        let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        let p = PoolProvider::new("t", vec![capped_member("a", 1), capped_member("b", 1)])
            .expect("pool")
            .with_observer(Arc::new(move |ev| {
                if let PoolEvent::SaturationShed { mode } = ev {
                    sink.lock().unwrap().push(format!("shed:{mode}"));
                }
            }));
        p.bench_set_in_flight(0, 1);
        p.bench_set_in_flight(1, 1);

        let out = p.complete_all(req(), PoolTier::Light, 5).await;
        assert!(out.is_empty(), "all saturated → nothing dispatched");
        let err = p.complete(req()).await.unwrap_err().to_string();
        assert!(err.contains("saturated"), "got: {err}");
        let got = seen.lock().unwrap().clone();
        assert!(got.iter().any(|e| e == "shed:all"), "{got:?}");
        assert!(got.iter().any(|e| e == "shed:one"), "{got:?}");
    }

    /// adversarial: `wait` mode with all members stuck-saturated must return within
    /// the bounded budget — never hang or unbounded-queue.
    #[tokio::test]
    async fn adversarial_saturation_wait_is_bounded() {
        let p = PoolProvider::new("t", vec![capped_member("a", 1)])
            .expect("pool")
            .with_saturation(Saturation::Wait, 40); // ~1-2 ticks
        p.bench_set_in_flight(0, 1); // stuck saturated (never frees)
        let res = tokio::time::timeout(std::time::Duration::from_secs(5), p.complete(req())).await;
        assert!(res.is_ok(), "wait must be bounded, not hang");
        assert!(res.unwrap().is_err(), "shed after the bounded wait");
    }

    /// adversarial: a hostile `saturation_wait_ms` is clamped, so `wait` can't be
    /// steered into a long stall.
    #[test]
    fn adversarial_saturation_wait_clamped() {
        let p = PoolProvider::new("t", vec![capped_member("a", 1)])
            .expect("pool")
            .with_saturation(Saturation::Wait, u64::MAX);
        assert_eq!(p.inner.saturation_wait_ms, 30_000, "clamped to the ceiling");
    }

    /// health() surfaces per-member saturation + cap.
    #[tokio::test]
    async fn positive_health_reports_saturation() {
        let p = PoolProvider::new("t", vec![capped_member("m", 1)]).expect("pool");
        p.bench_set_in_flight(0, 1);
        let h = p.health().await;
        assert_eq!(h.members[0].max_concurrency, 1);
        assert!(h.members[0].saturated, "at cap → saturated");
    }

    // --- graded health / latency EWMA (GPU pool 03) ------------------------

    fn graded_pool(threshold_ms: u64, names: &[&str]) -> PoolProvider {
        let specs = names
            .iter()
            .map(|n| ok_member(n, PoolTier::Light))
            .collect();
        PoolProvider::new("t", specs)
            .expect("pool")
            .with_policy(PoolPolicy::LeastLoaded)
            .with_health_grading(0.3, threshold_ms)
    }

    /// positive: a healthy (fast) member is preferred over a degraded (slow) one,
    /// even when least-loaded would otherwise pick the slow one.
    #[test]
    fn positive_healthy_sorts_before_degraded() {
        let p = graded_pool(1_000, &["slow", "fast"]);
        p.bench_set_latency(0, 5_000); // slow → degraded
        p.bench_set_in_flight(0, 0); //   …but idle
        p.bench_set_latency(1, 100); // fast → healthy
        p.bench_set_in_flight(1, 5); //   …but busier
                                     // Grade beats load: the healthy member comes first despite more in-flight.
        assert_eq!(p.bench_select(&req(), PoolTier::Light, 2), vec![1, 0]);
    }

    /// negative: with grading disabled (threshold 0), latency is ignored and the
    /// policy alone decides (least-loaded picks the idle-but-slow member).
    #[test]
    fn negative_grading_disabled_ignores_latency() {
        let p = graded_pool(0, &["slow", "fast"]);
        p.bench_set_latency(0, 5_000);
        p.bench_set_in_flight(0, 0);
        p.bench_set_latency(1, 100);
        p.bench_set_in_flight(1, 5);
        assert_eq!(p.bench_select(&req(), PoolTier::Light, 2), vec![0, 1]);
    }

    /// corner: both under the threshold ⇒ both healthy ⇒ the policy decides.
    #[test]
    fn corner_both_healthy_policy_decides() {
        let p = graded_pool(1_000, &["a", "b"]);
        p.bench_set_latency(0, 200);
        p.bench_set_in_flight(0, 3);
        p.bench_set_latency(1, 300);
        p.bench_set_in_flight(1, 1);
        // Least-loaded: b (1 in-flight) before a (3).
        assert_eq!(p.bench_select(&req(), PoolTier::Light, 2), vec![1, 0]);
    }

    /// boundary: all degraded ⇒ still served (degraded ≠ dead), ordered by policy.
    #[test]
    fn boundary_all_degraded_still_served() {
        let p = graded_pool(1_000, &["a", "b"]);
        p.bench_set_latency(0, 9_000);
        p.bench_set_in_flight(0, 4);
        p.bench_set_latency(1, 9_000);
        p.bench_set_in_flight(1, 1);
        let order = p.bench_select(&req(), PoolTier::Light, 2);
        assert_eq!(order.len(), 2, "degraded members are still eligible");
        assert_eq!(order, vec![1, 0], "same grade → least-loaded decides");
    }

    /// adversarial: a hostile huge latency is clamped in the health snapshot (no
    /// overflow/panic) and simply grades the member degraded.
    #[tokio::test]
    async fn adversarial_hostile_latency_clamped() {
        let p = graded_pool(1_000, &["m"]);
        p.bench_set_latency(0, u64::MAX);
        let h = p.health().await;
        assert_eq!(
            h.members[0].latency_ms_ewma,
            u32::MAX,
            "clamped, no overflow"
        );
        assert_eq!(h.members[0].state, PoolMemberState::Degraded);
    }

    /// health() reports the graded state + EWMA; a real call seeds the EWMA.
    #[tokio::test]
    async fn positive_call_seeds_ewma_and_grades() {
        let p = graded_pool(1_000, &["m"]);
        p.complete_all(req(), PoolTier::Light, 1).await;
        let h = p.health().await;
        // A scripted provider answers instantly, so it stays healthy.
        assert_eq!(h.members[0].state, PoolMemberState::Healthy);
    }
}
