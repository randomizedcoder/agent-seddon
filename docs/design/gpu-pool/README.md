# GPU/LLM-target pool — health-checked, capacity-aware load balancing

Status: **design / pre-implementation.** Built in gated per-increment PRs off `main`
(the adaptive-cognition rhythm). See [`STATUS.md`](STATUS.md).

## The problem

The agent is meant to lean on **many local LLM targets** — a heavy GLM-5.2 server, a
medium MI50 box, a small RTX card — and treat them as one **pool of GPU resources**:

> "If the MI50 goes offline (or is busy with other work), calls route to GLM-5.2, or
> vice versa. An abstraction layer, so we could have a pool of available GPU
> resources that get used."

Half of that already exists. The pool can tell whether a target is **alive**; it
cannot tell whether a target is **busy**, and it cannot **spread load**. This design
adds the load-balancing half on top of the health-checking half already in place.

## The local-model roster

| Target | Tier | Shape |
|---|---|---|
| GLM-5.2 (8×MI300) | heavy | big context, slow, expensive to occupy |
| MI50 32GB (ollama) | medium | one or two concurrent requests, cheap |
| small card (RTX) | light | fast, tiny, many concurrent slots |

Each is one `LlmProvider` (an `OpenAiCompatProvider` with its own `base_url`), grouped
into the pool as a member with a **tier** (a `>=` floor: `light < medium < heavy`).

## What already exists (extend, don't rebuild)

- **The `LlmPool` seam** — `agent-core/src/lib.rs:501-593`: `health()`,
  `complete_all(req, tier, fanout)` (fail-soft fan-out), `complete()` (failover).
  `PoolMemberHealth {name, tier, alive, consecutive_failures, last_probe_ms}`.
- **`PoolProvider`** — `agent-providers/src/pool.rs`: members with a tier + a static
  `cost`; a **circuit breaker** shared with the router (`router.rs:63-104`,
  `failure_threshold` / `cooldown`); an **active background probe** (a 1-token ping,
  `with_probe` / `probe_all`); passive mark-dead-on-error. Selection (`eligible`,
  `pool.rs:107-125`) = healthy ∩ capable ∩ `tier>=floor`, **sorted by static `cost`**,
  truncated to `fanout`.
- **Observability seam** — a typed `PoolEvent` (`pool.rs:27-48`) → `record_pool_event`
  (`metered.rs:1887-1923`) → `agent_pool_*` metrics. `agent-providers` deliberately
  does **not** depend on `agent-metrics`; the runtime maps events to metrics.
- **Wire** — `LlmPoolService {Health, Complete}` (`llm_pool.proto`), server + client,
  `agent --serve-llm-pool`. So the pool itself can run on another host.

## The gap

The health-check half is done; the **load-balancing half is not**:

- **No live load signal.** No per-target in-flight counter, no queue depth, no
  concurrency. Selection ignores current load entirely — it sorts by a *static* cost
  hint (which is hardcoded `0.0` today).
- **No load-balancing policy.** Only cheapest-first-deterministic. No least-loaded,
  round-robin, or weighted spread. (The single-pick `Router` has a `RoundRobin`
  policy the pool never got.)
- **No capacity.** No per-target max-concurrency, no backpressure, no admission
  control. Nothing stops the pool from piling every request onto one saturated MI50.
- **Binary health.** `alive`/`dead` only. `last_probe_ms` is captured but never used;
  there is no latency awareness, no "degraded" state.
- **Awkward config.** Parallel `members` / `tiers` string lists with no per-member
  endpoint, capacity, or weight — clumsy at "many targets" scale.

## Seam-extension strategy

Keep `LlmPool`'s three methods and the two consumers' expectations (`complete_all`'s
per-member fail-soft slots for the mode vote; `health()` for review). Everything is
**additive**:

- Live load and the selection **policy** live *inside* `PoolProvider` — the runtime
  and the seam signature are untouched.
- `PoolMemberHealth` gains fields (`in_flight`, `weight`, later `state` /
  `latency_ms_ewma`) so a **remote** pool exposes load over the wire — all new proto
  fields, so `buf breaking` passes with **no baseline bump**.
- Config gains a **struct-per-member** form (`[[pool.members]]`) beside the existing
  parallel lists, which keep working.

## The observability & quality contract (every increment)

Non-negotiable, per `CLAUDE.md`:

- **Protobuf** additive (buf-safe, no baseline bump); the existing `LlmPoolService`
  gains fields/RPCs, never breaks one.
- **Prometheus** via the `PoolEvent` → `record_pool_event` seam — new `agent_pool_*`
  families added the metered way; `agent-providers` stays off `agent-metrics`.
- **Tracing** — a `pool.select` span (policy, chosen member, in-flight) under the
  caller's span; never the prompt.
- **Tests** table-driven (`rstest`, all four prefix classes) with **mandatory
  `adversarial_`** cases (see security).
- **Bench** — an iai-callgrind Ir ceiling on the selection hot path (`eligible` runs
  on every call).
- **Leak** — a dhat budget (an in-flight guard must free its permit; no counter leak
  on panic/early-return).
- **Nix check** — a hermetic gate where one applies.

## Security stance (the model is untrusted — and so is a remote pool)

Pool selection runs on **every** LLM call, and when the pool is dialed as a remote
service (`= "grpc"`) its reported health/load comes from **another host** — untrusted.

- **Clamp hostile numbers** (NaN / negative / inf weights, in-flight, latency,
  capacity) to sane ranges before they enter selection math, a `sleep`, or a
  Prometheus `inc_by` — hostile inputs already cost this repo bugs.
- **Never panic** in selection: zero members, all-dead, all-saturated, a weight sum of
  zero — each has a defined fail-soft outcome, not a crash.
- **Never deadlock**: a per-member concurrency permit is bounded-wait → fail-soft, and
  is released on every path (RAII), including early-return and panic.
- The pool **degrades, never stalls**: exhaustion returns an error slot the caller
  reads, exactly like today's `complete_all`.

## Increments

| # | Increment | What it adds |
|---|---|---|
| **01** | [Load balance](01-load-balance.md) | struct-per-member config · per-target in-flight · pluggable policy (cost/round-robin/least-loaded/weighted) |
| **02** | [Capacity](02-capacity.md) | per-target `max_concurrency` · admission control / backpressure · saturation signal |
| **03** | [GPU health](03-gpu-health.md) | latency EWMA · graded `healthy\|degraded\|dead` state feeding selection |

Each is its own PR, based off `main` — **do not stack** (the code-review-flow lesson).

## Deferred (out of scope for this design)

- **Real GPU-utilization probing** — an optional per-member utilization endpoint
  (ollama `/api/ps`, an nvidia/DCGM exporter) as a health input. Request-level signals
  (in-flight, latency, breaker) come first; a hook is designed in `03`, not built.
- **Aggregate autoscaling / spillover to a cloud provider** when the whole pool is
  saturated.
- **Learned weights** — mine `agent_pool_*` to tune per-target weight from outcomes,
  rather than the configured value.
