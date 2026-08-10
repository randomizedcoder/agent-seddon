# 02 — Task-aware routing: `RouteHint`, `TaskRouter`, declarative policy

Status: ✅ **merged** (PR #229) — **as the engine slice only**; see [`STATUS.md`](STATUS.md).
Builds on [01](01-metadata.md)'s metadata.
This is the increment where the layer starts **routing by task**: a per-request `RouteHint`, a
declarative `RoutePolicy` you author, and a `TaskRouter` `LlmProvider` that resolves the two
against 01's model cards + the live pool signals.

> **As built** (`agent-providers/src/route.rs` + `task_router.rs`): the deterministic
> `Policy::resolve` engine, `[route]` TOML (`[[route.upstreams]]` + `[[route.rules]]` +
> `[route.default_prefer]`), and the `TaskRouter` wired as `[agent] provider = "task-router"` —
> but the hint is **derived from request shape** (tools/media presence) with a **fixed role per
> router**, `Match` has no `task_mode`, `Prefer` has no live-signal `policy` ordering, and
> `RouteHint` is **not** on `CompletionRequest`. The threading below (§"`RouteHint` on the
> request", §"Wiring the hint at the call sites") is specified in
> [02b](02b-hint-threading.md); live-signal ordering moved to [04](04-registry-backed.md).
> The breaker is a reorder-to-back, not a hard filter — a dead upstream is tried last.

> Still **in-process + TOML** — the policy lives in `[route]` config; [03](03-registry-proto.md)
> lifts both the fleet and the policy into the gRPC control plane. Here we prove the routing
> *mechanism* against the existing pool members.

## Motivation

The system classifies a `TaskMode` every turn and holds it in `session.current_mode` right where
it calls the model (`agent.rs:893`) — but the main loop routes to a **single fixed provider**,
ignoring mode, role, and request requirements. There is nowhere on a request to say "this needs a
128k coding model" or "route this classification vote to the cheap one." 02 adds that.

## What already exists (and its gaps)

- `CompletionRequest` (`lib.rs:88`) — `{messages, tools, max_tokens, temperature,
  response_format}`; **no task/role/requirement field.**
- `TaskMode` (`lib.rs:3490`) + `HybridClassifier` + `session.current_mode` (`session.rs:54`,
  updated each turn before `agent.rs:893`). Only `Review` forks behavior today.
- `Router` (`router.rs`) — failover chain, `is_capable` + policy order; **no task awareness.**
- `PoolInner::eligible/order` (`pool.rs:314/334`) — filter + policy order; **no rule matching.**
- Internal model-callers that reuse the main provider: compaction summarizer
  (`summarizing.rs:147`), verifier (`llm.rs:105`), dimensions (`dimensions.rs:140`), structured
  (`structured.rs:58`), classifier vote (`classifier.rs:67`), review fan-out (`summaries.rs:175`).
  Each is a **role** that should route independently.

## Design

### `RouteHint` on the request (additive)

```rust
pub struct RouteHint {
    pub task_mode: Option<TaskMode>,     // from session.current_mode
    pub role: Role,                      // main|judge|classify|summarize|verify|review
    pub min_context: u32,                // 0 = derive from message tokens
    pub needs_vision: bool,
    pub needs_tools: bool,
    pub max_cost: Option<f32>,           // per-Mtok ceiling
    pub latency_target_ms: Option<u32>,
    pub override_upstream: Option<String>,  // explicit id → bypass the policy
}
pub struct CompletionRequest { /* … */ pub route: Option<RouteHint> }  // additive, default None
```

`None` ⇒ today's behaviour exactly (the hot path is untouched when unset). `needs_tools`/
`needs_vision` are also *derivable* from the request (tools present, media present) — the hint
lets a caller assert them up front, and hardens `is_capable`.

### Declarative `RoutePolicy` — "tell it how to route"

An ordered rule list in `[route]` config (TOML now, proto in 03):

```toml
[route]
default_policy = "cost"          # applied when no rule's `prefer.policy` is set

[[route.rules]]                  # long prompts → only models that fit, cheapest first
match  = { min_context = 32000 }
prefer = { tags = ["long-context"], policy = "cost" }

[[route.rules]]                  # code review → a strong reasoning model
match  = { task_mode = "review" }
prefer = { tags = ["reasoning"], tier = "heavy" }

[[route.rules]]                  # the cheap classification vote
match  = { role = "classify" }
prefer = { tier = "light", policy = "least-loaded" }
```

Resolution (`RoutePolicy::resolve(hint, cards, live) -> ordered upstream ids`):

1. **Filter** — `enabled ∩ healthy ∩ is_capable ∩ context_window ≥ min_context ∩ tier-floor ∩
   cost ≤ max_cost`. If `override_upstream` is set and passes the filter, it wins outright.
2. **Match** — the first `RouteRule` whose `match` all holds (task_mode/role/min_context/tags);
   no rule → the default.
3. **Order** — the survivors by the rule's `prefer` (tags boost, tier, explicit `upstreams`
   order) then the `prefer.policy` (cost | latency | weighted | least-loaded) resolved against
   **live signals** (health grade, `in_flight`, latency EWMA, cost). This is "it decides".

Matching is **deterministic** and cheap (no model call) — the decision engine chosen for this
design. An LLM meta-router and learned weights are deferred (see the [README](README.md)).

### `TaskRouter` — the drop-in `LlmProvider`

```rust
pub struct TaskRouter { registry: Fleet, policy: RoutePolicy, /* breaker, observer */ }
#[async_trait] impl LlmProvider for TaskRouter {
    fn capabilities(&self) -> ModelCapabilities { /* union: max window, any-tools/vision */ }
    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse> {
        let order = self.policy.resolve(req.route.as_ref(), &self.cards(), &self.live());
        // try in order with the SAME failover safety as Router (retryable → next, terminal → stop)
    }
}
```

It *is-a* `LlmProvider`, so `[agent] provider = "task-router"` drops it into `self.provider` at
`agent.rs:893` — the loop routes without changing. Every in-process caller that passes a `role`
(summarizer/verifier/…) then routes for free. Failover reuses `agent_retry::classify` and the
shared breaker exactly like `Router` (`router.rs`) — only retryable failures fall to the next
upstream; terminal/unknown stop.

### Wiring the hint at the call sites

- **Main loop** (`agent.rs:893`) — fill `req.route = RouteHint { task_mode:
  Some(self.current_mode), role: Main, min_context: estimate(&req), needs_tools: !req.tools
  .is_empty(), … }` before `self.provider.complete(req)`.
- **Classifier vote** (`classifier.rs:67`) — `role: Classify` (drives the light tier), replacing
  the hardcoded `PoolTier::Light`.
- **Summarizer / verifier / dimensions / structured** — `role: Summarize|Verify|…`, so they can
  target a cheap or long-context model instead of always the main provider.

## Threading — no seam change

`LlmProvider`/`LlmPool` signatures unchanged. `RouteHint` is an additive request field;
`TaskRouter` is a new impl selected by config; `is_capable`/`order` gain rule awareness.

## Protobuf (additive — no baseline bump)

`common.proto` `CompletionRequest` gains `RouteHint route = N` (a new message: `task_mode`
reusing `TaskMode` from `mode.proto`, `role` enum, the requirement fields, `override_upstream`).
Additive field + new message ⇒ `buf breaking` passes untouched. `convert.rs` both directions;
an old client sends no hint ⇒ `None`.

## Prometheus metrics

| Metric | Type | Labels |
|---|---|---|
| `agent_router_decisions_total` | counter | `role`, `task_mode`, `chosen`, `rule` |
| `agent_router_no_candidate_total` | counter | `role` (every filter rejected all upstreams) |

Via a new `RouterEvent` mapped in `record_pool_event`'s neighbour — `agent-providers` stays off
`agent-metrics`.

## Tracing + logs

- A `route.select {role, task_mode, rule, chosen, eligible}` span under the caller's span; never
  the prompt.
- `DEBUG` "route review→heavy: chose kimi (reasoning, window≥N) over glm (cost)".

## Testing (table-driven + adversarial)

- `positive_` — one case per routing axis: `task_mode=review` → reasoning/heavy upstream;
  `role=classify` → light; `min_context=64000` → the only member that fits; `override_upstream`
  wins when eligible; `max_cost` excludes the pricey one.
- `negative_` — no rule matches → default policy; `override_upstream` that fails the filter is
  **ignored**, not forced.
- `corner_` — two rules match → first wins (order matters); empty policy → pure default.
- `boundary_` — `min_context` exactly the window; a single eligible upstream under every rule.
- `adversarial_` (**mandatory**) — an `override_upstream` naming an **unknown / disabled** id is
  rejected (no dispatch to a non-member); a hint with hostile numbers (huge `min_context`,
  negative `max_cost`) is clamped and fails **soft** (no candidate → a defined error, not a
  panic); a rule referencing a non-existent tag simply matches nothing.

## Benchmark + leak

- **Bench** — `RoutePolicy::resolve` over 50 cards + 8 rules (the new per-call hot path). An
  absolute Ir ceiling; keep it allocation-light (filter in place, no per-call `Vec<String>`
  clones — tags compared by ref).
- **Leak** — a routed `complete()` including a failover hop frees its scratch; the in-flight
  guard still nets zero.

## Security

- **`override_upstream` is filtered, never trusted** — a model-supplied override can only select
  an *already-eligible* member; it can never dial an arbitrary URL.
- Hint numbers (min_context/max_cost/latency) clamped before filter math.
- Routing is total: every degenerate case (no candidate, all-filtered, conflicting rules) has a
  defined fail-soft result. Failover honours the same terminal-vs-retryable classification as
  `Router` — an auth/billing failure never fans out to another upstream.

## Deferred to later increments

- **Hint threading** — `RouteHint` on `CompletionRequest`, the `task_mode` axis, per-call roles,
  override wiring, decision metrics ([02b](02b-hint-threading.md)).
- The fleet + policy as gRPC-managed objects ([03](03-registry-proto.md)) — 02 reads TOML `[route]`.
- Live-signal `prefer.policy` ordering, per-role wiring at scale + per-upstream metrics
  ([04](04-registry-backed.md)).
