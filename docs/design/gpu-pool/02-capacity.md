# 02 — Capacity: per-target concurrency cap + backpressure

Status: **implemented** (`max_concurrency`, `Saturation` {Shed, Wait}) — see
[`STATUS.md`](STATUS.md). Consumes [01](01-load-balance.md)'s in-flight counter: turn
"prefer the idle target" into "**never overload** a target," with a real per-GPU
concurrency limit and graceful backpressure.

> Implementation note: admission is an in-flight `< cap` **check** inside `eligible()`,
> not a semaphore held across the await — so there is no lock-across-`.await` and no
> deadlock surface. `wait` is a bounded poll loop (hard-capped tick count), never an
> unbounded queue.

## Motivation

Least-loaded ([01](01-load-balance.md)) spreads work, but nothing *stops* it piling on
when every target is busy. An MI50 that serves one request at a time will thrash if
the pool dispatches four. Each GPU target has a real **capacity** — a number of
concurrent requests it serves well — and the pool must respect it: skip a saturated
target, and when *all* eligible targets are full, degrade gracefully rather than
dogpile.

## What already exists (and its gaps)

- 01's `in_flight: AtomicUsize` per member — the live-load signal capacity builds on.
- `PoolSpec {candidate, tier, cost}` (`pool.rs:56-60`) and the `[[pool.members]]`
  config — the place a `max_concurrency` field lands.
- `complete_all` / `complete` (`pool.rs:290-347`) — fan-out and failover, both already
  **fail-soft** (a dead member → an error slot). Capacity extends the same
  fail-soft-ness to a *saturated* member.
- **Gap:** no admission control, no permit, no saturation state. `fanout` caps the
  *count* touched, not the *aggregate live capacity*.

## Design

### Per-target concurrency cap

Each member gets an optional `max_concurrency` (config; `None`/`0` ⇒ unbounded, the
default). Enforced as an **admission check against 01's in-flight counter**, not a
blocking semaphore held across the await — so there is no lock across `.await` and no
deadlock surface:

```rust
// admission, checked during selection
fn has_capacity(m: &PoolMember) -> bool {
    match m.max_concurrency {
        Some(cap) => m.in_flight.load(Acquire) < cap.get(),  // cap is NonZeroUsize
        None => true,
    }
}
```

Selection ([01](01-load-balance.md)'s `eligible`) gains one more filter: a member at
capacity is **skipped** (like a dead one). Ordering among the admitted survivors is
unchanged (least-loaded/weighted/…). The check-then-dispatch race is benign: the
in-flight guard makes a small transient overshoot self-correct within one request, and
the cap is a soft QoS bound, not a safety invariant, so a briefly-exceeded cap is
acceptable — documented, not a bug.

### When everything is saturated

If no eligible member has capacity, the pool does **not** queue unboundedly. Two
defined outcomes (config: `on_saturation = "shed" | "wait"`, default `shed`):

- **`shed`** (default) — return a fail-soft slot with `error = "saturated"` (a class
  string, like today's dead-member slot). The caller (mode vote, review) already
  tolerates a fail-soft slot, so this is a graceful degrade, never a stall.
- **`wait`** — a **bounded** wait (`saturation_wait_ms`, clamped) for a permit to free,
  then re-select; on timeout, fall through to `shed`. Never an unbounded queue.

`complete()` (failover) walks the capacity-admitted order; exhaustion (all saturated)
returns the `saturated` error, exactly like all-dead today.

### Aggregate-capacity-aware fan-out

`complete_all` dispatches to at most `min(fanout, admitted-with-capacity)` members —
so a fan-out of 3 against one idle + two saturated targets fans out to one, rather than
forcing work onto full targets. The mode vote degrades to fewer voters gracefully
(fail-soft slots), preserving its plurality-tally contract.

## Protobuf (additive — no baseline bump)

```proto
message PoolMemberHealth {
  // … 01 fields …
  uint32 max_concurrency = 8;   // additive; 0 ⇒ unbounded
  bool   saturated       = 9;   // additive; in_flight >= max_concurrency
}
```

`convert.rs` both directions; a remote pool's reported `max_concurrency` is clamped on
receipt.

## gRPC interface

`LlmPoolService.Health` now reports `max_concurrency` + `saturated` per member — no new
RPC. A remote pool enforces its own caps; a client sees which targets are full.

## Prometheus metrics

| Metric | Type | Labels |
|---|---|---|
| `agent_pool_member_saturated_total` | counter | `member` |
| `agent_pool_saturation_shed_total` | counter | — (all-saturated → shed) |
| `agent_pool_member_capacity` | gauge | `member` |

Via `PoolEvent` variants mapped in `record_pool_event`.

## Tracing + logs

- The `pool.select` span gains `admitted`, `saturated`, `action = dispatch|wait|shed`.
- `WARN` "pool saturated: all N eligible members at capacity → shed" (rate-limited).

## Testing (table-driven + adversarial)

- `positive_` — a member at `in_flight == cap` is skipped; work goes to a member with
  spare capacity; freeing a permit re-admits it.
- `negative_` — `max_concurrency = None` ⇒ never skipped (unbounded, today's
  behaviour).
- `corner_` — exactly at cap (skip) vs one below (admit); `fanout` shrinks to the
  admitted-with-capacity count.
- `boundary_` — every member saturated ⇒ `shed` returns a `saturated` slot (not a
  hang); `wait` mode times out then sheds.
- `adversarial_` (**mandatory**) — `max_concurrency = 0` / `usize::MAX` from config or
  a remote pool is clamped to a sane range; a **thundering herd** (100 concurrent
  dispatches, all targets tiny) **sheds fail-soft and never deadlocks or unbounded-
  queues**; a **panicking call** frees its permit (no permanent capacity loss).

## Benchmark + leak

- **Bench** — `eligible()` with the capacity filter over a fixed 8-member set (the
  admission check is per call). Absolute **Ir ceiling**.
- **Leak** — saturate + release cycles (incl. a panicking call and a `wait` timeout)
  leave every `in_flight` at 0 and hold no permits; assert zero net leak + a budget.

## Security

- `max_concurrency` and `saturation_wait_ms` are clamped (`0`⇒unbounded;
  `wait_ms ∈ [0, MAX]`) before any admission or `sleep` — a hostile config/remote
  cannot force an unbounded wait or a zero-permit lockout.
- The **all-saturated path is bounded and fail-soft** by construction — a target that
  hangs then errors cannot wedge the pool, because admission never blocks on a permit
  held across an await.

## Deferred to later increments

- Latency-EWMA-aware **degraded** state ([03](03-gpu-health.md)) — 02's capacity is a
  hard count; 03 adds a soft "slow but alive" de-prioritisation on top.
