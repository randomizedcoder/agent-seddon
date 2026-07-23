# 01 — LLM Pool & Health

Status: **design / pre-implementation.**

The foundation for the two model-calling stages of the review flow (the mode vote
in [`02`](mode-detection.md) and the summaries in [`08`](summarization.md)), and a
general capability the rest of the agent can adopt. It turns the existing
failover-only `Router` into a **declarative pool of heterogeneous, cheap,
possibly-flaky endpoints** with **active health-checking** and **parallel
fan-out** dispatch.

## Motivation

The deployment is not one model — it is a pool with very different shapes:

| Endpoint | Hardware | Role | Tier |
|---|---|---|---|
| GLM | 8×MI300, 192 GB ea | powerful, near-free, hammer it | `heavy` |
| local 32B | MI50 32 GB | summaries, simple judgments | `medium` |
| small | RTX 3070 8 GB | trivial classification | `light` |

Two properties matter that `Router` does not give us:

1. **They are intermittent / overloaded.** A colocated box reboots; the MI50 may
   be busy. We must know *which endpoints are alive right now* before dispatching,
   not discover it by burning a request.
2. **We want to use many at once.** The cheap-heavy economics call for *fan-out*
   — ask three light models "is this a review?" and combine — which `Router`, a
   pick-one failover chain, cannot express.

## What already exists (and its two gaps)

`Router` (`crates/agent-providers/src/router.rs`) is-a `LlmProvider` that composes
`Vec<Candidate { name, provider }>` with a per-candidate circuit breaker
(`failure_threshold`, `cooldown_secs`), `RoutePolicy::{InOrder, RoundRobin}`,
capability gating (`is_capable`), and a `RouteEvent` observer. `RouterCfg`
(`crates/agent-runtime/src/config.rs`) declares the candidate names and policy.
Reuse all of it. The gaps:

- **Health is passive.** `Health { consecutive_failures, opened_ms }` only opens
  after real requests fail. For an intermittent pool we want an **active probe**.
- **Dispatch is pick-one.** `complete()` returns the first healthy candidate's
  answer. We need a **fan-out** that returns *all* answers for a combine step.

## Design

### `LlmPool` — generalize, don't fork

A new component in `agent-providers` that **wraps the same `Vec<Candidate>` and
`Health` machinery** and adds:

```rust
/// A tier orders endpoints by capability, not just preference.
pub enum PoolTier { Heavy, Medium, Light }

pub struct PoolMember {
    pub candidate: Candidate,      // reuse Router's Candidate (name + Arc<dyn LlmProvider>)
    pub tier: PoolTier,
    pub cost_hint: f32,            // optional; 0.0 = free/local. Clamped, never trusted.
    pub health: Health,           // reuse Router's breaker; now ALSO fed by the prober
}

impl LlmPool {
    /// Failover: first healthy member at/above `min` tier. Delegates to Router semantics.
    async fn one(&self, req: CompletionRequest, min: PoolTier) -> Result<CompletionResponse>;

    /// Fan-out: dispatch to the N cheapest healthy members of `tier`, concurrently,
    /// bounded by `fanout`. Returns every settled result (Ok or per-member Err).
    async fn all(&self, req: CompletionRequest, tier: PoolTier, fanout: usize)
        -> Vec<(String /*member*/, Result<CompletionResponse>)>;
}
```

`one()` is Router behaviour with a tier floor. `all()` is the new primitive: a
bounded `join_all` (the idiom already used for parallel tool dispatch in
`agent.rs`), **fail-soft per member** — a dead member yields `Err` in its slot,
never aborts the batch. The *combine* step lives with the caller (02 votes, 08
just takes whatever came back), exactly as the verifier design keeps reduction out
of the fan-out.

The pool is-a `LlmProvider` too (via `one()` with `min = Light`), so it drops into
`[agent] provider = "pool"` for the whole agent if wanted, and composes with
`Router` (a pool member can itself be a `grpc` remote or a router).

### Active health probe

A background task (spawned once, like the telemetry writer) probes each member on
a timer and updates its `Health`:

- **Probe = capability-appropriate, cheap.** Prefer a provider-native liveness
  call — an OpenAI-compatible `GET /v1/models` or a 1-token `complete("ping")`
  with a short timeout. The probe method is a small addition to the pool member,
  not a new seam.
- **Feeds the existing breaker.** A failed probe increments the same
  `consecutive_failures` that request failures do; a success closes the breaker.
  So `order()` — already the one place that ranks members — transparently prefers
  live endpoints. No second ranking system.
- **Bounded & clamped.** Probe interval, timeout, and any server-supplied hint are
  clamped (`CLAUDE.md`: hostile numbers). A member is marked live/dead by the
  breaker state; a `gauge` exposes it (see Observability).

> **Why not just reuse the gRPC `grpc.health.v1` service?** That reports
> *transport* health of a served seam, not *model* liveness of a remote inference
> endpoint. The pool probe answers "will this model actually respond?", which is
> the question an intermittent MI50 poses.

### Configuration

