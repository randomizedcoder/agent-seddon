# Model router & upstream registry — implementation status

The living tracker for the [model-router](README.md) design. One gated PR per increment, based
off `main` — do not stack. Each ships the full contract (proto/gRPC/metrics/tracing/tests/bench/
leak) and must pass `nix develop -c nix flake check`.

## Increments

| # | Increment | Seam | Wire | Metrics | Tests | Bench+Leak | Status |
|---|---|:--:|:--:|:--:|:--:|:--:|:--:|
| — | Design directory | — | — | — | — | — | ✅ **merged** (PR #229) |
| 01 | [Rich metadata](01-metadata.md) (context/cost/tags + pool key-file/TLS) | ✅ | — | — | ✅ | ✅ | ✅ **merged** (PR #229) |
| 02 | [Task-aware routing](02-routing.md) (routing engine + `TaskRouter` + `[route]` policy) | ✅ | — | ✅ | ✅ | ✅ | ✅ **merged** (PR #229) |
| 02b | [`RouteHint` threading](02b-hint-threading.md) (hint on the request · task-mode axis · per-call roles · decision metrics) | ✅ | ✅ | ✅ | ✅ | ✅ | **planned** |
| 03 | [Config + registry control plane](03-registry-proto.md) (textproto bootstrap + `ProviderRegistryService`) | ✅ | ✅ | ✅ | ✅ | ✅ | **planned** |
| 04 | [Registry-backed routing](04-registry-backed.md) (consume + seed + live-signal ordering + per-upstream metrics) | ✅ | — | ✅ | ✅ | ✅ | **planned** |

## Build order = dependency order

- **01** makes the per-upstream metadata *exist* (context window, cost, tags) and makes the pool
  usable for authenticated/self-signed hosted endpoints — the Kimi + GLM enabler. TOML only.
- **02** adds the declarative policy engine + `TaskRouter` that route on 01's metadata.
  Still TOML (`[route]`); the hint is derived from request shape with a fixed role.
- **02b** threads the hint **onto the request** (core + proto): `session.current_mode` →
  `task_mode`, per-call roles at every internal call site, explicit override — and defines the
  precedence contract with the cognition graph's named-reference role routing.
- **03** makes the fleet + policy one `ModelRouterConfig` proto whose human-readable form is
  **textproto** — `agent --model-router-config <file>` loads the whole router at startup (no gRPC
  server needed), with multiple scenario files selectable by the flag — and serves the same
  messages via `ProviderRegistryService` (CRUD, swappable storage). The TOML→proto migration;
  legacy TOML still seeds it.
- **04** makes the router/pool **consume** the registry at scale, seeds it from TOML, adds the
  live-signal ordering policies (`cost | latency | least-loaded`), and per-upstream metrics.

## Implementation log (as-built deviations)

- **Design + 01 + 02 landed as one PR (#229)** rather than one PR per increment — recorded, not
  to be repeated; 02b/03/04 go back to one gated PR each.
- **01: routing metadata split, not piled onto the pool.** `PoolMemberCfg` gained the *enabler*
  fields only (`api_key_env`/`api_key_file`/`insecure_tls`/per-member `context_window`); the
  routing metadata (`tags`/`tier`/`input_cost`) lives on the new `[[route.upstreams]]`
  (`RouteUpstreamCfg`) instead of `ModelCapabilities`/`PoolMember`. Capability *facts* come from
  `provider.capabilities()`, routing *preferences* from config — a cleaner split than the spec's
  "everything on the model card". `PriceTable` plumbing did not land; `input_cost` is a manual
  per-upstream hint (revisit in 03 when cards become proto).
- **02: engine + fixed-role router only — hint threading deferred to [02b](02b-hint-threading.md)**
  (the code says so explicitly). As built: `Hint`/`Match` carry `role + min_context` (+
  capability/cost/tier filters) but **no `task_mode`**; `Prefer` orders by explicit
  position/tags/tier — the spec's `policy: cost|latency|weighted|least-loaded` live-signal
  ordering is **not built** (moved to 04, where the live snapshot exists); `min_context` is never
  derived (moved to 02b); `override_upstream` is honored by the engine but nothing sets it
  (02b). Breaker is a **reorder-to-back**, not a hard filter — a dead upstream is tried last,
  not dropped (deliberate: total availability beats strict health). Dispatch observability
  reuses the shared failover `RouteEvent` → `record_route_event`; decision-level metrics
  (role/rule/mode) come with 02b when the signals exist.
- **Cognition follow-ups (PR #232) shipped named-reference role routing first** — `[digest]
  provider`, `[instant] provider`, graph `provider` params / capability edges via
  `resolve_provider_ref`. The design predated it; [02b](02b-hint-threading.md) defines the
  composition: a named reference is a static pin that wins; unpinned slots stamped with a role
  route through the `TaskRouter` policy; a reference may itself name `"task-router"`.
- **Ports fixed for 03**: the `ProviderRegistryService` seam takes **50084** (metrics **9634**) —
  digest/graph/ast took 50081-3/9631-3 while the design was in flight.

## Cross-cutting invariants (every increment)

- **Additive only** — new struct/proto fields, new service; never a breaking change to `LlmPool`/
  `LlmProvider` or the existing `LlmPoolService`/`Provider` RPCs. `buf breaking` passes with **no
  baseline bump**.
- **`agent-providers` stays off `agent-metrics`** — new `agent_router_*` / `agent_registry_*`
  ride the `RouteEvent` → `record_route_event` seam.
- **Back-compat** — the existing `[provider]`/`[pool]`/`[router]` TOML keeps working and seeds the
  registry; an empty/absent `RouteHint` is exactly today's behaviour; policy defaults preserve
  current selection.
- **Secrets never stored or sent** — upstreams carry `api_key_ref` (env name / file path); keys
  resolve on the consuming host via `resolve_key_opt`.
- **Fail-soft + panic-free + deadlock-free** — every degenerate fleet (empty, all-dead,
  all-over-budget, hostile remote metadata, mid-refresh error) has a defined graceful outcome; the
  in-flight guard releases on every path (RAII).

## The immediate payoff (landed in 01)

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
