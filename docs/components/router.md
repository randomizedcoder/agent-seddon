# Router

Provider routing and failover. Parity spec [25](../parity/25-model-routing.md).

A `Router` **is-a** `LlmProvider`, so nothing downstream knows it exists — the
loop, the context strategy, and the metered decorators all see one provider. What
it adds is resilience: an agent that speaks to exactly one provider inherits that
provider's worst day, and a classified *transient* failure on the primary should
continue on a secondary rather than aborting the run.

Each candidate is still an independent seam, **including a `grpc` client**, so one
router can span local and remote providers.

## Configuration

```toml
[agent]
provider = "router"

[router]
providers         = ["anthropic", "openai-compat"]   # preference order
policy            = "in-order"                       # in-order | round-robin
failure_threshold = 3                                # failures before the breaker opens
cooldown_secs     = 30                               # how long it stays open
```

Candidate names are other **registered provider names**, built back through the
registry — so a candidate can be `grpc`, or anything an out-of-tree binary
registered.

## Three rules that make failover safe

### 1. Only retryable failures fail over

A terminal failure — auth, billing, bad request, content policy, unknown model —
fails the same way on every candidate. Trying them all burns the chain, and real
money, to arrive at the identical answer. Classification lives in `agent-retry`
(`classify`) and is shared rather than re-implemented:

| Class | Examples |
|---|---|
| `Retryable` | 429, 5xx, 529 overloaded, timeout, connection refused/reset |
| `Terminal` | 401/403 auth, 402 billing, 400 bad request, 404 model, content policy |

**Unknown failures classify as `Terminal`.** That is the conservative choice: an
unrecognised error is more likely a deterministic bug (a malformed request, an
unsupported parameter) than a transient blip, and retrying it across every
candidate is expensive and pointless.

> Classification reads the error *message*, because that is the contract the
> provider seam actually has — `Error::Provider(String)` carries no status code,
> and the in-tree adapters format failures as `"http {code}: {body}"`. This is
> the honest weak point: a custom provider that formats errors differently gets
> `Terminal` (i.e. no failover) rather than a wrong retry. Making the status
> structured on `Error` would remove the guesswork and is the natural follow-up.

### 2. An unhealthy candidate is skipped

Consecutive failures open a per-candidate circuit breaker; it closes again after
`cooldown_secs`. Without this a dead provider costs a timeout on *every* turn
forever. Unhealthy candidates are ordered **last** rather than dropped, so a total
outage still attempts something instead of failing with "no candidates".

### 3. Incapable candidates are not tried

A candidate that structurally cannot serve the request — no tool support when the
request carries tools, no vision when it carries images — is skipped. Failing over
to it would just produce a different error.

## Capabilities

The router reports the **union** of its candidates' capabilities, with the
**minimum** context window:

- Union, so the loop doesn't disable tools just because the *first* candidate
  lacks them.
- Minimum window, because a request has to fit whichever candidate ends up
  serving it.

## Streaming

Failover covers failures raised while *establishing* the stream. Once bytes are
flowing the turn is committed — restarting mid-stream would duplicate content the
caller has already seen.

## Observability

| Metric | Labels |
|---|---|
| `agent_route_decisions_total` | `target`, `decision` = `routed` \| `fellover` \| `skipped_unhealthy` \| `exhausted` |

Each candidate is also individually metered (it is wrapped in the standard
provider decorator before being handed to the router), so per-target latency and
error counts appear under the usual provider metrics with the candidate's name.

`agent-providers` does not depend on `agent-metrics`, so the router emits typed
`RouteEvent`s through a callback and the runtime turns them into metrics — keeping
the dependency direction intact rather than inverting it for observability.

## A note on the registry

The router factory is a **composing** factory: it builds its candidates by calling
back into the registry, which is why `FactoryCtx` carries a registry handle. The
borrow is immutable and re-entrant (`build_*` takes `&self`), so a factory the
registry invoked may call back into it.

A router listing itself would recurse until the stack blows, so that is rejected
at build time with a clear message.

## The task-router (`provider = "task-router"`)

The declaratively-routed sibling ([model-router](../design/model-router/README.md)
increments 02 + 02b): same is-a-`LlmProvider` drop-in, same failover/breaker
discipline (open breakers are *reordered to the back*, not dropped), but the
*decision* runs a `[route]` policy — ordered rules matched against each request's
signals, survivors ordered by tag/tier/explicit preference — over the fleet's
**live** capability facts (context window, tools, vision read from each provider)
plus configured metadata (`tags`/`tier`/`input_cost` on `[[route.upstreams]]`).

Since 02b every request carries a **`RouteHint`** (additive on
`CompletionRequest`, also on the wire): the turn's classified **task mode**
(stamped by the main loop), the calling **role** (`main` — the loop;
`summarize` — digest/memory distiller + instant objective; `verify` — the llm
verifier; `judge` — consensus critic + fork judge; each slot wrapped by a
`RoleScoped` stamp that never overwrites an explicit per-call hint), an optional
context floor / cost cap / tier floor, and an `override_upstream`. Rules match
`role`, `task_mode`, and `min_context`; a typo'd constraint is a **config error
at startup**, never a silently-match-anything rule.

The hint **narrows, never widens**: tools/vision requirements are derived from
the request itself (a hostile hint can't clear them), numbers are sanitized at
wire decode *and* again before resolution, an over-long override id is dropped
wholesale, and an override can only pick an *already-eligible* upstream. When no
`min_context` is asserted, a cheap chars/4 estimate stands in as the floor
filter (fail-soft: an upstream with an unknown window is never filtered out).

Precedence with named-reference role routing (`[digest] provider`,
`[instant] provider`, graph capability edges): a named pin always wins; an
unpinned slot routing through the task-router is decided by the policy under the
slot's role; a pin may itself name `"task-router"` (self-reference inside
`[route] upstreams` stays rejected).

