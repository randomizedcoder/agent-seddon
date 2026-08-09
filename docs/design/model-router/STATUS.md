# Model router & upstream registry — implementation status

The living tracker for the [model-router](README.md) design. One gated PR per increment, based
off `main` — do not stack. Each ships the full contract (proto/gRPC/metrics/tracing/tests/bench/
leak) and must pass `nix develop -c nix flake check`.

## Increments

| # | Increment | Seam | Wire | Metrics | Tests | Bench+Leak | Status |
|---|---|:--:|:--:|:--:|:--:|:--:|:--:|
| — | Design directory | — | — | — | — | — | **planned** |
| 01 | [Rich metadata](01-metadata.md) (context/cost/tags + pool key-file/TLS) | ✅ | ✅ | — | ✅ | ✅ | **planned** |
| 02 | [Task-aware routing](02-routing.md) (`RouteHint` + `TaskRouter` + policy) | ✅ | ✅ | ✅ | ✅ | ✅ | **planned** |
| 03 | [Config + registry control plane](03-registry-proto.md) (textproto bootstrap + `ProviderRegistryService`) | ✅ | ✅ | ✅ | ✅ | ✅ | **planned** |
| 04 | [Registry-backed routing](04-registry-backed.md) (consume + seed + per-role) | ✅ | — | ✅ | ✅ | ✅ | **planned** |

## Build order = dependency order

- **01** makes the per-upstream metadata *exist* (context window, cost, tags) and makes the pool
  usable for authenticated/self-signed hosted endpoints — the Kimi + GLM enabler. TOML only.
- **02** adds the `RouteHint` + declarative policy + `TaskRouter` that route on 01's metadata.
  Still TOML (`[route]`).
- **03** makes the fleet + policy one `ModelRouterConfig` proto whose human-readable form is
  **textproto** — `agent --model-router-config <file>` loads the whole router at startup (no gRPC
  server needed), with multiple scenario files selectable by the flag — and serves the same
  messages via `ProviderRegistryService` (CRUD, swappable storage). The TOML→proto migration;
  legacy TOML still seeds it.
- **04** makes the router/pool **consume** the registry at scale, seeds it from TOML, and routes
  each internal role independently, with per-upstream metrics.

## Cross-cutting invariants (every increment)

- **Additive only** — new struct/proto fields, new service; never a breaking change to `LlmPool`/
  `LlmProvider` or the existing `LlmPoolService`/`Provider` RPCs. `buf breaking` passes with **no
  baseline bump**.
- **`agent-providers` stays off `agent-metrics`** — new `agent_router_*` / `agent_registry_*`
  ride the `PoolEvent`/`RouterEvent` → `record_pool_event` seam.
- **Back-compat** — the existing `[provider]`/`[pool]`/`[router]` TOML keeps working and seeds the
  registry; an empty/absent `RouteHint` is exactly today's behaviour; policy defaults preserve
  current selection.
- **Secrets never stored or sent** — upstreams carry `api_key_ref` (env name / file path); keys
  resolve on the consuming host via `resolve_key_opt`.
- **Fail-soft + panic-free + deadlock-free** — every degenerate fleet (empty, all-dead,
  all-over-budget, hostile remote metadata, mid-refresh error) has a defined graceful outcome; the
  in-flight guard releases on every path (RAII).

## The immediate payoff (lands in 01)

A single pool of **Kimi (preferred) + GLM (self-signed fallback)**, both keys via `api_key_file`,
no secret committed — the concrete request that motivated the track. See
[01](01-metadata.md#the-kimi--glm-payoff).

## Deferred (whole-design)

- **LLM meta-router** — a cheap model picks among rule-eligible candidates. Decision engine stays
  declarative + live-signals.
- **Learned / outcome-based weights** — tune per-upstream preference from measured latency/cost/
  success (extends gpu-pool's deferred "learned weights").
- **Escalate-to-heavy on classifier disagreement** — the aspirational mode-vote path; the hook
  lands in 04, the adaptive part is deferred.
- **Whole-fleet saturation spillover** to a cloud provider.
