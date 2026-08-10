# 04 — Registry-backed routing at scale

Status: ✅ **built** (branch `feat/model-router-04`) — see [`STATUS.md`](STATUS.md) for
as-built deviations (opt-in `[route] source = "registry"`, rebuild-on-fingerprint
architecture, pool-mechanism stragglers, deferred inflight gauge / escalation hook /
judge-env unification). The increment that makes it real: the
`TaskRouter` (and the pool) **consume the [03](03-registry-proto.md) registry** as their source
of truth for 10–50 upstreams, TOML **seeds** the registry at boot, the **live-signal ordering
policies** (`cost | latency | least-loaded` — carved out of 02 as built) finally land against
the registry's health snapshot, and every upstream gets **per-upstream observability**.
Per-call role wiring itself moved earlier, to [02b](02b-hint-threading.md) — it needs no
registry; 04 scales it and picks up the stragglers (classifier vote, judge env-island).

> After 04, adding or retiring an upstream is a `Put`/`Delete` against a running registry — no
> code edit, no restart — and each subsystem (main loop, judge, classifier, summarizer, verifier,
> review) reaches a fit-for-purpose model.

## Motivation

01 gave upstreams a model card; 02 routed by task in-process against the TOML pool; 03 built the
gRPC registry. 04 connects them: the router reads the fleet + policy from the registry, so the
scale target (many upstreams, managed at runtime) is actually met, and the many internal callers
that today all reuse `[agent] provider` stop competing for one model.

## What already exists (and its gaps)

- `TaskRouter` (02) resolves a `RoutePolicy` over in-process cards from `[route]` TOML; per-call
  roles + `task_mode` on the request (02b).
- `ProviderRegistryService` + `ProviderRegistry` seam (03) — the fleet + policy, CRUD, storage.
- Still outside the role system after 02b: the classifier vote (`classifier.rs:67`,
  pool-tier path) and the eval-judge env convention.
- The pool builds members from the fixed TOML `[[pool.members]]` list (`builder.rs:604-649`).

## Design

### Router reads the registry

`TaskRouter` takes a `ProviderRegistry` handle (local `file`/`sqlite` store or a `= "grpc"`
remote) instead of an in-process card list. On each `complete` it snapshots the enabled fleet +
policy + live health, resolves as in 02, and dispatches. A registry **watch/poll** refreshes the
snapshot so a `Put` takes effect without restarting the agent (bounded cache; hostile numbers
clamped on every refresh).

### TOML seeds the registry (back-compat)

At boot, if the registry store is empty, the existing config is **imported**: each
`[[pool.members]]` / `[router] providers` / the top-level `[provider]` becomes an `Upstream`
(id = member name, prices from `PriceTable`, tags from config or defaults). `[route]` becomes the
`RoutePolicy`. So an existing TOML config keeps working unchanged — the registry is the new source
of truth, seeded from the old one. Removing TOML is **out of scope**; the seed is idempotent and
skipped once the store is populated.

### Live-signal ordering policies (moved here from 02)

`Prefer` gains the spec'd `policy: cost | latency | least-loaded` tail — as built, 02 orders by
explicit position/tags/tier only, because the engine has no live view. The registry snapshot
brings one: `UpstreamMeta` gains `in_flight` / `latency_ewma` (clamped on every refresh), and
the matched rule's `policy` breaks ties among equally-preferred survivors against those live
numbers plus the configured `input_cost`. Deterministic given a snapshot; the snapshot is
versioned so a decision is reproducible in the `route.select` span.

### Per-role routing at scale (roles landed in 02b)

[02b](02b-hint-threading.md) wired the per-call roles (main loop + distiller + instant +
verifier + judge + review, plus the named-reference precedence contract). 04 scales that over
the registry fleet and picks up the two stragglers:

- **Classifier vote** (`classifier.rs:67`) — dispatches through `LlmPool::complete_all` with a
  hardcoded `PoolTier::Light`; becomes a `Classify`-role policy decision (and the escalation
  hook below).
- The eval-harness **judge** convention (GLM by default, via `AGENT_E2E_JUDGE_*`) maps cleanly
  to the `Judge` role / a `judge`-tagged upstream — optionally unified here so the judge is a
  registry entry, not a separate env island. (Kept opt-in; the harnesses still accept the env
  override.)

### Escalation hook (designed, deferred)

The aspirational "classifier disagreement → escalate to heavy" path (`complete_all` only ever
hits `Light` today) becomes a `Role`-aware policy rule; the mechanism lands here as a hook, the
learned/adaptive part stays deferred (README).

## Threading — no seam change

`LlmProvider`/`LlmPool` unchanged. `TaskRouter` swaps its card source from config to the registry
handle; the internal callers swap their provider for the router + a role tag.

## Protobuf

No new proto — 04 **consumes** 03's service. (Any convenience RPC, e.g. a bulk `Import`, would be
additive.)

## Prometheus metrics

| Metric | Type | Labels |
|---|---|---|
| `agent_router_dispatch_total` | counter | `role`, `upstream`, `outcome` |
| `agent_router_upstream_inflight` | gauge | `upstream` |
| `agent_router_failover_total` | counter | `from`, `to`, `reason` |

Per-upstream, via the `RouterEvent`/`PoolEvent` seam — `agent-providers` stays off
`agent-metrics`. Session/user labels per the multi-session view.

## Tracing + logs

- The `route.select` span (02) now carries the registry snapshot version + role; failover hops
  log `from→to` with the retryable reason (never the prompt/body).

## Testing (table-driven + adversarial)

- `positive_` — a `Put` of a new upstream is picked up on the next `complete` (watch/poll); each
  role routes to its configured target; the TOML-seed import produces the same routing as the
  pre-migration `[pool]`.
- `negative_` — a `Disable` removes an upstream from routing but a later `Enable` restores it;
  seeding is skipped when the store is already populated.
- `corner_` — all upstreams disabled → a defined "no candidate" per role (fail-soft), not a hang;
  the judge role falls back to the default when no `judge`-tagged upstream exists.
- `boundary_` — 50 upstreams across roles routed within the bench ceiling; a role with exactly one
  eligible upstream.
- `adversarial_` (**mandatory**) — a **remote registry** (`= "grpc"`) reporting hostile
  cards/health can only **shuffle order or shed**, never dial outside the returned fleet, never
  panic, never leak in-flight; a mid-refresh registry error keeps the last good snapshot
  (degrade-don't-stall); an `override_upstream` still can't escape the eligible set.

## Benchmark + leak

- **Bench** — end-to-end `route → dispatch` over a 50-entry registry snapshot (cached) under the
  Ir ceiling; the snapshot refresh is off the per-call path.
- **Leak** — sustained routing + failover + periodic refresh nets zero in-flight and frees each
  snapshot (dhat budget).

## Security

- The registry snapshot is **clamped + validated on every refresh** (a remote registry is
  untrusted, exactly like a remote pool's health).
- Keys resolve **locally** from `api_key_ref` when a concrete provider is built — never from the
  registry payload.
- Per-role routing does not widen the model's authority: `override_upstream` and role are still
  filtered to the eligible fleet; a compromised registry misroutes at worst, within the fleet.
- Fail-soft/panic-free/deadlock-free across every degenerate fleet + refresh state.

## Deferred (whole-design, see [README](README.md))

- LLM meta-router; learned/outcome-based weights; the adaptive escalation policy; whole-fleet
  saturation spillover to a cloud provider.
