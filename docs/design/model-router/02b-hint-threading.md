# 02b — `RouteHint` threading: per-call, task-aware routing end-to-end

Status: **planned** — see [`STATUS.md`](STATUS.md). The follow-on slice [02](02-routing.md)
deliberately carved out (its code says so: *"threading the classified task mode, per-call role,
and explicit override onto the request is a following slice"*). 02 shipped the **engine** —
`route::Policy::resolve` over `Hint`/`UpstreamMeta` — and a `TaskRouter` whose hint is derived
from request *shape* (tools/media presence) with one **fixed role per router instance**. 02b puts
the hint **on the request**, so every call carries its classified `TaskMode`, its **role**
(which internal subsystem is asking), and an optional explicit override — per call, not per
process.

> This is also where the model-router meets the **named-reference role routing** that shipped
> with the cognition graph (`[digest] provider`, `[instant] provider`, graph `provider` params /
> capability edges, all via `resolve_provider_ref`, `registry.rs:1088`). The two compose; the
> precedence contract is defined below.

## Motivation

After 02, `[agent] provider = "task-router"` routes — but every request looks the same to it:
the role is frozen at construction (`Role::Main`), `task_mode` doesn't exist as a signal, and
nothing ever sets `override_upstream`, `max_cost`, or a tier floor even though
`Policy::resolve` already honors all of them. Meanwhile the system classifies a `TaskMode`
every turn and holds it in `session.current_mode` a few lines above the dispatch
(`agent.rs:977`), and each internal caller (distiller, objective, verifier, judge, review)
knows exactly which role it is — the signals exist at the call sites and die there.

## What already exists (and its gaps)

- `route::{Role, Hint, Match, Prefer, Rule, Policy}` (`agent-providers/src/route.rs`) — the
  deterministic engine. `Hint` has `role/min_context/needs_vision/needs_tools/max_cost/tier/
  override_upstream`; **no `task_mode`**. `Match` is `role + min_context`; **no `task_mode`**.
- `TaskRouter` (`task_router.rs`) — fleet from `[[route.upstreams]]`, breaker-as-reorder,
  failover via the shared `RouteEvent` observer (`record_route_event`). `hint()` derives
  needs-tools/vision from the request; `role` fixed via `with_role`; **`min_context` never
  derived** (always 0 = unfiltered).
- `TaskMode` (`agent-core`) — classified every turn (`HybridClassifier`), in
  `session.current_mode`; already has a proto enum (`mode.proto`).
- `CompletionRequest` (core + `common.proto`) — **no routing field**; a request arriving over
  the provider seam carries no task/role signal at all.
- **Named-reference role routing** (cognition follow-ups, PR #232) — `[digest] provider`,
  `[instant] provider`, graph node `provider` param / capability edge pin a role slot to a
  *named* provider via `resolve_provider_ref`. Static pins; no policy, no live signals.
- Decision-level observability — none: `RouteEvent` covers dispatch (routed/fell-over/
  skipped/exhausted) but not *why* (matched rule, role, task mode).

## Design

### `RouteHint` on the request (additive, core + proto)

```rust
// agent-core
pub struct RouteHint {
    pub task_mode: Option<TaskMode>,       // from session.current_mode
    pub role: Option<RouteRole>,           // main|judge|classify|summarize|verify|review
    pub min_context: u32,                  // 0 = unset ⇒ router derives an estimate
    pub max_cost: Option<f32>,             // per-Mtok input-cost ceiling
    pub tier: Option<PoolTier>,            // tier floor
    pub override_upstream: Option<String>, // explicit id — filtered, never trusted
}
pub struct CompletionRequest { /* … */ pub route: Option<RouteHint> } // default None
```

`None` ⇒ exactly today's behaviour — the hot path is untouched when unset. `RouteRole` lives in
`agent-core` (the providers crate's `route::Role` becomes a re-export/alias) so runtime call
sites can stamp it without a providers dependency.

Note what is **not** on the hint: `needs_tools`/`needs_vision` stay **derived** from the request
itself (tools present, media present) and are OR'd with nothing — a hint cannot *clear* a
derived requirement, only the engine-internal `Hint` carries the result. A hostile hint
therefore can't steer a tool-call request onto a tool-less upstream.

### The engine gains the task-mode axis

- `route::Hint` and `route::Match` gain `task_mode: Option<TaskMode>`; a rule with a mode
  constraint fires only when the hint carries that mode (absent hint-mode ⇒ mode-constrained
  rules don't match — fail to the default, never guess).
- `[[route.rules]] match` gains `task_mode = "review" | "implement" | …` (parse via the
  existing `TaskMode` names; unknown ⇒ config error at build, not silently-never-matching).

### `TaskRouter` merges per-request + construction-time signals

`hint()` becomes: start from the request's `RouteHint` (if any), then

1. `role`: request's role, else the router's fixed `with_role` default (back-compat).
2. `needs_tools`/`needs_vision`: derived from the request (never from the hint).
3. `min_context`: hint value, else a **cheap chars/4 estimate** over the messages, clamped to
   `[0, 2_000_000]` — a floor filter only, fail-soft (an upstream with unknown window `0` is
   never filtered out, exactly as 02 built it).
4. `max_cost`/`tier`/`override_upstream`: from the hint, clamped/validated (below).

### Wiring the call sites (the role table)

The main loop stamps the full hint; each internal role slot stamps its role. Where a subsystem
holds an `Arc<dyn LlmProvider>` built once, the builder wraps it in a thin `RoleScoped`
provider (stamps `role` + passthrough when `req.route` is `None` — one struct, no seam change):

| Caller | Role | Stamped where |
|---|---|---|
| main loop | `Main` + `task_mode` from `session.current_mode` | `agent.rs` before `provider.complete` (`agent.rs:977`) |
| distiller summary/facts | `Summarize` | builder wraps the distill provider |
| instant objective/relevance | `Summarize` | builder wraps the `[instant]` provider |
| verifier (`llm`/`ensemble`) | `Verify` | builder wraps the verifier's provider |
| consensus critic / fork judge | `Judge` | builder wraps the critic/judge provider |
| review fan-out | `Review` | review builder wraps its provider |

The classifier vote is **not** in scope: it dispatches through `LlmPool::complete_all` with a
`PoolTier` (a pool-trait path, not a provider call) — moving it onto hints is
[04](04-registry-backed.md)'s escalation-hook territory.

### Composition with named-reference role routing (precedence contract)

Two mechanisms now serve role routing; they compose rather than compete:

1. **A named reference is a static pin and always wins.** `[digest] provider = "glm"` (or a
   graph capability edge) resolves that exact provider via `resolve_provider_ref` — no policy
   involved. This is the deterministic, config/graph-authored fast path.
2. **An unpinned role slot inherits the main provider.** When that is the `TaskRouter`, the
   slot's `RoleScoped` wrapper stamps its role and the **policy** decides — per call, against
   live breaker state.
3. **A named reference may itself be `"task-router"`** — pin the slot to the router, let policy
   route it under the stamped role. (The 02 rule "`[route]` upstreams must not include
   `task-router` itself" still holds — no self-recursion.)

## Threading — no seam change

`LlmProvider`/`LlmPool` signatures untouched. `RouteHint` is an additive request field;
`RoleScoped` is a private wrapper impl; the engine's structs gain optional fields.

## Protobuf (additive — no baseline bump)

`common.proto`: `CompletionRequest` gains `RouteHint route_hint = <next>` plus a new
`RouteHint` message and `RouteRole` enum (reusing `TaskMode` from `mode.proto`).
[03](03-registry-proto.md)'s `upstream.proto` **imports** these rather than defining its own.
`convert.rs` maps both directions; an old client sends no hint ⇒ `None`. Decode clamps
everything (below). `buf breaking` passes untouched.

## Prometheus metrics

| Metric | Type | Labels |
|---|---|---|
| `agent_router_decisions_total` | counter | `role`, `task_mode`, `chosen`, `rule` |
| `agent_router_no_candidate_total` | counter | `role` |

Via a new `RouteEvent::Decided { role, task_mode, rule, chosen }` variant (and `Exhausted`
gaining the role) through the existing observer → `record_route_event` bridge —
`agent-providers` stays off `agent-metrics`. `rule` is the matched rule's **index** (`rule0`…)
or `default` — bounded cardinality, no config text in a label.

## Tracing + logs

A `route.select {role, task_mode, rule, chosen, eligible}` span under the caller's span — ids
and enum names only, never the prompt. `DEBUG` one-liner: "route review/Judge → kimi (rule 1:
reasoning ∩ heavy) over glm".

## Testing (table-driven + adversarial)

- `positive_` — a request stamped `task_mode=Review` matches a mode rule; a `Judge`-role
  request routes to the judge-preferred upstream; `override_upstream` naming an eligible id
  wins; `RoleScoped` stamps only when `route` is `None` (an explicit per-call hint survives).
- `negative_` — hint absent ⇒ identical order to 02 (golden case); mode-constrained rule
  doesn't fire without a hint mode; a named-reference-pinned slot ignores policy entirely.
- `corner_` — hint role beats `with_role`; both mode and role rules match ⇒ first in order
  wins; estimate path: empty messages ⇒ `min_context = 0`.
- `boundary_` — chars/4 estimate exactly at an upstream's window; `max_cost` exactly at an
  upstream's `input_cost`.
- `adversarial_` (**mandatory**) — a hint with hostile numbers (NaN/negative/inf `max_cost`,
  `u32::MAX` `min_context`) is clamped and fails **soft** (defined empty-candidate error, no
  panic, no filter bypass); `override_upstream` naming an unknown / ineligible / over-long
  (>128 chars, rejected before comparison) id is **ignored**, not dialed; a hint cannot clear
  derived `needs_tools` (tool request never lands on a tool-less upstream); a wire-decoded
  hint with an out-of-range enum value maps to `None`/`Main`, never widens authority.

## Benchmark + leak

- **Bench** — extend `route_resolve` (50 upstreams, now 8 rules incl. mode+role matches +
  the estimate path); the Ir ceiling moves deliberately in `nix/checks/bench.nix` with the
  diff recording it.
- **Leak** — a routed `complete` with a stamped hint + failover hop frees its scratch
  (`RoleScoped` adds no per-call allocation beyond the stamped hint).

## Security

- **The hint narrows, never widens.** Every hint field is a *filter or preference* over the
  already-configured fleet; `override_upstream` selects only among **already-eligible**
  members; derived capability requirements cannot be cleared by a hint. A prompt-injected
  hint (or hostile gRPC peer) can at worst re-order/shed within the fleet.
- All hint numbers clamped at decode **and** again at resolve (defense in depth — the served
  provider seam makes `CompletionRequest` attacker-controlled).
- `override_upstream` is length-capped and compared by exact id — it never touches a path,
  a URL, or a log unsanitized.

## Deferred to later increments

- `prefer.policy` live-signal ordering (`cost | latency | least-loaded`) — needs the live
  in-flight/EWMA snapshot the registry consumption brings; lands with
  [04](04-registry-backed.md).
- The fleet + policy as gRPC-managed objects ([03](03-registry-proto.md)).
- Classifier-vote tier routing + escalation hook ([04](04-registry-backed.md)).
