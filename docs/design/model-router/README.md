# Model router & upstream registry — task-aware routing over a fleet of LLMs

Status: **in progress** — increments [01](01-metadata.md) + [02](02-routing.md) merged
(PR #229, as the engine slice — see the [STATUS](STATUS.md) implementation log for the as-built
deviations); [02b](02b-hint-threading.md)/[03](03-registry-proto.md)/[04](04-registry-backed.md)
remain. One gated PR per increment, based off `main` (do not
stack) — the [gpu-pool](../gpu-pool/README.md) / adaptive-cognition rhythm. Living tracker:
[`STATUS.md`](STATUS.md).

## The problem

The agent should lean on **many upstream LLMs at once** — a strong hosted reasoning model, a
cheap local classifier, a long-context summarizer, a vision model — and **route each request to
the one that fits the task**. Two concrete upstreams already motivate it: a hosted **Kimi K3**
(valid-TLS litellm proxy, strong, our preferred generator) and **GLM-5.2** (self-signed dev
endpoint, our eval judge). We want to grow that to **10–50 upstreams** and pick between them per
request.

The [gpu-pool](../gpu-pool/README.md) track built the *load-balancing* half of this — health
probes, circuit breakers, in-flight-aware selection, latency grading — but for **"a handful of
local, keyless GPU boxes"**, and it named its own limits: *"parallel `members`/`tiers` string
lists … clumsy at many-targets scale"*, and cost-/latency-minimising policies *"deferred — needs
per-candidate price metadata … not yet plumbed"*. This design takes the pool from a GPU load
balancer to a **task-aware model router over a managed fleet**.

Three gaps, one design:

1. **Scale + management.** Config is hand-written TOML (`[pool]`/`[router]`/`[provider]`); the
   rest of the system moved its control surfaces to **protobuf + gRPC** (prompt library,
   scheduler, session registry). The fleet should be a **proto-defined, gRPC-managed registry**
   you can add to / update / enable at runtime — not a file you edit and restart.
2. **Rich per-upstream metadata.** Today a provider exposes only `{supports_tools,
   context_window, supports_response_format, supports_vision}`; there is **no cost, latency,
   tier, or capability taxonomy**, and — a real bug — **context window is not even per-upstream**
   (inline pool members inherit the one global `[agent] context_window`). Per-token pricing
   exists but sits **unplumbed** in `agent-tokenizer`'s `PriceTable`. We need each upstream to
   carry a **model card**: context length, cost, latency, capabilities, tags.
3. **Task-aware routing.** Selection is **task-blind**: the only capability gate is
   `is_capable()` (tools/vision). The system already classifies a `TaskMode` every turn and
   holds it in `session.current_mode` right where it calls the model — but the main loop routes
   to **one fixed provider**, ignoring it. We want to **tell the layer how to route** (rules) and
   let **it decide** the concrete upstream from live signals.

## What already exists (extend, don't rebuild)

- **Seams** (`agent-core/src/lib.rs`): `LlmProvider` (`capabilities`/`complete`/`stream`),
  `LlmPool` (`health`/`complete_all`/`complete` — fan-out + failover), and a concrete `Router`
  `LlmProvider` (failover chain) in `agent-providers/src/router.rs`.
- **Selection funnel** — small and centralized: `is_capable(caps, req)` (`router.rs:109`, shared
  by both seams), `PoolInner::eligible`/`order` (`pool.rs:314/334`), `Router::order`
  (`router.rs:211`). This is exactly where task/capability-aware logic slots in.
- **Live signals already measured**: per-member `in_flight` (RAII guard), latency EWMA, graded
  `healthy|degraded|dead`, circuit breaker, `max_concurrency`/saturation — all reusable as the
  "it decides" inputs.
- **`PoolMemberCfg`** (`config.rs:329`) is the de-facto upstream definition (name/endpoint/model/
  key/tier/weight/cost/max_concurrency) — this design lifts it into proto.
