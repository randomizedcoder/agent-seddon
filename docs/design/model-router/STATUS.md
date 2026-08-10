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
| 02b | [`RouteHint` threading](02b-hint-threading.md) (hint on the request · task-mode axis · per-call roles · decision metrics) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ **built** (`feat/model-router-02b`) |
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
- **02b as built** (branch `feat/model-router-02b`): `RouteRole` lives in `agent-core`
  (`route::Role` is a re-export); `TaskMode` + `PoolTier` **moved** from
  mode.proto/llm_pool.proto into common.proto so `RouteHint` avoids an import cycle —
  same-package, wire/JSON-identical, `buf breaking` (WIRE_JSON) green with **no baseline
  bump**. `needs_vision`/`needs_tools` are NOT on the core/wire hint at all (always
  request-derived — stronger than the spec's "derivable"). The spec's **review** role
  slot is NOT wired: the review fan-out dispatches via `LlmPool::complete_all` (a
  pool-tier path, like the classifier vote) — both recorded for 04's straggler pass.
  The digest distiller's no-pin fallback is now Summarize-stamped main provider (a
  routing main provider steers background distillation). `route.select` ships as a
  debug event inside the metered provider span rather than a per-call span (same
  attribution, no span overhead). Strictness tightened beyond the spec: a typo'd
  `match` role **or** task_mode is a startup config error (the earlier
  degrade-to-match-any behaviour is gone); `prefer` stays lenient.
- **02b hardening pass** (same branch, post-live-verify): coverage/bench/stress sweep.
  Tests grew to cover the full four-class table per surface (engine rule-index
  reporting incl. override-reports-no-rule; cost/tier boundary equality; estimate
  cases incl. media-only and clamp-at-cap; `-0.0` cost pinned as kept — IEEE;
  per-variant wire roundtrips for every role/mode/tier; `RoleScoped` roleless-hint
  passthrough + double-wrap outer-wins; empty-match-strings-are-unconstrained;
  metered Decided/NoCandidate land in the right families with bounded labels).
  Bench isolation showed the whole-path number was ~55% input construction —
  the isolated decision was 58k Ir; the **borrowed-view + index pass**
  (`UpstreamMeta<'a>` borrows id/tags; `resolve_indices` sorts fleet indices with
  the id tie-break borrowed, override/order() lose their by-id searches; the
  TaskRouter no longer clones every upstream's id+tags per call) cut it to
  **22.7k Ir (2.6×)** with byte-identical ordering (all prior tests unchanged).
  New `route_stress` (1,600 calls / 32 tasks / 30-member flapping fleet / hostile
  hints: liveness, exact Decided+NoCandidate accounting, post-storm recovery) and
  `route_leak` dhat budget (failover path frees all scratch, <120 blocks/call,
  registered in `nix/checks/leak.nix`).
- **02b infrastructure round** (same branch): every existing test tier covers the
  routed path in its own idiom. **CLI e2e over real HTTP**
  (`agent-cli/tests/route_e2e.rs`, `FakeLlm` harness): the shipped binary routes to
  the preferred upstream only (the fallback sees zero bytes), fails over between
  real endpoints on 429s, a `role = "main"` rule flips the choice (the 02b stamped
  hint reaches the policy through the binary), and a typo'd rule exits nonzero
  with zero requests sent. **config-roundtrip.nix**: a task-router fixture with
  role+mode rules must `--check-config` clean; a typo'd task_mode and a
  self-referencing upstream must fail closed — through the shipped binary's real
  loader/factory chain. **gRPC wire boundary** (`roundtrip.rs`, tcp+uds): a benign
  hint survives the provider seam intact; a hostile one is sanitized at decode
  (NaN cost dropped, min_context capped, 64KiB override gone) before the
  server-side provider sees it. **Wire load** (`loadtest` example +
  loadtest-smoke): new `provider-routed` ramp seam — a real `TaskRouter` over
  scripted upstreams behind the served seam, every request carrying a role+mode
  hint, so hint→decode→resolve→dispatch is the measured path (clean at conc 16,
  0 shed/err, ~25k req/s UDS loopback).
- **FIXED a pre-existing hermetic break on main** (exposed by the
  config-roundtrip extension): the crane source filter dropped `.textproto`, so
  `config/cognition/` was EMPTY in filtered sources and
  `agent-graph/tests/examples.rs` failed any fresh `nix build .#agent` /
  `.#checks…test` (verified failing on pristine `b781cf8`; caching had masked it
  since the cognition-graph merge). One-line filter clause in `nix/default.nix`.
- **02b live-verified** against the runpod endpoints (Kimi + GLM, keys via
  `api_key_file`): (1) a debug-cue prompt flipped the classifier (`mode.switch
  other→debug`) and the MAIN turn matched the `task_mode = "debug"` rule —
  `route.select role=main task_mode=debug rule=rule1 chosen=glm`, overriding the
  default Kimi preference per call; (2) the background distiller routed by its
  stamped role with **no named pin** — `route.select role=summarize rule=rule0
  chosen=glm` inside the `distill.exchange` span while the main turn used Kimi.

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
