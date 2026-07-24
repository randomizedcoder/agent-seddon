# 01 — Load balance: live in-flight + pluggable selection policy

Status: **design / pre-implementation.** The headline increment: turn the pool from
static cheapest-first into a **capacity-aware, load-balanced** router across many GPU
targets, and give it a config shape that scales.

## Motivation

Today every request goes to the same member — `eligible()` sorts by a static `cost`
that is hardcoded `0.0`, so the "cheapest" is just the first by index. Two turns fired
concurrently both land on the MI50 while the GLM sits idle. The pool has no idea
either target is busy. The fix is a **live load signal** plus a **selection policy**
that uses it.

## What already exists (and its gaps)

- `PoolMember` (`pool.rs:62-69`) — `{candidate, tier, cost: f32, health: Health,
  last_probe_ms}`. **No in-flight counter.**
- `PoolInner::eligible()` (`pool.rs:107-125`) — filters healthy ∩ capable ∩
  `tier>=floor`, sorts by `cost` asc (tie: index), truncates to `cap`. **The only
  selection logic; load-unaware.**
- `call_member()` (`pool.rs:128-156`) — times a call, updates the breaker, emits
  `PoolEvent::MemberCall`. **The natural inc/dec site for in-flight.**
- The `Router` has `RoutePolicy {InOrder, RoundRobin}` (`router.rs:41`); the pool has
  no policy enum. **The template to mirror.**
- `PoolCfg` (`config.rs:275-308`) — parallel `members` / `tiers` lists, no per-member
  endpoint / weight / capacity.

## Design

### Live in-flight

Add `in_flight: AtomicUsize` to `PoolMember`. Increment when a call is dispatched to
that member, decrement when it returns — via an **RAII guard** so an early-return, an
error, or a panic can never leak the count:

```rust
struct InFlightGuard<'a>(&'a AtomicUsize);
impl Drop for InFlightGuard<'_> {
    fn drop(&mut self) { self.0.fetch_sub(1, Ordering::AcqRel); }
}
// in call_member: let _g = InFlightGuard::enter(&member.in_flight); … provider.complete().await
```

The guard is the whole correctness story for load accounting; it is exercised by the
leak test (a panic mid-call must still decrement).

### Pluggable selection policy

A `PoolPolicy` enum mirroring `RoutePolicy`:

```rust
pub enum PoolPolicy { Cost, RoundRobin, LeastLoaded, Weighted }
```

`eligible()` keeps its **filter** (healthy ∩ capable ∩ `tier>=floor`) and changes only
the **ordering** of the survivors:

| Policy | Order survivors by |
|---|---|
| `Cost` | `cost` asc, then index — **today's behaviour (default, back-compat)** |
| `LeastLoaded` | `in_flight` asc, then `cost` asc, then index |
| `RoundRobin` | a rotating start cursor (`AtomicUsize`) modulo the survivor count |
| `Weighted` | weight-biased pick — higher `weight` chosen proportionally more often |

`fanout` still caps how many the fan-out touches. For the **single-answer**
`complete()` (failover), the head of the policy order is the primary and the rest are
the ordered fallbacks — so least-loaded gives "the idlest healthy capable target, then
the next idlest," which is exactly the MI50↔GLM spillover the user described.

Determinism: `Cost`, `LeastLoaded`, `RoundRobin` are deterministic given the same
state (important for the bench and reproducible tests). `Weighted` is the only
randomised one; it varies its pick by a counter (not a clock), so it stays
resume-safe and testable.

### Struct-per-member config (additive)

The new form, cleaner at scale and carrying per-member routing knobs:

```toml
[pool]
policy = "least-loaded"        # cost | round-robin | least-loaded | weighted (default: cost)

[[pool.members]]
name = "mi50"
endpoint = "http://mi50:11434/v1"   # inline → synth an OpenAiCompatProvider
tier = "medium"
weight = 1.0
# max_concurrency lands in 02

[[pool.members]]
name = "glm"                        # no endpoint → resolve via the provider registry
tier = "heavy"
weight = 4.0
```

