//! `TaskRouter` — a metadata-driven, declaratively-routed provider (model-router
//! increment 02).
//!
//! Like [`crate::router::Router`] it **is-a** `LlmProvider` composing others with the
//! same failover safety — a retryable failure (or a member-specific *auth* failure,
//! 401/403) advances to the next upstream, a request-level terminal (billing/bad-request)
//! stops, and an open circuit breaker is
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

use crate::route::{estimate_min_context, Hint, Policy, Role, Saturation, UpstreamMeta};
use crate::router::{poll_for_capacity, Health, RouteEvent, RouteObserver, MAX_SATURATION_WAIT_MS};
use agent_core::{
    price_usage, ChunkStream, CompletionRequest, CompletionResponse, Error, LlmProvider,
    ModelCapabilities, ModelPrices, PoolTier, Result, Usage,
};
use async_trait::async_trait;
use futures_util::StreamExt;
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
    /// Per-Mtok output cost (clamped non-negative on build). With `input_cost`
    /// it prices a turn this upstream serves — see [`RouterUpstream::card_prices`].
    pub output_cost: f32,
    /// Aggregate concurrency this upstream can absorb (≈ GPUs × per-GPU slots for
    /// a multi-GPU gateway); `0` = unknown/unbounded. Feeds the capacity-normalised
    /// `least-loaded` ordering — see [`crate::route::UpstreamMeta::max_concurrency`].
    pub max_concurrency: u32,
    pub provider: Arc<dyn LlmProvider>,
}

impl RouterUpstream {
    /// The card's rates as a price row, or `None` for an unpriced card (both
    /// costs `0`) so the caller falls back to its own price table rather than
    /// recording a confident `$0`. Cache lines are `0`: the OpenAI-compatible
    /// decoder's `prompt_tokens` already *includes* cached tokens, so billing
    /// `cache_read_tokens` again would double-count — a card carries no cache
    /// discount, so cached input is priced at the full input rate (upper bound).
    fn card_prices(&self) -> Option<ModelPrices> {
        (self.input_cost > 0.0 || self.output_cost > 0.0).then(|| ModelPrices {
            input: f64::from(self.input_cost),
            output: f64::from(self.output_cost),
            cache_read: 0.0,
            cache_write: 0.0,
        })
    }
}

/// Stamp the serving upstream's card cost onto `usage` — only when the provider
/// did not report a cost itself (never overwrite a real figure).
fn stamp_cost(usage: &mut Option<Usage>, prices: Option<ModelPrices>) {
    if let (Some(u), Some(p)) = (usage.as_mut(), prices) {
        if u.cost.is_none() {
            u.cost = Some(price_usage(&p, u));
        }
    }
}

