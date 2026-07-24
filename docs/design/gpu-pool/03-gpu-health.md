# 03 — GPU health: latency EWMA + graded state

Status: **implemented** (latency EWMA + `PoolMemberState`) — see [`STATUS.md`](STATUS.md).
Turns binary `alive`/`dead` into a **graded** `healthy | degraded | dead` signal, so
the pool prefers a *fast* healthy target over a *slow* one — not just a live one over a
dead one. Request-level signals only; real GPU-utilization probing is a documented
hook, not built here.

> Implementation note: grading feeds selection as a **stable sort by grade** appended
> to `order()`, so it composes with every 01 policy uniformly (healthy before degraded,
> policy order preserved within each grade) rather than threading grade into each
> policy comparator. Off by default (`degraded_threshold_ms = 0`).

## Motivation

[01](01-load-balance.md) balances by in-flight count; [02](02-capacity.md) caps
concurrency. Both treat every alive target as equal. But an MI50 mid-swap answers in
40s while the GLM answers in 3s — same in-flight count, very different cost to route
to. The pool already **measures** per-call and per-probe latency (`duration_ms`); it
just throws it away. This increment keeps a rolling latency estimate per target and
lets it **grade** health, so a target that is alive-but-slow is de-prioritised before
it is marked dead.

## What already exists (and its gaps)

- `call_member` / `probe_all` already produce `duration_ms` per member
  (`pool.rs:128-183`) — the raw latency samples, currently only emitted as metrics and
  discarded from selection.
- The `Health` breaker (`router.rs:63-104`) gives binary alive/dead with
  `consecutive_failures` + cooldown.
- **Gap:** no rolling latency, no "degraded" state, no latency term in selection.
  `last_probe_ms` is a timestamp, not a duration estimate.

## Design

### Per-target latency EWMA

Each member keeps an **exponentially-weighted moving average** of call+probe latency
(an `AtomicU64` of millis, updated after each sample: `ewma = α·sample + (1-α)·ewma`).
Cheap, bounded, no history buffer. Seeded from the first probe; α configurable
(`latency_alpha`, default ~0.3), clamped `(0,1]`.

### Graded health state

A member's state derives from the breaker **and** the EWMA:

```rust
enum MemberState { Healthy, Degraded, Dead }
// Dead     = breaker open (unchanged from 01/02)
// Degraded = alive, but latency_ewma > degraded_threshold_ms  (config; e.g. 20s)
// Healthy  = alive and fast
```

`Degraded` is a **soft** signal: a degraded target is still eligible (it can still
serve), but it sorts **after** every healthy target under least-loaded/weighted — so
the pool drains to the fast targets first and only spills to the slow one under load.
This is the graceful "the MI50 is swapping, lean on the GLM" behaviour, distinct from
the hard "the MI50 is down, fail it out" the breaker already gives.

Selection order becomes: **(state: Healthy < Degraded) then the 01 policy** (in-flight
/ cost / weight). Dead stays filtered out entirely.

### Latency-aware failover

`complete()`'s ordered fallbacks put healthy-fast first, degraded-slow last — so a
single answer goes to the quickest live target, and only falls through to a slow one if
the fast ones fail or are saturated.

## Protobuf (additive — no baseline bump)

```proto
enum PoolMemberState { POOL_MEMBER_STATE_UNSPECIFIED = 0; HEALTHY = 1; DEGRADED = 2; DEAD = 3; }

message PoolMemberHealth {
  // … 01/02 fields …
  PoolMemberState state          = 10;  // additive
  uint32          latency_ms_ewma = 11; // additive
}
```

`convert.rs` both directions; a remote pool's `latency_ms_ewma` is clamped and
`state` defaults to deriving from `alive` when `UNSPECIFIED` (old server).

## gRPC interface

`LlmPoolService.Health` reports `state` + `latency_ms_ewma` per member — no new RPC. A
remote pool grades its own members; a client sees which targets are slow.

## Prometheus metrics

| Metric | Type | Labels |
|---|---|---|
| `agent_pool_member_state` | gauge | `member`, `state` |
| `agent_pool_member_latency_ewma_seconds` | gauge | `member` |

(The per-call latency **histogram** already lands in [01](01-load-balance.md); this
adds the smoothed EWMA gauge + the state gauge.)

## Tracing + logs

- `pool.select` gains `state` per considered member; the chosen member's state is on
  the span.
- `INFO` "member mi50 → degraded (latency_ewma=22s)" on a state transition
  (transitions only, not every call).

## Testing (table-driven + adversarial)

- `positive_` — a fast + a slow alive member: the fast one is preferred; the slow one
  is graded `Degraded` and still eligible.
- `negative_` — under the `degraded_threshold`, both stay `Healthy` and 01's policy
  alone decides.
- `corner_` — latency exactly at the threshold (boundary of Degraded); EWMA seeding
  from the first sample.
- `boundary_` — all members degraded ⇒ still served (degraded ≠ dead), ordered by the
  01 policy; a member crossing back under the threshold returns to `Healthy`.
- `adversarial_` (**mandatory**) — a **remote pool reporting a hostile
  `latency_ms_ewma` (huge / from an attacker) or an out-of-range `state`** is clamped /
  defaulted before it affects ordering (a slow-lie can at worst de-prioritise a target,
  never panic or exile a healthy one permanently); the EWMA update saturates (no
  overflow) on a `u64::MAX` sample.

## Benchmark + leak

- **Bench** — `eligible()` with the state grade + EWMA read over a fixed 8-member set.
  Absolute **Ir ceiling** (the EWMA is an atomic load + compare, so the delta over 01
  is tiny — the ceiling records it).
- **Leak** — repeated sample/update cycles hold no allocation (the EWMA is a single
  atomic); assert zero net leak.

## Security

- `latency_alpha`, `degraded_threshold_ms`, and any **remote-reported** EWMA/state are
  clamped/validated before use — a hostile latency report can only *soft*-shuffle
  order, never crash selection or permanently remove a good target (the breaker, not
  the grade, is what fails a target out, and it is failure-count-driven).

## Deferred (the documented hook — not built)

- **Real GPU-utilization probing.** An optional per-member `utilization_endpoint`
  (ollama `/api/ps`, an nvidia/DCGM exporter) polled by the existing probe task to feed
  a *measured* load/VRAM signal into the grade — instead of inferring load from
  latency + in-flight. The seam is ready (the probe task, the graded state, the
  additive health fields); wiring a concrete utilization client is a follow-up, so
  request-level signals ship first.
