# GPU/LLM-target pool — implementation status

The living tracker for the [GPU pool](README.md) design. One gated PR per increment,
based off `main` — do not stack. Each ships the full contract (proto/gRPC/metrics/
tracing/tests/bench/leak) and must pass `nix develop -c nix flake check`.

## Increments

| # | Increment | Seam | Wire | Metrics | Tests | Bench+Leak | Status |
|---|---|:--:|:--:|:--:|:--:|:--:|:--:|
| — | Design directory | — | — | — | — | — | **merged** |
| 01 | Load balance (in-flight + policy) | ✅ | ✅ | ✅ | ✅ | ✅ | **merged** |
| 02 | Capacity (concurrency cap + backpressure) | ✅ | ✅ | ✅ | ✅ | ✅ | **in review** |
| 03 | GPU health (latency EWMA + graded state) | — | — | — | — | — | designed |

## 02 — what shipped

- **Per-target `max_concurrency`** (config `[[pool.members]] max_concurrency`, `0` ⇒
  unbounded) enforced as an **admission check** in `eligible()` against 01's in-flight
  counter — a member at its cap is skipped like a dead one. No semaphore held across an
  await ⇒ no deadlock surface; the check-then-dispatch race self-corrects within one
  request (a soft QoS bound, not a safety invariant).
- **Backpressure** (`[pool] on_saturation`): `shed` (default) returns fewer/zero
  fan-out slots and a `saturated` error from `complete`; `wait` polls a **bounded**
  budget (`saturation_wait_ms`, clamped ≤ 30s, hard-capped tick count) for a permit,
  then sheds. Never an unbounded queue.
- **Contract**: additive `max_concurrency`/`saturated` on `PoolMemberHealth` + proto
  (no buf bump) + `convert.rs`; `agent_pool_member_saturated{member}` gauge (via the
  enriched `MemberState` event) + `agent_pool_saturation_shed_total` counter (new
  `SaturationShed` event); table-driven + adversarial tests (skip-at-cap, all-saturated
  sheds not hangs, bounded wait, hostile `wait_ms` clamped); the `pool_select` bench +
  dhat leak still hold. Config documented in `config/agent.toml`.
- **Security**: `max_concurrency`/`saturation_wait_ms` clamped; the all-saturated path
  is bounded + fail-soft by construction (admission never blocks on a held permit).

## 01 — what shipped

- **`PoolPolicy` {Cost, RoundRobin, LeastLoaded, Weighted}** (`agent-providers/pool.rs`),
  default `Cost` (back-compat). `eligible()` keeps its filter (healthy ∩ capable ∩
  `tier>=floor`) and orders the survivors by the policy — least-loaded = fewest
  `in_flight`, tie-break cost then index.
- **Per-member `in_flight: AtomicUsize`**, raised for a call's duration via an **RAII
  `InFlightGuard`** so it is released on every path — success, error, or a **panic**
  in the provider (proven by an adversarial test).
- **Struct-per-member config** — `[[pool.members]]` (untagged with the old
  `members = [...]` list, both parse) carrying endpoint/model/tier/weight/cost/
  max_concurrency; an inline `endpoint` synthesizes an `OpenAiCompatProvider` (one
  block per GPU target). `[pool] policy` selects the strategy.
- **Contract**: additive `in_flight`/`weight` on `PoolMemberHealth` + proto (no buf
  baseline bump) + `convert.rs` (remote-reported weight clamped);
  `agent_pool_member_inflight{member}` gauge, `agent_pool_member_latency_seconds{member}`
  histogram, `agent_pool_select_total{policy}` counter via the `PoolEvent` seam (new
  `MemberState` event, `Dispatch` gains `policy`); `pool_select` bench (Ir ceiling) +
  a dhat leak (in-flight released, no leak). Config documented in `config/agent.toml`.
- **Security**: `weight`/`cost` and remote-reported `in_flight`/`weight` clamped;
  selection panic-free (empty/all-dead/hostile numbers) and the guard deadlock-free.

## Build order = dependency order

- **01** adds the live in-flight signal + the pluggable policy + struct-per-member
  config. Everything else builds on the in-flight counter.
- **02** consumes 01's in-flight to enforce a per-target concurrency cap + backpressure.
- **03** grades health with a latency EWMA on top of 01/02's alive/capacity signals.

## Cross-cutting invariants (every increment)

- **Additive only** — new proto fields/policies, never a breaking change to
  `LlmPool`'s three methods or `LlmPoolService`'s two RPCs. `buf breaking` passes with
  no baseline bump.
- **`agent-providers` stays off `agent-metrics`** — new metrics ride the
  `PoolEvent` → `record_pool_event` seam.
- **The two consumers are untouched** — the mode-classifier vote
  (`complete_all(Light, fanout)`) and review summaries (`health()` + `summarize()`)
  keep their exact contracts.
- **Fail-soft + panic-free + deadlock-free** — every degenerate input (empty pool,
  all-dead, all-saturated, hostile remote numbers) has a defined graceful outcome; the
  in-flight/permit accounting releases on every path (RAII).
- **Back-compat** — `policy` defaults to `cost` (today's behaviour); the parallel
  `members`/`tiers` config lists keep working beside `[[pool.members]]`.

## Deferred (whole-design)

- Real GPU-utilization probing (ollama `/api/ps`, nvidia/DCGM) — the hook is designed
  in [03](03-gpu-health.md), not built.
- Aggregate autoscaling / cloud spillover when the whole pool is saturated.
- Learned per-target weights from `agent_pool_*` outcomes.