/// Per-upstream live dispatch accounting (model-router 04): requests currently
/// in flight and a smoothed latency, fed by the router's own dispatch path and
/// read by the [`crate::route::OrderPolicy`] live-signal ordering. Lock-free —
/// this sits on the per-call hot path.
#[derive(Default)]
pub(crate) struct LiveStats {
    // `Arc` so an [`InFlightGuard`] can hold a handle to *this* counter after it
    // is detached from `&self` and moved into a returned `ChunkStream` — the slot
    // must stay raised until the stream drains, not merely until it is set up.
    in_flight: Arc<AtomicU32>,
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

/// One pass's candidates (see [`TaskRouter::order`]): `order` is offered first;
/// `reserve` is the held-back spillover tier (gap §8.7 item 8), offered only when
/// nothing in `order` is admitted. `rule` = which rule decided (`Decided` event);
/// `spilled` = `order` already leads with the reserve (no primary had headroom).
struct Plan {
    order: Vec<usize>,
    reserve: Vec<usize>,
    rule: Option<usize>,
    spilled: bool,
}

/// RAII in-flight guard: decrements on every exit path (incl. panic/cancel), so
/// the least-loaded signal can never drift upward from a lost decrement — and
/// emits the [`RouteEvent::InFlight`] gauge event from BOTH edges (the release
/// fires in `Drop`, so a cancelled call still reports and the gauge drains to
/// 0 rather than sticking at its last value).
///
/// It is **owned** (`'static`), holding only cloned `Arc` handles rather than a
/// borrow of the router, so a streamed dispatch can move it *into* the returned
/// [`ChunkStream`]: the slot is then released when the stream drains or the
/// consumer drops it, not when the stream is merely set up (a streamed call that
/// is still generating must count as in-flight — see `route`/`stream`).
struct InFlightGuard {
    in_flight: Arc<AtomicU32>,
    observer: Option<RouteObserver>,
    upstream_id: Arc<str>,
}
impl InFlightGuard {
    /// Take one in-flight slot on upstream `i`. Soft mode (the default) and an
    /// uncapped upstream (`max_concurrency == 0`) always admit. Under the opt-in
    /// **hard** cap (gap §8.7 item 3) the check-and-increment is one CAS loop —
    /// the mirror of `PoolMember::try_reserve` — so N racing callers can never
    /// all pass the ceiling; `None` = at cap, don't dispatch.
    fn try_enter(router: &TaskRouter, i: usize) -> Option<Self> {
        let in_flight = Arc::clone(&router.live[i].in_flight);
        let cap = router.upstreams[i].max_concurrency;
        let n = if router.saturation.is_some() && cap != 0 {
            let mut cur = in_flight.load(Ordering::Acquire);
            loop {
                if cur >= cap {
                    return None;
                }
                match in_flight.compare_exchange_weak(
                    cur,
                    cur + 1,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                ) {
                    Ok(_) => break cur + 1,
                    Err(actual) => cur = actual, // lost the race — retry with the fresh value
                }
            }
        } else {
            in_flight.fetch_add(1, Ordering::AcqRel) + 1
        };
        let observer = router.observer.clone();
        let upstream_id: Arc<str> = Arc::from(router.upstreams[i].id.as_str());
        if let Some(o) = &observer {
            o(RouteEvent::InFlight {
                upstream: &upstream_id,
                count: n,
            });
        }
        Some(Self {
            in_flight,
            observer,
            upstream_id,
        })
    }
}
impl Drop for InFlightGuard {
    fn drop(&mut self) {
        let n = self
            .in_flight
            .fetch_sub(1, Ordering::AcqRel)
            .saturating_sub(1);
        if let Some(o) = &self.observer {
            o(RouteEvent::InFlight {
                upstream: &self.upstream_id,
                count: n,
            });
        }
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
    /// Router-owned retry budget (gap §8.7 item 9). Each `max_retries` is a
    /// whole-fleet **re-pass**: a pass that exhausts every candidate on transient
    /// failures backs off once (jittered, capped at `max_delay`) and tries the
    /// fleet again. Routed upstreams build fail-fast (`max_retries: 0`), so retry
    /// lives here — a 429 fails over to a headroom upstream at once instead of
    /// burning ≤20 s × N in-provider backoff first. Default `new(0)` = a single
    /// pass (today's behaviour); set via [`Self::with_retry_budget`].
    retry: agent_retry::RetryPolicy,
    /// Opt-in **hard** capacity (gap §8.7 item 3). `None` (the default) keeps
    /// model-router 05's soft semantics: `max_concurrency` only reorders and a
    /// saturated upstream is still dispatched. `Some(policy)` makes it an
    /// admission cap — a saturated upstream is skipped (never dispatched), and a
    /// pass where *every* candidate is saturated sheds or waits per `policy`.
    saturation: Option<Saturation>,
    /// Bounded wait budget (ms) for `Saturation::Wait`; clamped ≤30 s.
    saturation_wait_ms: u64,
    /// The registry snapshot fingerprint this fleet was built from (model-router
    /// 04 tail): `0` = a static (TOML-built) fleet. Carried on every decision
    /// event so a routing choice is attributable to a fleet version.
    snapshot_version: u64,
}

impl TaskRouter {
    /// Build a router over `upstreams` steered by `policy`. Errors on an empty fleet
    /// (a router with nothing to route to can never answer). Per-member `input_cost` /
    /// `output_cost` are clamped finite + non-negative so a hostile config can't
    /// poison ordering or a turn's price.
    pub fn new(mut upstreams: Vec<RouterUpstream>, policy: Policy) -> Result<Self> {
        if upstreams.is_empty() {
            return Err(Error::Provider(
                "task-router needs at least one upstream".into(),
            ));
        }
        for u in &mut upstreams {
            for c in [&mut u.input_cost, &mut u.output_cost] {
                if !c.is_finite() || *c < 0.0 {
                    *c = 0.0;
                }
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
            retry: agent_retry::RetryPolicy::new(0),
            saturation: None,
            saturation_wait_ms: 0,
            snapshot_version: 0,
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
    /// Set the router-owned retry budget — the number of whole-fleet re-passes on
    /// transient failure (gap §8.7 item 9). Capped at
    /// [`agent_core::MAX_UPSTREAM_RETRIES`] so a hostile/fat-fingered config can't
    /// turn into an unbounded retry storm; `0` (the default) keeps the single-pass
    /// behaviour. The schedule is `agent-retry`'s canonical jittered backoff
    /// (500 ms base, capped at 20 s) — one wait between passes, not per upstream.
    pub fn with_retry_budget(mut self, max_retries: u32) -> Self {
        self.retry =
            agent_retry::RetryPolicy::new(max_retries.min(agent_core::MAX_UPSTREAM_RETRIES));
        self
    }
    /// Opt into the **hard** per-upstream concurrency cap (gap §8.7 item 3):
    /// `None` = soft (the default; `max_concurrency` only reorders), `Some(Shed)`
    /// = skip saturated upstreams and shed when all are saturated, `Some(Wait)` =
    /// first wait up to `wait_ms` (clamped ≤30 s) for a slot to free. An upstream
    /// with `max_concurrency == 0` is uncapped in every mode.
    pub fn with_saturation(mut self, saturation: Option<Saturation>, wait_ms: u64) -> Self {
        self.saturation = saturation;
        self.saturation_wait_ms = wait_ms.min(MAX_SATURATION_WAIT_MS);
        self
    }
    pub fn with_observer(mut self, observer: RouteObserver) -> Self {
        self.observer = Some(observer);
        self
    }
    /// The effective saturation policy and (clamped) wait budget — `(None, _)`
    /// = soft. Lets a `RegistryRouter` rebuild be checked for the opt-in cap.
    pub fn saturation_policy(&self) -> (Option<Saturation>, u64) {
        (self.saturation, self.saturation_wait_ms)
    }
    /// Stamp the registry snapshot fingerprint this fleet was built from
    /// (`RegistryRouter` sets it on every rebuild; `0` = static fleet).
    pub fn with_snapshot_version(mut self, version: u64) -> Self {
        self.snapshot_version = version;
        self
    }
    pub fn snapshot_version(&self) -> u64 {
        self.snapshot_version
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
            max_concurrency: u.max_concurrency,
        }
    }

    /// Whether an upstream is at its concurrency ceiling right now
    /// (`max_concurrency != 0 && in_flight >= max_concurrency`). `0` = unbounded,
    /// never saturated. Read live off [`LiveStats`] — a saturated upstream is
    /// deferred behind one with headroom in [`Self::order`].
    fn is_saturated(&self, i: usize) -> bool {
        let cap = self.upstreams[i].max_concurrency;
        cap != 0 && self.live[i].snapshot().0 >= cap
    }

    /// Indices to try, in order: the policy's preferred-and-capable order, then
    /// three buckets so a request lands on a *usable* upstream first — those with
    /// **headroom** ahead of those at their concurrency ceiling (gap §8.7 item 9:
    /// fail over to an upstream with headroom), ahead of those whose breaker is
    /// **open** (skipped-then-tried-last so a total outage still attempts
    /// something). Policy order is preserved within each bucket. Also returns
    /// which rule decided (for the `Decided` event). The engine resolves straight
    /// to fleet indices (same order as `self.upstreams`) — no by-id re-lookup.
    ///
    /// **Spillover** (gap §8.7 item 8): survivors tagged with the deciding
    /// preference's `spill_to` are a held-back reserve. While some primary has
    /// usable headroom (breaker closed, under its cap) the reserve is kept out of
    /// [`Plan::order`] and parked in [`Plan::reserve`] — tried within the pass
    /// only if every primary then refuses admission (hard cap). Once *no* primary
    /// has headroom the plan spills: the reserve leads, primaries follow. A
    /// fleet where only the reserve can serve uses it as primary (fail-soft —
    /// spillover never refuses a request the fleet could answer).
    fn order(&self, hint: &Hint) -> Plan {
        let now = (self.now_ms)();
        let fleet: Vec<UpstreamMeta<'_>> =
            (0..self.upstreams.len()).map(|i| self.meta(i)).collect();
        let (mut ordered, mut rule) = self.policy.resolve_indices(hint, &fleet);
        // An override can't jump the spill queue: one naming a reserve upstream
        // (under the preference that would otherwise decide) is dropped, so the
        // reserve is still only spilled onto — a carried hint can't pull a
        // request onto the (typically paid) tier while a primary has headroom.
        if hint.override_upstream.is_some() {
            let plain = Hint {
                override_upstream: None,
                ..hint.clone()
            };
            let (all, plain_rule) = self.policy.resolve_indices(&plain, &fleet);
            let pick_is_reserve = ordered.first().is_some_and(|&i| {
                self.policy
                    .prefer_for(plain_rule)
                    .is_reserve(&self.upstreams[i].tags)
            });
            if pick_is_reserve {
                (ordered, rule) = (all, plain_rule);
            }
        }
        let prefer = self.policy.prefer_for(rule);
        let (mut primary, mut reserve): (Vec<usize>, Vec<usize>) = ordered
            .into_iter()
            .partition(|&i| !prefer.is_reserve(&self.upstreams[i].tags));
        if primary.is_empty() {
            primary = std::mem::take(&mut reserve);
        }
        let (mut primary, primary_headroom) = self.bucket(primary, now);
        let (mut reserve, _) = self.bucket(reserve, now);
        let spilled = !reserve.is_empty() && !primary_headroom;
        if spilled {
            reserve.append(&mut primary);
            primary = std::mem::take(&mut reserve);
        }
        Plan {
            order: primary,
            reserve,
            rule,
            spilled,
        }
    }

    /// Bucket `ordered` into headroom → saturated → breaker-open (policy order
    /// kept within each); the flag says whether any member has usable headroom.
    fn bucket(&self, ordered: Vec<usize>, now: u64) -> (Vec<usize>, bool) {
        let mut headroom = Vec::new();
        let mut saturated = Vec::new();
        let mut unhealthy = Vec::new();
        for i in ordered {
            if self.health[i].is_open(now, self.cooldown_ms) {
                self.emit(RouteEvent::SkippedUnhealthy {
                    target: &self.upstreams[i].id,
                });
                unhealthy.push(i);
            } else if self.is_saturated(i) {
                saturated.push(i);
            } else {
                headroom.push(i);
            }
        }
        let has_headroom = !headroom.is_empty();
        headroom.extend(saturated);
        headroom.extend(unhealthy);
        (headroom, has_headroom)
    }

    /// Try each chosen upstream in turn, stopping at the first success or the first
    /// **terminal** failure (mirrors `Router::route`).
    /// `op` is handed the chosen provider **and** this attempt's [`InFlightGuard`];
    /// a buffered call holds it for the duration of its future, a streamed call
    /// moves it into the returned stream so the slot drains with the stream rather
    /// than at setup.
    async fn route<T, F, Fut>(&self, req: &CompletionRequest, op: F) -> Result<T>
    where
        F: Fn(Arc<dyn LlmProvider>, InFlightGuard, Option<ModelPrices>) -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        let hint = self.hint(req);
        let mode = hint.task_mode.map_or("-", |m| m.as_str());
        // Decide once up front — the fleet + policy don't change between retry
        // passes, so the `Decided`/`NoCandidate` event and the `route.select`
        // trail reflect the opening choice (the per-pass `order()` is recomputed
        // inside each pass so breaker/cooldown state from a failed pass steers
        // the next one).
        let Plan {
            order: first_order,
            rule,
            ..
        } = self.order(&hint);
        if first_order.is_empty() {
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
            chosen: &self.upstreams[first_order[0]].id,
        });
        // Attaches to the caller's current span, so a decision is reproducible
        // against the exact fleet version that produced it (0 = static fleet).
        tracing::debug!(
            target: "route.select",
            snapshot_version = self.snapshot_version,
            role = hint.role.as_str(),
            task_mode = mode,
            rule = ?rule,
            chosen = %self.upstreams[first_order[0]].id,
            "route decided"
        );

        // Router-owned retry (gap §8.7 item 9): one `op()` call = one whole-fleet
        // pass; `agent_retry::run` re-invokes it up to `retry.max_retries` times,
        // backing off once (jittered, capped) between passes. Failover *within* a
        // pass stays sleepless — now actually fast because routed upstreams build
        // fail-fast, so each surfaces its 429 at once.
        let op = &op;
        let hint = &hint;
        let out =
            agent_retry::run(&self.retry, || async move { self.one_pass(hint, op).await }).await;
        if out.is_err() {
            // Budget exhausted (or the fleet went terminal on a pass) — the chain
            // is done.
            self.emit(RouteEvent::Exhausted);
        }
        out
    }

    /// One whole-fleet pass over a freshly-recomputed [`Self::order`]: the first
    /// success is [`Attempt::Done`]; a request-level terminal aborts the chain
    /// ([`Attempt::Fail`], no budget spend); otherwise every candidate is tried
    /// in turn (sleeplessly) and the pass ends in [`Attempt::Retry`] so the
    /// driver backs off once and re-passes the fleet (if budget remains). Under
    /// the opt-in hard cap a saturated candidate is skipped; a pass that admits
    /// *nothing* waits (bounded, once) or sheds per [`Saturation`].
    async fn one_pass<T, F, Fut>(&self, hint: &Hint, op: &F) -> agent_retry::Attempt<T, Error>
    where
        F: Fn(Arc<dyn LlmProvider>, InFlightGuard, Option<ModelPrices>) -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        self.run_plan(hint, op, self.order(hint)).await
    }