- **Task taxonomy** — `TaskMode {Review, Implement, Design, Debug, Explain, Other}` (`lib.rs`),
  classified every turn by `HybridClassifier`, sitting in `session.current_mode` at `agent.rs:893`
  where `self.provider.complete(req)` runs.
- **Control-plane precedent** — `PromptService` (List/Get/Put/Delete over metadata-carrying
  `PromptEntry`, server-side swappable storage) and `SchedulerService` (a job registry that
  *"outlives an individual agent process … one process holds the registry while any number of
  agents drive it"*). `SessionRegistry` mints server-side UUIDs. A `ProviderRegistryService` is a
  direct mirror.
- **Named-reference role routing** (cognition follow-ups, PR #232 — shipped after this design
  was written): `[digest] provider`, `[instant] provider`, and graph node `provider` params /
  capability edges pin a role slot to a *named* provider via `resolve_provider_ref`
  (`registry.rs:1088`). Static pins, no policy or live signals — exactly the mechanism the
  per-call `RouteHint` upgrades; the precedence contract (named pin wins; unpinned slots route
  by policy; a pin may name `"task-router"`) is defined in [02b](02b-hint-threading.md).

## The design

Five concepts, each **additive** over the existing seams.

### 1. `Upstream` — the model card

A named LLM endpoint definition + metadata. Replaces the TOML pool member as the unit of the
fleet:

- **Identity** — `id`, `kind` (`openai-compat` | `anthropic` | `grpc`), `enabled`.
- **Connection** — `base_url`, `model`, **`api_key_ref`** (an env-var name or file path,
  **never the secret value**), `insecure_tls`, `version`, `max_retries`.
- **Capabilities** — per-upstream `context_window`, `max_output_tokens`, `supports_tools`,
  `supports_vision`, `supports_response_format`, and **`tags[]`** (`coding`, `reasoning`,
  `long-context`, `cheap`, `vision`, …) — the free-form axis routing rules match on.
- **Economics / perf** — `input_cost` / `output_cost` (per 1M tokens), `tier`
  (`light|medium|heavy`), `weight`, `max_concurrency`.
- **Live state** (runtime only, never stored) — health/grade, `in_flight`, `latency_ewma`,
  breaker — the gpu-pool signals.

### 2. `ProviderRegistryService` — the gRPC control plane

The fleet lives in a service, not a TOML file (mirrors `PromptService`):

- `List / Get / Put / Delete / Enable` over `Upstream`.
- `GetPolicy / PutPolicy` over the routing rules.
- `Route(RouteHint) → RouteDecision` — introspection ("what would you pick, and why").
- `Health` — the per-upstream liveness/grade snapshot.
- **Swappable server-side storage** (file / sqlite / grpc) like the prompt library; `agent
  --serve-provider-registry`. One registry, many agents drive it.

### 3. `RouteHint` — per-request routing signals (all four)

Added as an optional field on `CompletionRequest` (core + `common.proto`, additive) so it
travels with the request, including to a remote router (lands in
[02b](02b-hint-threading.md); the 02 engine already resolves all of these):

- `task_mode` — the classified `TaskMode` (from `session.current_mode`).
- `role` — the internal call-site (`main` | `judge` | `classify` | `summarize` | `verify` |
  `review`), so each subsystem routes to a fit model.
- **Request requirements** — `min_context` (the prompt won't fit smaller models),
  `needs_vision`, `needs_tools`, `max_cost`, `latency_target` — hard filters. (As refined in
  [02b](02b-hint-threading.md): tools/vision are **derived from the request**, never asserted
  by the hint — a hint can narrow the fleet but never clear a real requirement;
  `latency_target` waits for the live snapshot in [04](04-registry-backed.md).)
- `override_upstream` — an explicit id that bypasses classification when a caller already knows.

### 4. `RoutePolicy` — "tell it how to route"

A declarative, ordered list of rules:

```
RouteRule {
  match:  { task_mode?, role?, min_context?, tags_required?, needs_vision?, … }
  prefer: { tags[], tier, upstreams[], policy: cost | latency | weighted | least-loaded }
}
```

Resolution is two-phase: **filter** upstreams by the hard requirements (enabled ∩ healthy ∩
capability ∩ `context_window ≥ min_context` ∩ tier-floor), then **order** the survivors by the
first matching rule's `prefer`, resolved against **live signals** (health grade, in-flight,
latency EWMA, configured cost). No match → a default rule. This is the "it decides" half — cheap,
deterministic, predictable.

### 5. `TaskRouter` — the drop-in seam

An `LlmProvider` impl that reads the `RouteHint` (+ `session.current_mode`) + registry + policy,
picks one upstream, and calls it with the existing breaker/failover safety. Drops into
`[agent] provider = "task-router"` and `self.provider` at `agent.rs:893` — the loop starts
routing without touching the loop. Because it *is-a* `LlmProvider`, every in-process caller
(summarizer, verifier, dimensions, structured) gets routing for free once it passes a `role`.

## Configuration & bootstrap — one human-readable file, pointed at startup

The whole model-router config is itself a proto message —
`ModelRouterConfig { repeated Upstream upstreams; RoutePolicy policy }` — written as **protobuf
text format (textproto)**. This is the point of the migration made concrete: the proto schema
*is* the config language, so there is **no second format to maintain**, it is human-readable and
git-diffable, and it round-trips through the exact messages the gRPC control plane uses.

At startup you point the agent at a file — **no gRPC server required** for the simple case; the
file loads into a local registry and the `TaskRouter` routes off it:

```sh
agent --model-router-config config/model-router/runpod.textproto  "…"
# or [agent] model_router_config = "…"  /  env AGENT_MODEL_ROUTER_CONFIG
```

Because it is plain text, you keep **several scenario files under version control** and select one
by pointing the flag at it — swapping the whole fleet + routing policy by choosing a file:
`runpod.textproto`, `local-ollama.textproto`, `eval.textproto`, `prod-fleet.textproto`. The
registry's `file` storage backend reads and writes this **same** textproto, so a hand edit and a
control-plane `Put` share one format.

```textproto
# config/model-router/runpod.textproto
upstreams { id: "kimi"  kind: "openai-compat"  enabled: true
  base_url: "https://…-4000.proxy.runpod.net/v1"  model: "moonshotai/Kimi-K3"
  api_key_ref: "file:~/Downloads/runpod/glm/kimi-api-key"
  context_window: 131072  tags: "reasoning"  tags: "long-context"  tier: HEAVY }
upstreams { id: "glm"   kind: "openai-compat"  enabled: true
  base_url: "https://213.173.96.56:8000/v1"  model: "/model"
  api_key_ref: "file:~/Downloads/runpod/glm/glm-api-key"  insecure_tls: true  tier: HEAVY }
policy {
  rules { match { role: REVIEW }        prefer { tags: "reasoning"  tier: HEAVY } }
  rules { match { min_context: 32000 }  prefer { tags: "long-context"  policy: "cost" } }
  default_policy: "cost"
}
```

Schema + loader land in [03](03-registry-proto.md); the legacy TOML `[pool]`/`[provider]` remains
only as a back-compat seed for existing configs.

## Seam-extension strategy (additive)

- New logic lives **inside** the shared selection funnel: extend `is_capable` (context/vision/
  tools), add rule-based ordering to `PoolInner::order` / `Router::order`. The `LlmPool` /
  `LlmProvider` trait signatures are **untouched**.
- `ModelCapabilities`, `PoolSpec`/`PoolMember`, `CompletionRequest` **gain fields**; every
  addition is a new struct field / new proto field number — `buf breaking` passes with **no
  baseline bump**.
- `ProviderRegistryService` is a **new** service + messages — purely additive.
- **The human-readable config is textproto** (the proto schema itself), pointed at with
  `--model-router-config`; the legacy TOML `[pool]`/`[provider]`/`[router]` keeps working and
  **seeds** the registry at boot — no config is removed, the registry is the new source of truth.

## The observability & quality contract (every increment)

Non-negotiable, per [`CLAUDE.md`](../../../CLAUDE.md):

- **Protobuf** additive (buf-safe, no baseline bump) — the `agent-core`-mirroring type names
  keep their `buf.yaml` lint exemptions.
- **Prometheus** via the `PoolEvent` → `record_pool_event` seam (`agent-providers` stays off
  `agent-metrics`) — new `agent_router_*` / per-upstream families added the metered way.
- **Tracing** — a `route.select` span (matched rule, chosen upstream, why) under the caller's
  span; never the prompt.
- **Tests** table-driven (`rstest`, all four prefix classes) with **mandatory `adversarial_`**
  cases (hostile metadata numbers, traversal ids, over-long segments).
- **Bench** — the iai-callgrind Ir ceiling on `eligible` stays green (selection runs on every
  call, now over up to 50 upstreams).
- **Leak** — the dhat in-flight-guard budget holds.

## Security stance (the model is untrusted — and so is a remote registry)

Routing runs on **every** LLM call, and a registry dialed as `= "grpc"` reports metadata from
**another host** — untrusted.

- **Secrets never stored or sent.** An `Upstream` carries `api_key_ref` (env name / file path),
  resolved locally at build time (reuse `resolve_key_opt`). No key value in proto, storage, or a
  log.
- **Clamp hostile numbers** (context/cost/weight/latency/in_flight — NaN/neg/inf) to sane ranges
  before selection math, a `sleep`, or an `inc_by`.
- **Validate ids/names** as path-safe segments server-side (`safe_segment`, over-length cap) — a
  registry id can become a storage path segment (session_registry discipline).
- **Never panic / never deadlock / degrade-don't-stall** on a degenerate fleet (empty, all-dead,
  all-over-budget, zero-weight) — every case has a defined fail-soft outcome, exactly like the
  pool today.