Decisions are observable: `agent_router_decisions_total{role,task_mode,chosen,
rule}` (`rule` = matched index or `default` — bounded, never config text),
`agent_router_no_candidate_total{role}`, and a `route.select` debug event inside
the metered provider span. The decision hot path is benched, hardened, and
budgeted: `route_resolve` measures the whole path (~115k Ir), the isolated
decision (~34k), and the **production index path** `resolve_indices` (~22.7k —
the borrowed-view/index pass cut the decision 2.6×: `UpstreamMeta` borrows
id/tags, ordering resolves to fleet indices, no per-call `String` clones), plus
the chars/4 estimate (~3.8k for 24KiB). A concurrency stress test
(`route_stress`: 1,600 calls across 32 tasks over a 30-member flapping fleet
with hostile hints — liveness + exact decision accounting + post-storm
recovery) and a dhat leak budget (`route_leak`: failover path frees all
scratch, <120 blocks/call) gate it alongside the Ir ceilings.

## The provider registry (`[registry]`, `--serve-provider-registry`)

[Model-router 03](../design/model-router/03-registry-proto.md): the task-router's
fleet + policy as one proto contract, `agent.v1.ModelRouterConfig`, with two
faces over the same messages:

- **The textproto scenario file** — `agent --model-router-config FILE` (or
  `[agent] model_router_config` / `AGENT_MODEL_ROUTER_CONFIG`) parses a
  `config/model-router/*.textproto` at startup and **replaces** the TOML
  `[route]` block wholesale, then builds through the *same* factory chain (one
  build path — the two forms cannot route differently). Fail closed: a
  missing/unparseable/invalid file aborts the build; no partial fleet. Keep
  several scenario files under version control and swap fleets atomically.
- **`ProviderRegistryService`** (port 50084, metrics 9634) — the live control
  plane over the same messages: `List/Get/Put/Delete/Enable` on upstream cards,
  `GetPolicy/PutPolicy`, `Route` introspection (*what would you pick and why* —
  it runs the same `route::Policy` engine, so the answer is the router's), and
  `Health`. Swappable storage behind `[registry] store`: `file` (the same
  textproto bundle — hand-edited or `Put`-rewritten, one format), `sqlite`
  (feature `registry-sqlite`, prost-encoded blobs), `grpc` (a central registry),
  `""` (off). `agent_registry_mutations_total{op}` +
  `agent_registry_upstreams{enabled}` meter the control plane.

**Security.** A card's `api_key_ref` is a kind-prefixed *reference* —
`env:NAME` / `file:/path` — never a secret: keys resolve on the host that
builds the concrete provider, so a compromised registry has no key to serve; a
raw value is rejected (without being echoed). Every id is `safe_segment`-gated
before it can become a storage path or label; every number (cost, window,
weight, retries, concurrency) is clamped at wire decode *and* on store ingest;
sizes and counts are capped (`MAX_REGISTRY_UPSTREAMS`, rule/tag caps, a 1 MiB
textproto cap applied before parsing). All three stores share one `ops` module,
so validation cannot drift between backends.

**Registry-backed routing** ([04](../design/model-router/04-registry-backed.md)):
`[route] source = "registry"` swaps the static startup list for the live store —
the TOML `[route]` fleet *seeds* an empty registry once (idempotent; a
control-plane edit is never overwritten by a reboot), and a
`Put`/`Delete`/`Enable`/`PutPolicy` takes effect within
`[registry] refresh_secs` (0 = per call), no restart. The `RegistryRouter`
rebuilds its inner router only when the snapshot *fingerprint* changes, reusing
unchanged cards' provider instances (re-tagging never drops a connection); a
mid-refresh registry error keeps the last good fleet; hostile or unbuildable
cards are skipped with a warning, re-validated + re-clamped before any build.
A raw inline `api_key` refuses to seed (the registry stores references only).

**Live-signal ordering** (04, the 02-deferred `prefer.policy`):
`cost | latency | least-loaded` breaks ties among equally-preferred survivors
using the router's own dispatch accounting (an RAII in-flight counter + an
α=0.3 latency EWMA per upstream) — a *tie-break*, never an override: an
explicitly preferred upstream still wins regardless of its live numbers, and
unknown (`0`) values are neutral. Per-upstream dispatch is metered:
`agent_router_dispatch_total{role,upstream,outcome}`,
`agent_router_failover_total{from,to,reason}`, and the
`agent_router_upstream_inflight{upstream}` gauge (fed from both edges of the
RAII in-flight guard, so it drains to 0 even for cancelled calls). The classifier vote and review
fan-out stamp `classify`/`review` role hints on their requests (the fan-out
mechanism itself stays the pool's — a vote wants N independent answers).

## Deferred

- **Cost- and latency-based policies.** `in-order` and `round-robin` are
  implemented; cost-minimising routing needs per-candidate price metadata, which
  lives in `agent-tokenizer`'s `PriceTable` and is not yet plumbed to candidates.
  (For the task-router, live-signal `prefer.policy` ordering is
  [model-router 04](../design/model-router/04-registry-backed.md).)
- **Structured provider errors.** Classification is message-based (see above);
  a `status` on `Error::Provider` would make it exact.
- **Mid-stream failover**, which requires replay semantics the seam does not have.