    /// The body of [`Self::one_pass`] over an already-built [`Plan`].
    async fn run_plan<T, F, Fut>(
        &self,
        hint: &Hint,
        op: &F,
        plan: Plan,
    ) -> agent_retry::Attempt<T, Error>
    where
        F: Fn(Arc<dyn LlmProvider>, InFlightGuard, Option<ModelPrices>) -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        if plan.order.is_empty() {
            // A pass can find no candidate only if every upstream's breaker is
            // open AND filtered — treat as exhausted, don't spin the budget.
            return agent_retry::Attempt::Fail(Error::Provider(
                "no upstream can serve this request (capability/requirement mismatch)".into(),
            ));
        }
        // Emitted here, not in `order()`, so the opening `Decided` plan in
        // `route()` doesn't double-count a spill.
        if plan.spilled {
            self.emit(RouteEvent::Spilled {
                role: hint.role.as_str(),
            });
        }
        let mut last: Option<Error> = None;
        let mut waited = false;
        loop {
            let (mut admitted, done) = self.try_each(hint, op, &plan.order, &mut last).await;
            if let Some(done) = done {
                return done;
            }
            // Every primary refused admission (they filled between `order()` and
            // the CAS) — spill onto the held-back reserve before waiting/shedding.
            if !admitted && !plan.reserve.is_empty() {
                self.emit(RouteEvent::Spilled {
                    role: hint.role.as_str(),
                });
                let (a, done) = self.try_each(hint, op, &plan.reserve, &mut last).await;
                if let Some(done) = done {
                    return done;
                }
                admitted = a;
            }
            if admitted {
                break;
            }
            // Every candidate is at its hard cap. `wait`: poll (bounded) for a slot
            // to free, then re-run the pass ONCE; otherwise / on timeout, shed —
            // `Fail`, so a QoS shed spends no retry budget (pool semantics).
            if self.saturation == Some(Saturation::Wait) && !waited {
                waited = true;
                let freed = poll_for_capacity(self.saturation_wait_ms, || {
                    plan.order
                        .iter()
                        .chain(&plan.reserve)
                        .any(|&i| !self.is_saturated(i))
                        .then_some(())
                })
                .await;
                if freed.is_some() {
                    continue;
                }
            }
            self.emit(RouteEvent::Shed {
                role: hint.role.as_str(),
            });
            return agent_retry::Attempt::Fail(Error::Provider(
                "task-router saturated: all candidate upstreams at capacity".into(),
            ));
        }
        // The whole fleet failed transiently this pass. Back off once (the driver
        // owns the wait) and re-pass if budget remains; `after: None` because the
        // per-upstream `Retry-After` is not on the error message the router sees —
        // the point is to move fast across the fleet, not honour one upstream's
        // hint. Budget-0 (the default) ⇒ `run` returns this err after one pass.
        let err =
            last.unwrap_or_else(|| Error::Provider("task-router exhausted all upstreams".into()));
        agent_retry::Attempt::Retry { err, after: None }
    }

