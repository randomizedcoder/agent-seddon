# GPU/LLM-target pool — implementation status

The living tracker for the [GPU pool](README.md) design. One gated PR per increment,
based off `main` — do not stack. Each ships the full contract (proto/gRPC/metrics/
tracing/tests/bench/leak) and must pass `nix develop -c nix flake check`.

## Increments

| # | Increment | Seam | Wire | Metrics | Tests | Bench+Leak | Status |
|---|---|:--:|:--:|:--:|:--:|:--:|:--:|
| — | Design directory | — | — | — | — | — | **this PR** |
| 01 | Load balance (in-flight + policy) | — | — | — | — | — | designed |
| 02 | Capacity (concurrency cap + backpressure) | — | — | — | — | — | designed |
| 03 | GPU health (latency EWMA + graded state) | — | — | — | — | — | designed |

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