## Increments

| # | Increment | What it adds |
|---|---|---|
| **01** ✅ | [Rich metadata](01-metadata.md) | per-upstream `context_window` · pool-member `api_key_file`/`env` + `insecure_tls` (the Kimi+GLM enabler) · `tags`/`tier`/`input_cost` on `[[route.upstreams]]` (as-built split — see STATUS) |
| **02** ✅ | [Task-aware routing](02-routing.md) | deterministic `route::Policy` engine · `TaskRouter` `LlmProvider` (fixed role, shape-derived hint) · declarative `[route]` TOML · breaker-as-reorder failover |
| **02b** | [`RouteHint` threading](02b-hint-threading.md) | `RouteHint` on `CompletionRequest` (core + proto) · `task_mode` axis on `Match` · per-call roles at every internal call site (`RoleScoped`) · named-reference precedence contract · decision metrics + `route.select` span |
| **03** | [Registry control plane](03-registry-proto.md) | `upstream.proto` + **`ModelRouterConfig` textproto** loaded at startup (`--model-router-config <file>`, scenario files) · `ProviderRegistryService` (CRUD + Route + Health, port 50084) · textproto/sqlite/grpc storage · `--serve-provider-registry` · SEAMS row |
| **04** | [Registry-backed routing](04-registry-backed.md) | router/pool consume the registry (10–50) · TOML seeds it · live-signal `prefer.policy` ordering (`cost`/`latency`/`least-loaded`) · per-upstream metrics · escalation hook |

Each is its own PR off `main` — **do not stack** (the code-review-flow lesson).

## Deferred (out of scope for this design)

- **LLM meta-router** — a cheap model that reads an ambiguous task and picks among the
  rule-eligible candidates (decision engine stays declarative + live-signals for now).
- **Learned / outcome-based weights** — tune per-upstream preference from measured
  latency/cost/success (extends gpu-pool's deferred "learned weights").
- **Escalate-to-heavy on classifier disagreement** — the aspirational mode-vote path
  (`complete_all` only ever hits `Light` today).
- **Whole-fleet saturation spillover** to a cloud provider.