    /// Offer the request to each of `cands` in turn: a candidate at its hard cap
    /// is skipped (not dispatched, not a breaker failure); an admitted one is
    /// dispatched. Returns whether anything was admitted, plus `Some` when the
    /// pass is decided (success or a request-terminal).
    async fn try_each<T, F, Fut>(
        &self,
        hint: &Hint,
        op: &F,
        cands: &[usize],
        last: &mut Option<Error>,
    ) -> (bool, Option<agent_retry::Attempt<T, Error>>)
    where
        F: Fn(Arc<dyn LlmProvider>, InFlightGuard, Option<ModelPrices>) -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        let mut admitted = false;
        for (attempt, &i) in cands.iter().enumerate() {
            // Hard cap (opt-in): an upstream at its ceiling is skipped. Soft mode
            // always admits.
            let Some(guard) = InFlightGuard::try_enter(self, i) else {
                self.emit(RouteEvent::SkippedSaturated {
                    target: &self.upstreams[i].id,
                });
                continue;
            };
            admitted = true;
            if let Some(done) = self.dispatch(hint, op, cands, attempt, guard, last).await {
                return (true, Some(done));
            }
        }
        (admitted, None)
    }

    /// Dispatch one admitted candidate (`order[attempt]`, slot already held by
    /// `guard`). `Some(attempt)` ends the pass (success or a request-terminal);
    /// `None` = it failed over-ably — `last` holds the error, try the next.
    async fn dispatch<T, F, Fut>(
        &self,
        hint: &Hint,
        op: &F,
        order: &[usize],
        attempt: usize,
        guard: InFlightGuard,
        last: &mut Option<Error>,
    ) -> Option<agent_retry::Attempt<T, Error>>
    where
        F: Fn(Arc<dyn LlmProvider>, InFlightGuard, Option<ModelPrices>) -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        let i = order[attempt];
        let u = &self.upstreams[i];
        self.emit(RouteEvent::Routed { target: &u.id });
        let started = (self.now_ms)();
        let outcome = op(u.provider.clone(), guard, u.card_prices()).await;
        match outcome {
            Ok(v) => {
                self.health[i].record_success();
                let elapsed = (self.now_ms)().saturating_sub(started);
                self.live[i].record_latency(u32::try_from(elapsed).unwrap_or(u32::MAX));
                self.emit(RouteEvent::Dispatched {
                    role: hint.role.as_str(),
                    upstream: &u.id,
                    outcome: "ok",
                });
                Some(agent_retry::Attempt::Done(v))
            }
            Err(e) => {
                let msg = e.to_string();
                self.health[i].record_failure((self.now_ms)(), self.failure_threshold);
                // Terminal errors fail identically on every upstream — stop. EXCEPT an
                // auth-terminal (401/403), which is upstream-specific (a rotated key /
                // forbidden endpoint on this upstream says nothing about the next), so
                // fall over instead of aborting the whole chain.
                let auth_terminal = agent_retry::is_auth_terminal(&msg);
                if agent_retry::classify(&msg) == agent_retry::Class::Terminal && !auth_terminal {
                    self.emit(RouteEvent::Dispatched {
                        role: hint.role.as_str(),
                        upstream: &u.id,
                        outcome: "terminal",
                    });
                    // A request-terminal fails the same way on a re-pass too —
                    // abort without spending the budget.
                    return Some(agent_retry::Attempt::Fail(e));
                }
                self.emit(RouteEvent::Dispatched {
                    role: hint.role.as_str(),
                    upstream: &u.id,
                    outcome: if auth_terminal { "auth" } else { "retryable" },
                });
                if attempt + 1 < order.len() {
                    self.emit(RouteEvent::FellOver {
                        from: &u.id,
                        to: &self.upstreams[order[attempt + 1]].id,
                        reason: if auth_terminal { "auth" } else { "retryable" },
                    });
                }
                *last = Some(e);
                None
            }
        }
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
        self.route(&req, move |p, guard, prices| {
            let r = r.clone();
            // A buffered call is in-flight for the whole future: hold the guard
            // until `complete` resolves, then it drops here.
            async move {
                let _in_flight = guard;
                let mut resp = p.complete(r).await?;
                stamp_cost(&mut resp.usage, prices);
                Ok(resp)
            }
        })
        .await
    }

    async fn stream(&self, req: CompletionRequest) -> Result<ChunkStream> {
        // Fallover covers failures raised while *establishing* the stream; once bytes
        // flow the turn is committed (mirrors `Router::stream`).
        let r = req.clone();
        self.route(&req, move |p, guard, prices| {
            let r = r.clone();
            async move {
                let inner = p.stream(r).await?; // setup failure drops `guard` here → falls over
                                                // Move the guard INTO the returned stream so the in-flight slot is
                                                // held until the stream drains (or the consumer drops it), not at
                                                // setup — a streamed upstream is busy while it generates. Mirrors
                                                // metered.rs's chunk-stream wrapper.
                let guarded = async_stream::stream! {
                    let _in_flight = guard;
                    let mut inner = inner;
                    while let Some(mut item) = inner.next().await {
                        // The terminal chunk carries usage: price it by the
                        // upstream that actually generated it.
                        if let Ok(chunk) = &mut item {
                            stamp_cost(&mut chunk.usage, prices);
                        }
                        yield item;
                    }
                };
                Ok(Box::pin(guarded) as ChunkStream)
            }
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

    /// A provider whose `stream` yields `n` text chunks **lazily** (no pre-collect,
    /// so `n = usize::MAX` models an endless upstream). `complete` is unsupported —
    /// it exists only to exercise the streamed in-flight accounting.
    struct StreamProvider {
        n: usize,
        caps: ModelCapabilities,
    }
    #[async_trait]
    impl LlmProvider for StreamProvider {
        fn capabilities(&self) -> ModelCapabilities {
            self.caps.clone()
        }
        async fn complete(&self, _r: CompletionRequest) -> Result<CompletionResponse> {
            Err(Error::Provider("stream-only provider".into()))
        }
        async fn stream(&self, _r: CompletionRequest) -> Result<ChunkStream> {
            let s = futures_util::stream::iter(0..self.n).map(|i| {
                Ok(agent_core::CompletionChunk {
                    delta_text: format!("c{i}"),
                    ..Default::default()
                })
            });
            Ok(Box::pin(s))
        }
    }

    fn streamer(id: &str, n: usize) -> RouterUpstream {
        up(
            id,
            Arc::new(StreamProvider {
                n,
                caps: caps(true, false, 1000),
            }),
        )
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
            output_cost: 0.0,
            max_concurrency: 0,
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
                spill_to: vec![],
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

    /// A static (TOML-built) fleet reports version 0; `RegistryRouter` stamps
    /// the snapshot fingerprint via the builder.
    #[test]
    fn positive_snapshot_version_defaults_static_and_is_stampable() {
        let r = router(vec![up("glm", ok("x", true, false))], prefer(&["glm"]));
        assert_eq!(r.snapshot_version(), 0);
        assert_eq!(r.with_snapshot_version(7).snapshot_version(), 7);
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
    async fn negative_request_terminal_does_not_fall_over() {
        // A request-level terminal (400) fails identically everywhere → abort.
        let primary = Arc::new(FailProvider::new("http 400: unsupported parameter"));
        let secondary = Arc::new(FailProvider::new("should-not-be-reached"));
        let r = router(
            vec![up("kimi", primary.clone()), up("glm", secondary.clone())],
            prefer(&["kimi", "glm"]),
        );
        assert!(r.complete(req()).await.is_err());
        assert_eq!(
            secondary.calls.load(Ordering::SeqCst),
            0,
            "a request-terminal failure must not burn the fallback"
        );
    }

    // --- positive: auth-terminal is upstream-specific → falls over -----------
    #[tokio::test]
    async fn positive_auth_terminal_falls_over_to_next() {
        // A 403 (rotated key / forbidden endpoint) on kimi says nothing about glm.
        let primary = Arc::new(FailProvider::new("http 403: forbidden"));
        let r = router(
            vec![
                up("kimi", primary.clone()),
                up("glm", ok("from-glm", true, false)),
            ],
            prefer(&["kimi", "glm"]),
        );
        let resp = r
            .complete(req())
            .await
            .expect("auth-terminal must fall over");
        assert_eq!(resp.message.content_text(), "from-glm");
        assert_eq!(primary.calls.load(Ordering::SeqCst), 1, "primary was tried");
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
            output_cost: 0.0,
            max_concurrency: 0,
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
            output_cost: 0.0,
            max_concurrency: 0,
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
            output_cost: 0.0,
            max_concurrency: 0,
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

    #[tokio::test]
    async fn positive_inflight_events_fire_on_both_edges_including_failover() {
        // A failed-then-successful dispatch emits 1,0 per attempted upstream —
        // the release edge fires even for the failed attempt (RAII), so a
        // gauge fed from these events always drains to 0.
        let fail = FailProvider::new("http 500: transient");
        let failing = RouterUpstream {
            id: "bad".into(),
            tags: vec![],
            tier: PoolTier::Medium,
            input_cost: 0.0,
            output_cost: 0.0,
            max_concurrency: 0,
            provider: Arc::new(fail),
        };
        let ok = RouterUpstream {
            id: "ok".into(),
            tags: vec![],
            tier: PoolTier::Medium,
            input_cost: 0.0,
            output_cost: 0.0,
            max_concurrency: 0,
            provider: Arc::new(OkProvider {
                answer: "fine".into(),
                caps: caps(true, false, 1000),
            }),
        };
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = events.clone();
        let router = TaskRouter::new(vec![failing, ok], prefer(&["bad", "ok"]))
            .unwrap()
            .with_observer(Arc::new(move |ev| {
                if let RouteEvent::InFlight { upstream, count } = ev {
                    sink.lock().unwrap().push((upstream.to_string(), count));
                }
            }));
        router
            .complete(CompletionRequest::default())
            .await
            .expect("fails over");
        assert_eq!(
            events.lock().unwrap().clone(),
            vec![
                ("bad".to_string(), 1),
                ("bad".to_string(), 0),
                ("ok".to_string(), 1),
                ("ok".to_string(), 0),
            ]
        );
    }

    // --- streamed in-flight accounting (gap §8.7 item 2) -------------------
    // A streamed dispatch must stay in-flight until the stream *drains*, not
    // merely until it is set up: the guard moved into the returned ChunkStream
    // releases the slot on drain / drop, so least-loaded never reads a still-
    // generating upstream as idle.

    #[tokio::test]
    async fn positive_streamed_call_stays_in_flight_until_the_stream_drains() {
        let router = router(vec![streamer("s", 3)], prefer(&["s"]));
        let stream = router.stream(req()).await.expect("stream set up");
        // The bug was that the slot dropped here, at setup.
        assert_eq!(
            router.live[0].snapshot().0,
            1,
            "a streamed call is in-flight while it generates, not just at setup"
        );
        let chunks: Vec<_> = stream.collect().await;
        assert_eq!(chunks.len(), 3);
        assert_eq!(
            router.live[0].snapshot().0,
            0,
            "the slot releases once the stream is fully consumed"
        );
    }

    #[tokio::test]
    async fn positive_streamed_inflight_release_event_fires_on_drain_not_setup() {
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = events.clone();
        let router = TaskRouter::new(vec![streamer("s", 2)], prefer(&["s"]))
            .unwrap()
            .with_observer(Arc::new(move |ev| {
                if let RouteEvent::InFlight { upstream, count } = ev {
                    sink.lock().unwrap().push((upstream.to_string(), count));
                }
            }));
        let stream = router.stream(req()).await.expect("stream set up");
        // Only the acquire edge so far — the gauge must NOT have drained at setup.
        assert_eq!(events.lock().unwrap().clone(), vec![("s".to_string(), 1)]);
        let _: Vec<_> = stream.collect().await;
        assert_eq!(
            events.lock().unwrap().clone(),
            vec![("s".to_string(), 1), ("s".to_string(), 0)],
            "the release edge fires when the stream drains"
        );
    }

    #[tokio::test]
    async fn negative_stream_setup_failure_releases_slot_and_falls_over() {
        // The first upstream's stream setup fails (retryable); it must release its
        // own slot and fail over to a working streamer, leaving both at zero.
        let bad = up("bad", Arc::new(FailProvider::new("http 503: transient")));
        let router = router(vec![bad, streamer("good", 1)], prefer(&["bad", "good"]));
        let stream = router.stream(req()).await.expect("falls over to good");
        let _: Vec<_> = stream.collect().await;
        for live in &router.live {
            assert_eq!(
                live.snapshot().0,
                0,
                "every attempt released its in-flight slot"
            );
        }
    }

    #[tokio::test]
    async fn boundary_empty_stream_acquires_and_releases_exactly_once() {
        // A zero-chunk stream still takes and returns exactly one slot: acquire at
        // setup, release when the (immediately empty) stream drains.
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = events.clone();
        let router = TaskRouter::new(vec![streamer("s", 0)], prefer(&["s"]))
            .unwrap()
            .with_observer(Arc::new(move |ev| {
                if let RouteEvent::InFlight { upstream, count } = ev {
                    sink.lock().unwrap().push((upstream.to_string(), count));
                }
            }));
        let stream = router.stream(req()).await.expect("stream set up");
        let chunks: Vec<_> = stream.collect().await;
        assert!(chunks.is_empty());
        assert_eq!(router.live[0].snapshot().0, 0);
        assert_eq!(
            events.lock().unwrap().clone(),
            vec![("s".to_string(), 1), ("s".to_string(), 0)]
        );
    }

    #[tokio::test]
    async fn adversarial_abandoned_unbounded_stream_does_not_leak_in_flight() {
        // A hostile endpoint that streams forever must not pin a slot when the
        // consumer abandons the turn: dropping the stream drops the guard.
        let router = router(vec![streamer("s", usize::MAX)], prefer(&["s"]));
        let mut stream = router.stream(req()).await.expect("stream set up");
        assert_eq!(router.live[0].snapshot().0, 1);
        let _first = stream.next().await.expect("one chunk");
        assert_eq!(
            router.live[0].snapshot().0,
            1,
            "still in-flight partway through an endless stream"
        );
        drop(stream); // abandon it before it ever ends
        assert_eq!(
            router.live[0].snapshot().0,
            0,
            "abandoning an endless stream must not leak the slot"
        );
    }

    // --- router-owned retry on the routed path (gap §8.7 item 9) -----------
    // Routed upstreams build fail-fast, so the TaskRouter owns retry as a
    // bounded whole-fleet re-pass budget and defers saturated upstreams behind
    // ones with headroom. Failover WITHIN a pass is sleepless (fast); the only
    // wait is one jittered backoff BETWEEN passes. `start_paused` lets a test
    // read that wait off the virtual clock.

    /// Fails with `msg` for its first `fail_times` calls, then succeeds with
    /// `answer`. Models an upstream that is briefly rate-limited then recovers.
    struct FlakyProvider {
        msg: String,
        answer: String,
        fail_times: usize,
        calls: Arc<AtomicUsize>,
    }
    #[async_trait]
    impl LlmProvider for FlakyProvider {
        fn capabilities(&self) -> ModelCapabilities {
            caps(true, false, 1000)
        }
        async fn complete(&self, r: CompletionRequest) -> Result<CompletionResponse> {
            let n = self.calls.fetch_add(1, Ordering::SeqCst);
            if n < self.fail_times {
                Err(Error::Provider(self.msg.clone()))
            } else {
                ScriptedProvider::new(vec![final_turn(&self.answer)])
                    .complete(r)
                    .await
            }
        }
    }

    /// Give a built router a **deterministic** (no-jitter) retry budget so a test
    /// can assert exact backoff timings and dispatch counts — production uses the
    /// jittered `with_retry_budget`.
    fn with_budget(mut r: TaskRouter, budget: u32) -> TaskRouter {
        r.retry = agent_retry::RetryPolicy::new(budget).with_jitter(agent_retry::Jitter::None);
        r
    }

    fn up_cap(id: &str, provider: Arc<dyn LlmProvider>, max_concurrency: u32) -> RouterUpstream {
        RouterUpstream {
            max_concurrency,
            ..up(id, provider)
        }
    }

    #[tokio::test(start_paused = true)]
    async fn positive_429_fails_over_fast_to_headroom_upstream() {
        // kimi is fail-fast-429; glm is healthy. glm serves on the SAME pass —
        // kimi is tried exactly once (no in-provider retry, no re-pass) and no
        // backoff elapses, because the pass succeeded.
        let bad = Arc::new(FailProvider::new("http 429: slow down"));
        let r = with_budget(
            router(
                vec![
                    up("kimi", bad.clone()),
                    up("glm", ok("from-glm", true, false)),
                ],
                prefer(&["kimi", "glm"]),
            ),
            3,
        );
        let start = tokio::time::Instant::now();
        let resp = r.complete(req()).await.expect("fails over");
        assert_eq!(resp.message.content_text(), "from-glm");
        assert_eq!(
            bad.calls.load(Ordering::SeqCst),
            1,
            "the 429 upstream is tried once — failover, not in-provider retry"
        );
        assert_eq!(
            start.elapsed(),
            std::time::Duration::ZERO,
            "failover within a pass is sleepless"
        );
    }

    #[tokio::test]
    async fn positive_saturated_upstream_is_deferred() {
        // kimi is preferred but pinned at its concurrency ceiling; glm has
        // headroom, so glm serves even though the policy lists kimi first.
        let r = router(
            vec![
                up_cap("kimi", ok("from-kimi", true, false), 1),
                up_cap("glm", ok("from-glm", true, false), 1),
            ],
            prefer(&["kimi", "glm"]),
        );
        r.live[0].in_flight.store(1, Ordering::Relaxed); // kimi at its ceiling
        let resp = r.complete(req()).await.expect("routes to headroom");
        assert_eq!(resp.message.content_text(), "from-glm");
    }

    #[tokio::test]
    async fn negative_terminal_aborts_without_budget_retry() {
        // A request-terminal (400) fails identically everywhere: abort at once,
        // no failover and no whole-fleet re-pass even with a budget set.
        let primary = Arc::new(FailProvider::new("http 400: unsupported parameter"));
        let secondary = Arc::new(FailProvider::new("should-not-be-reached"));
        let r = with_budget(
            router(
                vec![up("kimi", primary.clone()), up("glm", secondary.clone())],
                prefer(&["kimi", "glm"]),
            ),
            5,
        );
        assert!(r.complete(req()).await.is_err());
        assert_eq!(
            primary.calls.load(Ordering::SeqCst),
            1,
            "a request-terminal is tried exactly once — no budget spend"
        );
        assert_eq!(
            secondary.calls.load(Ordering::SeqCst),
            0,
            "a request-terminal must not fall over"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn corner_whole_fleet_429_backs_off_then_succeeds() {
        // Both upstreams 429 on pass 1; glm recovers on pass 2. Exactly one
        // between-pass backoff happens — deterministic (no jitter, 500 ms base,
        // attempt-0 ceiling) — and failover within each pass stays sleepless.
        let kimi = Arc::new(FailProvider::new("http 429: slow down"));
        let glm_calls = Arc::new(AtomicUsize::new(0));
        let glm = Arc::new(FlakyProvider {
            msg: "http 429: slow down".into(),
            answer: "from-glm".into(),
            fail_times: 1,
            calls: glm_calls.clone(),
        });
        let r = with_budget(
            router(
                vec![up("kimi", kimi.clone()), up("glm", glm)],
                prefer(&["kimi", "glm"]),
            ),
            3,
        );
        let start = tokio::time::Instant::now();
        let resp = r.complete(req()).await.expect("recovers on the re-pass");
        assert_eq!(resp.message.content_text(), "from-glm");
        assert_eq!(
            start.elapsed(),
            std::time::Duration::from_millis(500),
            "exactly one between-pass backoff"
        );
        assert_eq!(
            glm_calls.load(Ordering::SeqCst),
            2,
            "glm is tried once per pass (pass 1 fails, pass 2 serves)"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn boundary_retry_budget_is_exhausted_then_errors() {
        // A permanently-429 two-upstream fleet: with budget B the router makes
        // 1 + B whole-fleet passes, so each upstream is dispatched 1 + B times,
        // then errors. Check-the-check: budget 0 fails after a single pass.
        for (budget, per_upstream) in [(2u32, 3usize), (0, 1)] {
            let a = Arc::new(FailProvider::new("http 429: slow down"));
            let b = Arc::new(FailProvider::new("http 429: slow down"));
            let r = with_budget(
                router(
                    vec![up("a", a.clone()), up("b", b.clone())],
                    prefer(&["a", "b"]),
                ),
                budget,
            );
            let err = r.complete(req()).await.expect_err("fleet is down");
            assert!(err.to_string().contains("429"), "{err}");
            assert_eq!(
                a.calls.load(Ordering::SeqCst),
                per_upstream,
                "budget {budget}: upstream a dispatch count"
            );
            assert_eq!(
                b.calls.load(Ordering::SeqCst),
                per_upstream,
                "budget {budget}: upstream b dispatch count"
            );
        }
    }

    #[tokio::test(start_paused = true)]
    async fn adversarial_permanent_429_storm_cannot_pin_the_router() {
        // Every upstream always 429s. The call returns within the bounded budget:
        // at most `budget` backoffs, each clamped to `max_delay` — never the old
        // unbounded, minutes-long in-provider burn. Budget 4, no jitter ⇒ the
        // total wait is 0.5 + 1 + 2 + 4 = 7.5 s (all ceilings ≤ 20 s cap).
        let a = Arc::new(FailProvider::new("http 429: slow down"));
        let b = Arc::new(FailProvider::new("http 429: slow down"));
        let r = with_budget(
            router(
                vec![up("a", a.clone()), up("b", b.clone())],
                prefer(&["a", "b"]),
            ),
            4,
        );
        let start = tokio::time::Instant::now();
        let err = r.complete(req()).await.expect_err("storm never clears");
        assert!(err.to_string().contains("429"), "{err}");
        assert_eq!(
            start.elapsed(),
            std::time::Duration::from_millis(7500),
            "total wait is the bounded sum of capped backoffs, not minutes"
        );
        assert_eq!(a.calls.load(Ordering::SeqCst), 5, "1 + 4 passes");
        assert_eq!(b.calls.load(Ordering::SeqCst), 5);
    }

    // --- opt-in hard capacity (gap §8.7 item 3) ------------------------------

    /// A succeeding provider that counts its calls.
    fn counting(answer: &str) -> (Arc<dyn LlmProvider>, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let p = Arc::new(FlakyProvider {
            msg: String::new(),
            answer: answer.into(),
            fail_times: 0,
            calls: calls.clone(),
        });
        (p, calls)
    }

    /// Two-upstream fleet: `glm` (preferred, has headroom) always 429s; `kimi`
    /// is healthy but pinned at its cap of 1. Soft mode falls over onto the
    /// saturated kimi; hard mode must not.
    fn saturated_fallback(saturation: Option<Saturation>) -> (TaskRouter, Arc<AtomicUsize>) {
        let (kimi, kimi_calls) = counting("from-kimi");
        let r = router(
            vec![
                up("glm", Arc::new(FailProvider::new("http 429: slow down"))),
                up_cap("kimi", kimi, 1),
            ],
            prefer(&["glm", "kimi"]),
        )
        .with_saturation(saturation, 0);
        r.live[1].in_flight.store(1, Ordering::Relaxed); // kimi at its ceiling
        (r, kimi_calls)
    }

    #[tokio::test]
    async fn positive_hard_cap_skips_saturated_upstream() {
        let (r, kimi_calls) = saturated_fallback(Some(Saturation::Shed));
        let err = r
            .complete(req())
            .await
            .expect_err("glm 429s, kimi is capped");
        assert!(
            err.to_string().contains("429"),
            "a dispatch happened: {err}"
        );
        assert_eq!(
            kimi_calls.load(Ordering::SeqCst),
            0,
            "a saturated upstream is never dispatched under the hard cap"
        );
        assert_eq!(r.live[1].snapshot().0, 1, "kimi's slot count untouched");
    }

    #[tokio::test]
    async fn negative_soft_default_still_dispatches_saturated() {
        // Check-the-check for the case above: the default (soft, model-router 05)
        // falls over onto the saturated kimi and it serves.
        let (r, kimi_calls) = saturated_fallback(None);
        let resp = r.complete(req()).await.expect("soft mode dispatches");
        assert_eq!(resp.message.content_text(), "from-kimi");
        assert_eq!(kimi_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn negative_shed_spends_no_retry_budget() {
        // Every candidate is at its hard cap: shed at once — no dispatch, no
        // between-pass backoff even with a retry budget set.
        let (a, a_calls) = counting("a");
        let (b, b_calls) = counting("b");
        let events: Arc<std::sync::Mutex<Vec<String>>> = Arc::default();
        let sink = events.clone();
        let r = with_budget(
            router(
                vec![up_cap("a", a, 1), up_cap("b", b, 2)],
                prefer(&["a", "b"]),
            ),
            3,
        )
        .with_saturation(Some(Saturation::Shed), 0)
        .with_observer(Arc::new(move |ev| match ev {
            RouteEvent::SkippedSaturated { target } => {
                sink.lock().unwrap().push(format!("saturated:{target}"));
            }
            RouteEvent::Shed { role } => sink.lock().unwrap().push(format!("shed:{role}")),
            _ => {}
        }));
        r.live[0].in_flight.store(1, Ordering::Relaxed);
        r.live[1].in_flight.store(2, Ordering::Relaxed);
        let start = tokio::time::Instant::now();
        let err = r.complete(req()).await.expect_err("all saturated");
        assert!(err.to_string().contains("saturated"), "{err}");
        assert_eq!(start.elapsed(), std::time::Duration::ZERO, "no backoff");
        assert_eq!(a_calls.load(Ordering::SeqCst), 0);
        assert_eq!(b_calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            *events.lock().unwrap(),
            ["saturated:a", "saturated:b", "shed:main"],
            "one pass, one shed — the budget is not spent re-passing"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn corner_wait_admits_when_a_permit_frees() {
        // `wait`: kimi is at its cap; a slot frees at 100 ms, inside the 1 s
        // budget, so the request waits (bounded) and kimi serves it.
        let (kimi, kimi_calls) = counting("from-kimi");
        let r = Arc::new(
            router(vec![up_cap("kimi", kimi, 1)], prefer(&["kimi"]))
                .with_saturation(Some(Saturation::Wait), 1_000),
        );
        r.live[0].in_flight.store(1, Ordering::Relaxed);
        let r2 = r.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            r2.live[0].in_flight.store(0, Ordering::Relaxed); // the other call finished
        });
        let start = tokio::time::Instant::now();
        let resp = r.complete(req()).await.expect("admitted after the wait");
        assert_eq!(resp.message.content_text(), "from-kimi");
        assert_eq!(kimi_calls.load(Ordering::SeqCst), 1);
        let waited = start.elapsed();
        assert!(
            waited >= std::time::Duration::from_millis(100)
                && waited <= std::time::Duration::from_millis(125),
            "admitted on the first 25 ms tick after the slot freed, got {waited:?}"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn boundary_wait_times_out_then_sheds() {
        // The slot never frees: the wait is exactly its budget, then a shed.
        let (kimi, kimi_calls) = counting("from-kimi");
        let r = router(vec![up_cap("kimi", kimi, 1)], prefer(&["kimi"]))
            .with_saturation(Some(Saturation::Wait), 100);
        r.live[0].in_flight.store(1, Ordering::Relaxed);
        let start = tokio::time::Instant::now();
        let err = r.complete(req()).await.expect_err("never frees");
        assert!(err.to_string().contains("saturated"), "{err}");
        assert_eq!(start.elapsed(), std::time::Duration::from_millis(100));
        assert_eq!(kimi_calls.load(Ordering::SeqCst), 0);
        // A hostile wait budget is clamped to the 30 s ceiling.
        let (p, _) = counting("x");
        let r = router(vec![up("x", p)], prefer(&["x"]))
            .with_saturation(Some(Saturation::Wait), u64::MAX);
        assert_eq!(r.saturation_wait_ms, MAX_SATURATION_WAIT_MS);
    }

    #[tokio::test]
    async fn corner_streamed_hard_slot_held_until_drain() {
        // A streamed call keeps its hard slot until the stream drains/drops, so
        // a second stream sheds while the first is live and is admitted after.
        let r = router(
            vec![RouterUpstream {
                max_concurrency: 1,
                ..streamer("s", usize::MAX)
            }],
            prefer(&["s"]),
        )
        .with_saturation(Some(Saturation::Shed), 0);
        let first = r.stream(req()).await.expect("admitted");
        let err = r
            .stream(req())
            .await
            .err()
            .expect("slot held by the live stream");
        assert!(err.to_string().contains("saturated"), "{err}");
        drop(first);
        assert!(r.stream(req()).await.is_ok(), "slot released on drop");
    }

    /// A provider that parks every call on `gate` and records its own peak
    /// concurrency — the truth the router's admission must respect.
    struct GatedProvider {
        gate: Arc<tokio::sync::Semaphore>,
        inside: Arc<AtomicUsize>,
        peak: Arc<AtomicUsize>,
    }
    #[async_trait]
    impl LlmProvider for GatedProvider {
        fn capabilities(&self) -> ModelCapabilities {
            caps(true, false, 1000)
        }
        async fn complete(&self, r: CompletionRequest) -> Result<CompletionResponse> {
            let now = self.inside.fetch_add(1, Ordering::SeqCst) + 1;
            self.peak.fetch_max(now, Ordering::SeqCst);
            let _permit = self.gate.acquire().await.expect("gate open");
            self.inside.fetch_sub(1, Ordering::SeqCst);
            ScriptedProvider::new(vec![final_turn("ok")])
                .complete(r)
                .await
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn adversarial_concurrent_burst_never_exceeds_hard_cap() {
        // 10 concurrent calls race one upstream capped at 2. The CAS admission
        // lets exactly 2 in and sheds 8 — the provider never sees a 3rd.
        // Check-the-check: soft mode lets the whole burst through (peak 10).
        const BURST: usize = 10;
        for (saturation, admitted) in [(Some(Saturation::Shed), 2usize), (None, BURST)] {
            let gate = Arc::new(tokio::sync::Semaphore::new(0));
            let inside = Arc::new(AtomicUsize::new(0));
            let peak = Arc::new(AtomicUsize::new(0));
            let provider = Arc::new(GatedProvider {
                gate: gate.clone(),
                inside: inside.clone(),
                peak: peak.clone(),
            });
            let r = Arc::new(
                router(vec![up_cap("gpu", provider, 2)], prefer(&["gpu"]))
                    .with_saturation(saturation, 0),
            );
            let shed = Arc::new(AtomicUsize::new(0));
            let tasks: Vec<_> = (0..BURST)
                .map(|_| {
                    let (r, shed) = (r.clone(), shed.clone());
                    tokio::spawn(async move {
                        let out = r.complete(req()).await;
                        if out.is_err() {
                            shed.fetch_add(1, Ordering::SeqCst);
                        }
                        out
                    })
                })
                .collect();
            // Hold the gate until every call has either entered the provider or
            // been shed — the moment of maximum contention.
            tokio::time::timeout(std::time::Duration::from_secs(10), async {
                while inside.load(Ordering::SeqCst) + shed.load(Ordering::SeqCst) < BURST {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("every call resolved admission");
            gate.add_permits(BURST);
            let mut ok = 0;
            for t in tasks {
                match t.await.expect("task") {
                    Ok(_) => ok += 1,
                    Err(e) => assert!(e.to_string().contains("saturated"), "{e}"),
                }
            }
            assert_eq!(
                peak.load(Ordering::SeqCst),
                admitted,
                "{saturation:?}: peak"
            );
            assert_eq!(ok, admitted, "{saturation:?}: served");
            assert_eq!(r.live[0].snapshot().0, 0, "every slot released");
        }
    }

    // --- card-fed pricing (gap §8.2) ------------------------------------------

    /// Succeeds with a fixed `usage` (buffered) or one text chunk + a terminal
    /// chunk carrying `usage` (streamed) — the shape every real decoder emits.
    struct UsageProvider {
        usage: Usage,
    }
    #[async_trait]
    impl LlmProvider for UsageProvider {
        fn capabilities(&self) -> ModelCapabilities {
            caps(true, false, 1000)
        }
        async fn complete(&self, r: CompletionRequest) -> Result<CompletionResponse> {
            let mut resp = ok("y", true, false).complete(r).await?;
            resp.usage = Some(self.usage.clone());
            Ok(resp)
        }
        async fn stream(&self, _r: CompletionRequest) -> Result<ChunkStream> {
            let chunks = vec![
                Ok(agent_core::CompletionChunk {
                    delta_text: "y".into(),
                    ..Default::default()
                }),
                Ok(agent_core::CompletionChunk {
                    finish_reason: Some("stop".into()),
                    usage: Some(self.usage.clone()),
                    ..Default::default()
                }),
            ];
            Ok(Box::pin(futures_util::stream::iter(chunks)))
        }
    }

    fn usage(prompt: u32, completion: u32) -> Usage {
        Usage {
            prompt_tokens: prompt,
            completion_tokens: completion,
            total_tokens: prompt.saturating_add(completion),
            ..Default::default()
        }
    }

    /// An upstream serving `u`, carded at `input`/`output` USD per Mtok.
    fn priced(id: &str, u: Usage, input: f32, output: f32) -> RouterUpstream {
        let mut up = up(id, Arc::new(UsageProvider { usage: u }));
        up.input_cost = input;
        up.output_cost = output;
        up
    }

    async fn buffered_cost(r: &TaskRouter) -> Option<agent_core::Cost> {
        r.complete(req()).await.unwrap().usage.unwrap().cost
    }

    #[tokio::test]
    async fn positive_card_prices_stamp_buffered_usage() {
        // 1 Mtok in @ $2 + 1 Mtok out @ $8 = $10.
        let r = router(
            vec![priced("kimi", usage(1_000_000, 1_000_000), 2.0, 8.0)],
            prefer(&[]),
        );
        let c = buffered_cost(&r)
            .await
            .expect("card-priced turn carries a cost");
        assert_eq!((c.input, c.output, c.total), (2.0, 8.0, 10.0));
        assert_eq!((c.cache_read, c.cache_write), (0.0, 0.0));
    }

    #[tokio::test]
    async fn positive_stream_terminal_usage_gets_card_cost() {
        let r = router(
            vec![priced("kimi", usage(500_000, 250_000), 2.0, 8.0)],
            prefer(&[]),
        );
        let chunks: Vec<_> = r.stream(req()).await.unwrap().collect().await;
        let costs: Vec<_> = chunks
            .iter()
            .map(|c| c.as_ref().unwrap().usage.as_ref().and_then(|u| u.cost))
            .collect();
        // Only the terminal (usage-bearing) chunk is priced; text chunks untouched.
        assert!(costs[0].is_none());
        assert_eq!(costs[1].as_ref().map(|c| c.total), Some(3.0));
    }

    #[tokio::test]
    async fn negative_unpriced_card_leaves_cost_none() {
        // A $0/$0 card is "unknown", not "free": leave the cost unset so the
        // agent loop falls back to its own price table.
        let r = router(
            vec![priced("mi50", usage(1_000, 1_000), 0.0, 0.0)],
            prefer(&[]),
        );
        assert!(buffered_cost(&r).await.is_none());
    }

    #[tokio::test]
    async fn negative_provider_reported_cost_is_not_overwritten() {
        let reported = agent_core::Cost {
            total: 0.42,
            ..Default::default()
        };
        let mut u = usage(1_000_000, 1_000_000);
        u.cost = Some(reported);
        let r = router(vec![priced("a", u, 2.0, 8.0)], prefer(&[]));
        assert_eq!(buffered_cost(&r).await, Some(reported));
    }

    #[tokio::test]
    async fn corner_failover_prices_the_serving_upstream() {
        // A (expensive) fails over-ably; B (cheap) serves ⇒ B's rates apply.
        let mut a = up("a", Arc::new(FailProvider::new("http 429: slow down")));
        a.input_cost = 100.0;
        a.output_cost = 100.0;
        let b = priced("b", usage(1_000_000, 0), 1.0, 1.0);
        let r = router(vec![a, b], prefer(&["a", "b"]));
        assert_eq!(buffered_cost(&r).await.map(|c| c.total), Some(1.0));
    }

    #[tokio::test]
    async fn boundary_cached_tokens_not_double_billed() {
        // OpenAI-compat `prompt_tokens` already includes the cached hits, so a
        // card (no cache rate) bills them once, via the input line.
        let mut u = usage(1_000_000, 0);
        u.cache_read_tokens = 600_000;
        let r = router(vec![priced("a", u, 2.0, 0.0)], prefer(&[]));
        let c = buffered_cost(&r).await.unwrap();
        assert_eq!((c.input, c.cache_read, c.total), (2.0, 0.0, 2.0));
        // Output-only pricing still counts as a priced card.
        let r = router(
            vec![priced("b", usage(0, 1_000_000), 0.0, 3.0)],
            prefer(&[]),
        );
        assert_eq!(buffered_cost(&r).await.map(|c| c.total), Some(3.0));
    }

    #[tokio::test]
    async fn adversarial_hostile_card_costs_clamp() {
        for (input, output) in [
            (f32::NAN, f32::NAN),
            (-1.0, -5.0),
            (f32::INFINITY, f32::NEG_INFINITY),
        ] {
            let r = TaskRouter::new(
                vec![priced("x", usage(u32::MAX, u32::MAX), input, output)],
                prefer(&[]),
            )
            .unwrap();
            assert_eq!(
                (r.upstreams[0].input_cost, r.upstreams[0].output_cost),
                (0.0, 0.0),
                "{input}/{output} clamps on build"
            );
            // Clamped to an unpriced card ⇒ no (poisoned) cost is stamped.
            assert!(buffered_cost(&r).await.is_none(), "{input}/{output}");
        }
        // A huge-but-finite rate with max token counts stays finite.
        let r = router(
            vec![priced("x", usage(u32::MAX, u32::MAX), f32::MAX, f32::MAX)],
            prefer(&[]),
        );
        assert!(buffered_cost(&r).await.unwrap().total.is_finite());
    }

    // --- spillover tiers (gap §8.7 item 8) -----------------------------------

    type Events = Arc<std::sync::Mutex<Vec<String>>>;

    /// `cloud` (tagged "cloud", cap 2) is listed FIRST in the explicit order, so
    /// without spillover it would serve; `local` (cap 1) is the primary. With
    /// `spill_to = spill` the cloud is a reserve held back behind local.
    fn spill_fleet(
        saturation: Option<Saturation>,
        spill: &[&str],
    ) -> (TaskRouter, Arc<AtomicUsize>, Arc<AtomicUsize>, Events) {
        let (local, local_calls) = counting("from-local");
        let (cloud, cloud_calls) = counting("from-cloud");
        let mut policy = prefer(&["cloud", "local"]);
        policy.default_prefer.spill_to = spill.iter().map(|s| (*s).to_string()).collect();
        let events: Events = Arc::default();
        let sink = events.clone();
        let r = router(
            vec![
                RouterUpstream {
                    tags: vec!["cloud".into()],
                    ..up_cap("cloud", cloud, 2)
                },
                up_cap("local", local, 1),
            ],
            policy,
        )
        .with_saturation(saturation, 1_000)
        .with_observer(Arc::new(move |ev| {
            let e = match ev {
                RouteEvent::Spilled { role } => format!("spilled:{role}"),
                RouteEvent::SkippedSaturated { target } => format!("saturated:{target}"),
                RouteEvent::Shed { role } => format!("shed:{role}"),
                _ => return,
            };
            sink.lock().unwrap().push(e);
        }));
        (r, local_calls, cloud_calls, events)
    }
    const CLOUD: usize = 0;
    const LOCAL: usize = 1;

    #[tokio::test]
    async fn positive_reserve_is_held_back_while_a_primary_has_headroom() {
        for sat in [None, Some(Saturation::Shed), Some(Saturation::Wait)] {
            let (r, local, cloud, events) = spill_fleet(sat, &["cloud"]);
            let resp = r.complete(req()).await.expect("local serves");
            assert_eq!(resp.message.content_text(), "from-local", "{sat:?}");
            assert_eq!(
                cloud.load(Ordering::SeqCst),
                0,
                "{sat:?}: reserve untouched"
            );
            assert_eq!(local.load(Ordering::SeqCst), 1);
            assert!(events.lock().unwrap().is_empty(), "{sat:?}: no spill");
        }
        // Check-the-check: without `spill_to` the explicit order picks cloud.
        let (r, _, cloud, _) = spill_fleet(Some(Saturation::Shed), &[]);
        let resp = r.complete(req()).await.unwrap();
        assert_eq!(resp.message.content_text(), "from-cloud");
        assert_eq!(cloud.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn positive_spills_to_reserve_when_every_primary_is_saturated() {
        // Soft, shed and wait all spill once the primary is at its cap.
        for sat in [None, Some(Saturation::Shed), Some(Saturation::Wait)] {
            let (r, local, cloud, events) = spill_fleet(sat, &["cloud"]);
            r.live[LOCAL].in_flight.store(1, Ordering::Relaxed);
            let resp = r.complete(req()).await.expect("cloud absorbs overflow");
            assert_eq!(resp.message.content_text(), "from-cloud", "{sat:?}");
            assert_eq!(
                (local.load(Ordering::SeqCst), cloud.load(Ordering::SeqCst)),
                (0, 1)
            );
            assert_eq!(*events.lock().unwrap(), ["spilled:main"], "{sat:?}");
            assert_eq!(
                r.live[CLOUD].snapshot().0,
                0,
                "{sat:?}: reserve slot released"
            );
        }
    }

    #[tokio::test]
    async fn negative_primary_failure_does_not_spill() {
        // Spillover is capacity-driven, not error-driven: a primary WITH headroom
        // that 429s does not pull the reserve into the same pass.
        let (cloud, cloud_calls) = counting("from-cloud");
        let mut policy = prefer(&["local", "cloud"]);
        policy.default_prefer.spill_to = vec!["cloud".into()];
        let r = router(
            vec![
                up("local", Arc::new(FailProvider::new("http 429: slow down"))),
                RouterUpstream {
                    tags: vec!["cloud".into()],
                    ..up("cloud", cloud)
                },
            ],
            policy,
        )
        .with_saturation(Some(Saturation::Shed), 0);
        let err = r.complete(req()).await.expect_err("local 429s, no spill");
        assert!(err.to_string().contains("429"), "{err}");
        assert_eq!(cloud_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn corner_breaker_open_primary_spills() {
        // A primary whose breaker is open has no usable headroom either.
        let (r, local, cloud, events) = spill_fleet(None, &["cloud"]);
        r.health[LOCAL].record_failure((r.now_ms)(), 1);
        let resp = r.complete(req()).await.expect("cloud serves the outage");
        assert_eq!(resp.message.content_text(), "from-cloud");
        assert_eq!(
            (local.load(Ordering::SeqCst), cloud.load(Ordering::SeqCst)),
            (0, 1)
        );
        assert_eq!(*events.lock().unwrap(), ["spilled:main"]);
    }

    #[tokio::test]
    async fn corner_only_reserve_can_serve_is_used_without_spilling() {
        // A tool-call request only the reserve can take: it serves as primary —
        // spillover never refuses a request the fleet could answer.
        let mut policy = prefer(&["local", "cloud"]);
        policy.default_prefer.spill_to = vec!["cloud".into()];
        let events: Events = Arc::default();
        let sink = events.clone();
        let r = router(
            vec![
                up("local", ok("from-local", false, false)),
                RouterUpstream {
                    tags: vec!["cloud".into()],
                    ..up("cloud", ok("from-cloud", true, false))
                },
            ],
            policy,
        )
        .with_observer(Arc::new(move |ev| {
            if let RouteEvent::Spilled { role } = ev {
                sink.lock().unwrap().push(format!("spilled:{role}"));
            }
        }));
        let resp = r.complete(req_with_tools()).await.expect("cloud has tools");
        assert_eq!(resp.message.content_text(), "from-cloud");
        assert!(events.lock().unwrap().is_empty(), "not a spill");
    }

    #[tokio::test]
    async fn corner_spill_to_is_scoped_to_its_rule() {
        // Only the `judge` rule holds cloud in reserve; `main` (default prefer,
        // no spill_to) still takes cloud first by explicit order.
        let (cloud, _) = counting("from-cloud");
        let (local, _) = counting("from-local");
        let mut judge = role_rule(Role::Judge, &["cloud", "local"]);
        judge.prefer.spill_to = vec!["cloud".into()];
        let mut policy = prefer(&["cloud", "local"]);
        policy.rules.push(judge);
        let r = router(
            vec![
                RouterUpstream {
                    tags: vec!["cloud".into()],
                    ..up("cloud", cloud)
                },
                up("local", local),
            ],
            policy,
        );
        let main = r.complete(req()).await.unwrap();
        assert_eq!(main.message.content_text(), "from-cloud");
        let judged = hinted(
            req(),
            agent_core::RouteHint {
                role: Some(Role::Judge),
                ..Default::default()
            },
        );
        let resp = r.complete(judged).await.unwrap();
        assert_eq!(resp.message.content_text(), "from-local");
    }

    #[tokio::test]
    async fn corner_primary_filling_after_planning_spills_within_the_pass() {
        // The race the in-pass fallback covers: `order()` saw local with headroom
        // (reserve parked), then local filled before admission — the pass offers
        // the reserve instead of shedding.
        let (r, local, cloud, events) = spill_fleet(Some(Saturation::Shed), &["cloud"]);
        let plan = Plan {
            order: vec![LOCAL],
            reserve: vec![CLOUD],
            rule: None,
            spilled: false,
        };
        r.live[LOCAL].in_flight.store(1, Ordering::Relaxed); // filled since planning
        let hint = r.hint(&req());
        let op = |p: Arc<dyn LlmProvider>, _g: InFlightGuard, _c: Option<ModelPrices>| async move {
            p.complete(req()).await
        };
        let agent_retry::Attempt::Done(resp) = r.run_plan(&hint, &op, plan).await else {
            panic!("the reserve should have served");
        };
        assert_eq!(resp.message.content_text(), "from-cloud");
        assert_eq!(
            (local.load(Ordering::SeqCst), cloud.load(Ordering::SeqCst)),
            (0, 1)
        );
        assert_eq!(*events.lock().unwrap(), ["saturated:local", "spilled:main"]);
    }

    #[tokio::test]
    async fn boundary_reserve_also_full_sheds_without_dispatch() {
        let (r, local, cloud, events) = spill_fleet(Some(Saturation::Shed), &["cloud"]);
        r.live[LOCAL].in_flight.store(1, Ordering::Relaxed);
        r.live[CLOUD].in_flight.store(2, Ordering::Relaxed);
        let err = r.complete(req()).await.expect_err("whole fleet full");
        assert!(err.to_string().contains("saturated"), "{err}");
        assert_eq!(
            (local.load(Ordering::SeqCst), cloud.load(Ordering::SeqCst)),
            (0, 0)
        );
        assert_eq!(
            *events.lock().unwrap(),
            [
                "spilled:main",
                "saturated:cloud",
                "saturated:local",
                "shed:main"
            ]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn boundary_wait_admits_on_the_reserve_when_it_frees() {
        // Everything full; the bounded wait watches the reserve too, so a cloud
        // slot freeing at 100 ms is taken.
        let (r, _, cloud, _) = spill_fleet(Some(Saturation::Wait), &["cloud"]);
        let r = Arc::new(r);
        r.live[LOCAL].in_flight.store(1, Ordering::Relaxed);
        r.live[CLOUD].in_flight.store(2, Ordering::Relaxed);
        let r2 = r.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            r2.live[CLOUD].in_flight.store(1, Ordering::Relaxed);
        });
        let resp = r.complete(req()).await.expect("admitted after the wait");
        assert_eq!(resp.message.content_text(), "from-cloud");
        assert_eq!(cloud.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn adversarial_override_cannot_jump_the_spill_queue() {
        // A carried override naming the reserve is dropped while local has
        // headroom — a hint can't pull the request onto the paid tier.
        let over = |id: &str| {
            hinted(
                req(),
                agent_core::RouteHint {
                    override_upstream: Some(id.into()),
                    ..Default::default()
                },
            )
        };
        let (r, _, cloud, _) = spill_fleet(Some(Saturation::Shed), &["cloud"]);
        let resp = r.complete(over("cloud")).await.unwrap();
        assert_eq!(resp.message.content_text(), "from-local");
        assert_eq!(cloud.load(Ordering::SeqCst), 0);
        // Once local is full the reserve is fair game (the override agrees).
        r.live[LOCAL].in_flight.store(1, Ordering::Relaxed);
        let resp = r.complete(over("cloud")).await.unwrap();
        assert_eq!(resp.message.content_text(), "from-cloud");
        // Check-the-check: without spill_to the same override is honoured.
        let (r, _, _, _) = spill_fleet(Some(Saturation::Shed), &[]);
        let resp = r.complete(over("cloud")).await.unwrap();
        assert_eq!(resp.message.content_text(), "from-cloud");
        // And an override naming a primary still wins over the explicit order.
        let resp = r.complete(over("local")).await.unwrap();
        assert_eq!(resp.message.content_text(), "from-local");
    }
}