```toml
[agent]
provider = "pool"            # optional: use the pool as the agent's provider too

[pool]
members = ["glm", "mi50", "rtx3070"]   # names of registered providers (may be grpc/router)
probe_interval_secs = 15               # active liveness cadence; clamped [5, 3600]
probe_timeout_secs  = 3                # per-probe deadline; clamped
fanout              = 3                # max concurrent members for `all()`
policy              = "cheapest"       # cheapest | round-robin | in-order (reuses RoutePolicy + tier)

[pool.tiers]                            # tier + cost per member name
glm      = { tier = "heavy",  cost = 0.0 }
mi50     = { tier = "medium", cost = 0.0 }
rtx3070  = { tier = "light",  cost = 0.0 }
```

Each `members` name is resolved back through the registry (the composing-factory
pattern `Router` already uses — `FactoryCtx::registry()`), so a member can be an
`openai-compat`, an `anthropic`, a `grpc` remote, or a nested `router`.

## Failure semantic

**Fail-soft.** `all()` never fails as a batch — dead members are empty slots and
the caller decides if it has enough. `one()` fails-over like `Router`, and only
falls through to an error when *every* member is unhealthy or incapable (Router's
"ordered last, never dropped" rule keeps a total outage attempting something).

## Protobuf

The pool is serviceable so a remote process can host the health view and fan-out
(the endpoints themselves are already remote; this lets the *pool policy* be a
service). Closed sets are enums; hints are floats clamped on receipt.

```proto
enum PoolTier { POOL_TIER_UNSPECIFIED = 0; LIGHT = 1; MEDIUM = 2; HEAVY = 3; }

message PoolMemberHealth {
  string  name            = 1;
  PoolTier tier           = 2;
  bool    alive           = 3;   // breaker closed
  uint32  consecutive_failures = 4;
  uint32  last_probe_ms   = 5;   // duration of the last probe, for accounting
}

message HealthReport { repeated PoolMemberHealth members = 1; }

// A single fan-out request/response reuses the provider request shape.
message PoolCompleteRequest {
  agent.v1.CompletionRequest req = 1;
  PoolTier tier    = 2;
  uint32   fanout  = 3;          // clamped server-side
}
message PoolMemberResult {
  string name        = 1;
  bool   ok          = 2;
  string error       = 3;        // status/class only, never body text
  uint32 duration_ms = 4;        // per-member wall-clock (parallelism accounting)
  agent.v1.CompletionResponse response = 5;
}
message PoolCompleteResponse { repeated PoolMemberResult results = 1; }
```

## gRPC interface

```proto
service LlmPoolService {
  rpc Health   (google.protobuf.Empty)  returns (HealthReport);
  rpc Complete (PoolCompleteRequest)    returns (PoolCompleteResponse);  // fan-out
}
```

`--serve-llm-pool`, endpoint from a new `pool` block in `nix/constants.nix`. Wire
failure semantic: **fail-soft** — a per-member error is a field, not a gRPC error;
the RPC only errors on a malformed request. Consolidated in
[`10`](wire-contracts.md).

## Prometheus metrics

| Metric | Type | Labels |
|---|---|---|
| `agent_pool_members_alive` | gauge | `tier` |
| `agent_pool_probe_duration_seconds` | histogram | `member`, `outcome` |
| `agent_pool_probes_total` | counter | `member`, `outcome` = `live`\|`dead` |
| `agent_pool_dispatch_duration_seconds` | histogram | `mode` = `one`\|`all` |
| `agent_pool_member_calls_total` | counter | `member`, `outcome` |

Emitted the `Router` way: `agent-providers` stays off `agent-metrics`; the pool
raises typed `PoolEvent`s (mirroring `RouteEvent`) that the runtime turns into
metrics (`metered.rs`). Per-member latency also appears under the standard
provider metrics because each member is wrapped in the provider decorator.

## Tracing + logs

- Span `pool.dispatch` with fields `mode`, `tier`, `fanout`, `alive_members`; one
  child span `pool.member` per fan-out call (`member`, `duration_ms`, `ok`). This
  child fan-out is what makes 02/08 parallelism legible in the trace.
- Span `pool.probe` per probe cycle (`member`, `duration_ms`, `alive`).
- Logs: `INFO` on a member transitioning live↔dead (name + tier only); `WARN`
  when `all()` returns fewer than `fanout` live results. **Never** log endpoint
  URLs, keys, or response bodies — names, tiers, counts, durations only.

## Security

- Endpoint URLs/keys stay in the provider layer (as today); the pool handles only
  names. Errors carry class/status, never bodies.
- `cost`, `fanout`, probe intervals, and any server hint are **clamped** before
  use in a `sleep`, a loop bound, or an `inc_by`.
- A member listing the pool (recursion) is rejected at build time, exactly as
  `Router` rejects listing itself.

## Deferred

- **Cost- and latency-minimising policy** beyond `cheapest`-by-tier — needs the
  per-candidate `PriceTable` plumbing the `Router` doc already lists as deferred.
- **Adaptive fan-out** (widen when disagreement is high). Fixed `fanout` first.
- **Trust-weighted combine** — belongs to the caller and to the offline weighting
  the verifier design describes; not the pool's job.
