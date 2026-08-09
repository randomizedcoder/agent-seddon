# LLM/GPU pool — the `LlmPool` seam

Groups several model endpoints — a heavy GLM, a medium MI50 box, a small card —
into one **health-checked, load-balanced pool of GPU
resources**. It routes around a target that is **offline** (an active liveness probe
+ circuit breaker) *or* **busy** (in-flight-aware selection + per-target capacity),
and prefers a **fast** target over a **slow** one (latency-graded health). Fails
**soft**: a dead or saturated member is a slot in the result, never a batch failure.

- **Trait:** `agent_core::LlmPool` ([`agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs))
  — `health()`, `complete_all(req, tier, fanout)` (parallel fan-out), `complete()`
  (single-answer failover).
- **Impl crate:** [`agent-providers`](../../crates/agent-providers) (`PoolProvider`),
  gated by `provider-pool`; each member is an `LlmProvider` (typically
  `OpenAiCompatProvider`).
- **Config:** `[pool]` — off when no members are set.
- **Service:** `agent --serve-llm-pool` (`agent.v1.LlmPoolService`: `Health` +
  `Complete`).
- **Consumers:** the [mode](mode.md) classifier's light-tier vote and the
  [code-review](../design/code-review/README.md) summaries fan-out.

## Selection policy

`health ∩ capability ∩ tier-floor ∩ has-capacity` filters the members; the
**policy** then orders the survivors (config `[pool] policy`):

| Policy | Orders by |
|---|---|
| `cost` (default, back-compat) | configured cost, cheapest first |
| `least-loaded` | fewest in-flight requests — best for a heterogeneous GPU pool |
| `round-robin` | a rotating cursor across the survivors |
| `weighted` | biased by each member's configured `weight` |

`tier` is a floor (`light < medium < heavy`): a heavy job demands a heavy member; a
cheap classification vote accepts anything. Live in-flight load is tracked per member
via an RAII guard that releases on every path (including a panic in the provider).

## Capacity + backpressure

Each member takes an optional `max_concurrency` (`0` = unbounded). A member at its
cap is **skipped** in selection (an admission check, not a lock held across the
await — no deadlock surface). When every eligible member is saturated,
`[pool] on_saturation` decides: `shed` (default — fewer/zero fan-out slots, a
`saturated` error from `complete`) or `wait` (a **bounded** poll for a permit, then
shed). Never an unbounded queue.

## Graded health

A per-member latency EWMA (from real calls + probes) grades each member
`healthy | degraded | dead`: dead = breaker open; degraded = alive but slower than
`degraded_threshold_ms`. Grading feeds selection as a stable sort by grade — a
healthy member sorts before a degraded one, the policy order preserved within each
grade. Degraded ≠ dead: still eligible, just de-prioritised. `0` disables grading
(the default).

## Config

```toml
[pool]
policy               = "least-loaded"   # cost | least-loaded | round-robin | weighted
on_saturation        = "shed"           # shed | wait
saturation_wait_ms   = 500              # bounded wait budget for `wait` (clamped <= 30s)
degraded_threshold_ms = 0               # e.g. 20000 to demote a target slower than 20s
latency_alpha        = 0.3              # EWMA smoothing factor in (0,1]
probe_interval_secs  = 15               # active liveness probe cadence (0 disables)

[[pool.members]]
name = "mi50"
endpoint = "http://mi50:11434/v1"       # inline endpoint → an OpenAI-compatible provider
model = "mistral-small:24b"
tier = "medium"
max_concurrency = 2

[[pool.members]]                        # an authenticated / self-signed HOSTED upstream,
name = "glm"                            # no secret in the file (model-router increment 01)
endpoint = "https://213.173.96.56:8000/v1"
model = "/model"
api_key_file = "~/Downloads/runpod/glm/glm-api-key"  # api_key > api_key_env > api_key_file
insecure_tls = true                     # self-signed dev endpoint ONLY (warns; MITM risk)
context_window = 131072                 # per-member; unset ⇒ global [agent] context_window
tier = "heavy"
weight = 4.0
```

An inline `endpoint` member takes per-member **auth** (`api_key` / `api_key_env` /
`api_key_file` — all empty ⇒ keyless, for a local server that ignores the key), a
per-member **`insecure_tls`** (self-signed dev endpoints only — it warns), and a
per-member **`context_window`** (a fleet of different-sized models isn't forced to
share the one global window). The older parallel-list form (`members = ["glm", "mi50"]`
+ `tiers = [...]`) still parses. See [`config/agent.toml`](../../config/agent.toml) for
the annotated block.

## Observability

Metrics ride the `PoolEvent` → `record_pool_event` seam (so `agent-providers` stays
off `agent-metrics`): `agent_pool_members_alive{tier}`,
`agent_pool_member_inflight{member}`, `agent_pool_member_latency_seconds{member}`,
`agent_pool_select_total{policy}`, `agent_pool_member_saturated{member}`,
`agent_pool_saturation_shed_total`, `agent_pool_member_state{member,state}`,
`agent_pool_member_latency_ewma_ms{member}`, and the probe histogram.

## Security

Pool selection runs on every LLM call, and a `= "grpc"` remote pool's reported
health/load comes from another host — untrusted. Config and remote-reported weights,
in-flight counts, capacities, and latencies are **clamped** (NaN / negative / inf →
safe defaults) before any selection math, `sleep`, or `inc_by`. Selection never
panics on a degenerate pool (empty / all-dead / all-saturated / zero-weight), and the
in-flight guard + capacity admission are deadlock-free by construction.

## Design

The seam originated in the [code-review flow](../design/code-review/llm-pool.md)
(component 01) and was extended into a full load balancer by the
[GPU pool](../design/gpu-pool/README.md) track: load balancing
([01](../design/gpu-pool/01-load-balance.md)), capacity
([02](../design/gpu-pool/02-capacity.md)), and graded health
([03](../design/gpu-pool/03-gpu-health.md)).