The builder (`builder.rs:558-617`) accepts **either** form: `[[pool.members]]` if
present, else the parallel `members`/`tiers` lists (unchanged). An inline `endpoint`
makes an `OpenAiCompatProvider` directly — so adding a GPU target is one config block,
no separate `[provider.*]` stanza. `weight`/`cost` are clamped on read (see security).

## Threading — no seam change

`LlmPool`'s methods are untouched. In-flight and policy live entirely inside
`PoolProvider`. The only outward-facing change is **additive fields on
`PoolMemberHealth`** so a remote pool can expose load.

## Protobuf (additive — no baseline bump)

```proto
message PoolMemberHealth {
  string name = 1;
  PoolTier tier = 2;
  bool alive = 3;
  uint32 consecutive_failures = 4;
  uint32 last_probe_ms = 5;
  uint32 in_flight = 6;     // additive
  float  weight   = 7;      // additive
}
```

`convert.rs:2381-2415` both directions; `in_flight`/`weight` default to `0` for an old
server. A remote pool's reported `in_flight`/`weight` is **clamped on receipt**.

## gRPC interface

The existing `LlmPoolService.Health` now carries `in_flight`/`weight` per member — no
new RPC. `Complete` (fan-out) is unchanged. A remote `= "grpc"` pool therefore
load-balances the same way a local one does; a client reading `Health` sees live load.

## Prometheus metrics

| Metric | Type | Labels |
|---|---|---|
| `agent_pool_member_inflight` | gauge | `member` |
| `agent_pool_member_latency_seconds` | histogram | `member` |
| `agent_pool_select_total` | counter | `policy`, `member` |

Raised via a new/enriched `PoolEvent` (e.g. `Select {policy, member, in_flight}`)
mapped in `record_pool_event` — `agent-providers` stays off `agent-metrics`. (Today's
`MemberCall.duration_ms` folds into a member-*un*labelled dispatch histogram; the new
per-member latency histogram fixes that gap.)

## Tracing + logs

- A `pool.select {policy, chosen, in_flight, eligible}` span under the caller's span
  (the mode vote's `mode.classify`, a turn's `agent.turn`). Counts only.
- `DEBUG` "select least-loaded: mi50 (in_flight=0) over glm (in_flight=2)".

## Testing (table-driven + adversarial)

- `positive_` — one case **per policy**: given three members with set in-flight/cost/
  weight, assert the chosen order. Least-loaded picks the idlest; round-robin rotates;
  weighted's distribution over N picks is within tolerance; cost = index order.
- `negative_` — one member → always chosen regardless of policy; a dead member is
  filtered before ordering.
- `corner_` — all equal load → least-loaded falls back to cost then index
  (deterministic); round-robin wraps at the survivor count.
- `boundary_` — `fanout` larger than the survivor set; a single survivor under each
  policy.
- `adversarial_` (**mandatory**) — a **remote pool reporting NaN / negative / huge
  `in_flight` or `weight`** is clamped before it orders selection (no panic, no
  overflow); a `weight` sum of zero falls back to uniform; an **empty pool** returns an
  empty selection, not a panic; the in-flight guard **decrements on a panicking call**
  (no leaked load).

## Benchmark + leak

- **Bench** (`iai-callgrind`) — `eligible()` over a fixed 8-member set under
  `LeastLoaded` (the per-call hot path). Absolute **Ir ceiling**; the sort + in-flight
  reads are what it guards. Keep it allocation-light.
- **Leak** (`dhat`) — N dispatches (including a panicking one) leave `in_flight` at 0
  for every member and free the selection scratch; assert zero net leak + a budget.

## Security

- `weight`, `cost`, and any **remote-reported** `in_flight` are clamped
  (`0.0..=MAX`, finite; `NaN`/`inf` → a safe default) before selection math.
- Selection is total: every degenerate input (empty, all-dead, zero-weight-sum) has a
  defined non-panicking result.
- The in-flight guard releases on all paths (the leak test proves it), so a hostile
  target that hangs then errors cannot permanently inflate a member's load and exile
  it from selection.

## Deferred to later increments

- `max_concurrency` **enforcement** (the config field may be *parsed* here but is
  inert until [02](02-capacity.md)).
- Latency-EWMA-aware ordering and graded health ([03](03-gpu-health.md)) — 01 uses
  binary alive/dead + in-flight only.
