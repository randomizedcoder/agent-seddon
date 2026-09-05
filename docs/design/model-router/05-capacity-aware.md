# 05 — Capacity-aware routing (multi-GPU gateways)

## Problem

A configured upstream is not always one GPU. A live upstream like **Kimi** can be an
**OpenAI-API-compatible gateway that internally load-balances across several GPUs**
(e.g. 3× B300). Such an endpoint absorbs ~N× the concurrency of a single card, but the
router treated every upstream as interchangeable on load: the `least-loaded` tie-break
([04](04-registry-backed.md)) keyed on **raw in-flight count**, so a 3×-capacity gateway and a
1× single-GPU box with equal cost/tags/tier were driven to the *same* in-flight count — the
gateway received ~1 share instead of 3, and its headroom was wasted.

(The fan-out **pool** — [gpu-pool 02](../gpu-pool/02-capacity.md) — already models this: `max_concurrency`
is a hard admission cap there, and `least-loaded` fills a bigger member later. This increment brings
the **task-router** to the same capacity-awareness, since that is the path the Kimi/GLM fleet uses.)

## Decision

Order `least-loaded` by **normalised load `in_flight / max_concurrency`** instead of raw in-flight.
The field already exists end-to-end on the `Upstream` card/proto and is clamped
(`MAX_UPSTREAM_CONCURRENCY`) — 04 simply never *consumed* it on the router track. This increment
consumes it:

- **Semantics — a gateway is one upstream with a bigger `max_concurrency`.** No new "replicas"
  concept: the gateway does its own internal load-balancing, so from the agent's view it is one
  endpoint whose capacity ≈ GPUs × per-GPU slots. Set `max_concurrency ≈ 3 ×` a single card's slots.
- **Soft, not a cap.** The engine only *reorders*; it never hard-skips a saturated upstream. A
  higher-capacity endpoint simply looks less loaded and is preferred until the ratios equalise; the
  gateway's own `429`/`503` (retryable → existing failover) remains the real overload backstop.
- **Exact back-compat.** `max_concurrency = 0` (unknown/unset) ⇒ effective capacity `1`, so the key
  is `in_flight × SCALE` — monotonic in in-flight, i.e. byte-identical ordering to 04. A fleet that
  sets no capacity is unaffected.

## Mechanism

`route::UpstreamMeta` gains `max_concurrency: u32`; `Prefer::rank`'s `LeastLoaded` arm becomes a
fixed-point ratio (`in_flight * LEAST_LOADED_SCALE / max(cap, 1)`), overflow-safe in `i64`. The
field is threaded through `RouterUpstream` (both the static `[[route.upstreams]]` build in
`registry.rs` and the registry-card build in `registry_router.rs`), and the `[[route.upstreams]]`
TOML (`RouteUpstreamCfg`) gains a `max_concurrency` key (clamped on build). The store's `decide()`
preview carries it too, so the Route introspection matches the live decision.

## Deferred → 06 (adaptive effective-capacity)

Static capacity is the whole win for a stable fleet. A planned follow-up makes the *effective*
capacity track the gateway's real state: `effective = configured × health_factor`, where
`health_factor ∈ (0, 1]` shrinks on retryable overload (`429`/`503`) and latency-EWMA spikes and
recovers on sustained success — so a B300 dropping out of the gateway is absorbed automatically,
still without hard-blocking. Note the health caveat this addresses: a single gateway `ping()` proves
the *gateway* is up, not that all N cards behind it are — latency grading + `429` backpressure are the
observable proxies for reduced real capacity; per-backend visibility (the deferred `/api/ps`-style
utilization probe) stays out of scope because we cannot see behind the gateway.
