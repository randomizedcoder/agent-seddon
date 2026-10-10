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

### 1. Retryable and *auth-terminal* failures fail over; request-terminals abort

A *request-level* terminal failure — billing, bad request, content policy, unknown
model — fails the same way on every candidate. Trying them all burns the chain, and
real money, to arrive at the identical answer. But an **auth-terminal (401/403) is
member-specific** — a rotated key or a dead/forbidden endpoint on one upstream says
nothing about the next — so those **do** fail over. Classification lives in
`agent-retry` and is shared rather than re-implemented (`classify` +
`is_auth_terminal`):

| Class | Fails over? | Examples |
|---|---|---|
| `Retryable` | yes | 429, 5xx, 529 overloaded, timeout, connection refused/reset |
| `Terminal` + auth (`is_auth_terminal`) | **yes** (member-specific) | 401/403, bare "unauthorized"/"forbidden"/"invalid api key" |
| `Terminal`, request-level | no (aborts) | 402 billing, 400 bad request, 404 model, content policy |

So a failover loop aborts only on `classify == Terminal && !is_auth_terminal(msg)`.
This is what lets a rotated Kimi pod (403) transparently fall over to GLM.

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

### Preferred generator vs. judge

The recommended dev split is **Kimi = the generator** (the more powerful model,
so it drives the main loop and reviews) and **GLM = the judge** (consensus
critic, fork judge, verifier). Express it in `[route]` (see the commented
template in `config/agent.toml`): tag GLM `"judge"`, add a lowest-precedence
`role = "judge"` rule that prefers the `"judge"` tag, and set
`default_prefer.upstreams = ["kimi", "glm"]` so everything else prefers Kimi and
falls back to GLM. Generation (role `main`/`review`) then lands on Kimi; judging
lands on GLM.

The committed **default provider stays local** (`provider = "openai-compat"` →
Ollama) so a fresh checkout runs offline. The RunPod endpoints are **ephemeral
pods** — the `base_url` changes when a pod restarts — so keep them in operator
config (or `--dart-define`/env), never as a committed default, and re-check the
URL with `curl <base>/v1/models` before assuming a pod is down. Keys are always
referenced (`api_key_file` / `api_key_ref: "file:…"`), never inlined.

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

### Fast 429 failover — router-owned retry (gap §8.7 item 9)

On the routed path, **retry lives on the router, not inside each upstream.** Routed
upstreams build **fail-fast** (`max_retries: 0` in `builder.rs` — both the
openai-compat synth and the anthropic synth), so a transient 429/5xx surfaces to the
`TaskRouter` immediately instead of burning ≤ 20 s × N of in-provider backoff first.
A single 429 on a busy upstream therefore fails over to one with headroom *at once*.

The router owns the budget via `with_retry_budget(max_retries)`:

- **One `op()` = one whole-fleet pass.** `route()` wraps a single `one_pass()` over a
  freshly-recomputed `order()` and feeds it to `agent_retry::run` (the one canonical
  retry impl) — there is no hand-rolled loop. A pass tries each candidate in turn;
  the first success returns, a request-level **terminal aborts the chain without
  spending budget**, and a pass that exhausts every candidate on transient failures
  reports `Retry` so the driver backs off **once** (jittered, capped at 20 s) and
  re-passes the fleet — up to `max_retries` re-passes. Failover *within* a pass stays
  sleepless, so the only wait is between passes, not per upstream.
- **Headroom-aware ordering.** `order()` sorts candidates into three buckets —
  **headroom** first, then those at their concurrency ceiling
  (`max_concurrency != 0 && in_flight >= max_concurrency`), then breaker-open
  (skipped-then-tried-last) — preserving policy order within each. So "fail over when
  another upstream has headroom" is a structural guarantee, not just the soft
  least-loaded tie-break.
- **Where the budget comes from.** Registry path: `max(card.max_retries)` over the
  enabled cards (`registry_router.rs`). Static `[route]` path: the new
  `[route] retry_budget` knob (default `2`, matching the in-provider count inline
  upstreams used to carry — same resilience, now as fast whole-fleet re-passes).
  Both clamp to `agent_core::MAX_UPSTREAM_RETRIES`. Default `0` on a bare
  `TaskRouter::new` keeps the historical single-pass behaviour.

Because retry moved off the upstream, a per-upstream `Retry-After` is not on the
error the router sees — the router uses its own capped jittered backoff between
passes rather than honouring one upstream's hint, which is the right call when the
goal is to move across the fleet fast. `max_retries` is also dropped from
`RegistryRouter::provider_key` (the built provider no longer depends on it), so a
budget-only card edit reuses the live connection and only rebuilds the router.

### Hard capacity — opt-in per-upstream cap (gap §8.7 item 3)

By default `max_concurrency` is **soft** on the router path
([model-router 05](../design/model-router/05-capacity-aware.md)): it normalises
`least-loaded` and `order()` defers a saturated upstream, but a saturated upstream is
still dispatched if the order reaches it. `[route] on_saturation` turns it into an
**admission cap** — the same semantics the fan-out pool has
([gpu-pool 02](../design/gpu-pool/02-capacity.md)), reusing its `Saturation` enum and
bounded poll:

| `on_saturation` | Saturated upstream | Every candidate saturated |
|---|---|---|
| `soft` (default) | still dispatched (reorder only) | — |
| `shed` | **skipped**, never dispatched | error `task-router saturated: …` at once |
| `wait` | **skipped**, never dispatched | poll every 25 ms for up to `saturation_wait_ms` (default 500, clamped ≤ 30 s), re-run the pass **once**, else shed |

- **Race-free.** A slot is taken by one CAS on the upstream's in-flight counter (the
  mirror of the pool's `PoolMember::try_reserve`), so N concurrent callers can never
  all pass the ceiling — a 10-call burst against a cap of 2 admits exactly 2.
- **Not a fault.** A skip is our own admission, so it never counts toward the
  breaker. A shed is `Attempt::Fail` — it spends **no** retry budget (the fleet is
  full, not flaky). A pass where some upstreams were dispatched and failed while the
  rest were saturated ends in the normal between-pass backoff, which gives slots time
  to free.
- **Streams hold their slot until drained** (the in-flight guard rides the returned
  stream), so a long generation keeps counting against the cap.
- **Observability.** `RouteEvent::SkippedSaturated` / `RouteEvent::Shed` map onto the
  existing route-decision counter as `skipped_saturated` / `shed`.
- Applies to both the static `[route]` fleet and the registry-backed fleet (re-applied
  on every rebuild). An upstream with `max_concurrency = 0` is uncapped in every mode.
- **Spill** to a reserve tier when saturated — see below (gap §8.7 item 8).

### Spillover tiers — `prefer.spill_to` (gap §8.7 item 8)

`spill_to = ["cloud"]` on a rule's `prefer` (or `[route.default_prefer]`, or the registry
`RoutePrefer.spill_to`) makes every eligible upstream carrying one of those tags a
**reserve**. The reserve is held back while a **primary** (an eligible upstream without
a spill tag) still has usable headroom, and is spilled onto only once none does — so
local GPUs absorb the load and the paid tier takes only the overflow.

- **"No headroom"** = every primary is saturated (`in_flight ≥ max_concurrency`) or has
  its breaker open. A spilled pass tries the reserve first, then the primaries; it
  re-evaluates on every retry pass, so traffic returns to the primaries as soon as a
  slot frees.
- **Works in every `on_saturation` mode.** Under `soft` the reserve leads the order once
  the primaries are full. Under `shed` / `wait` it is also the in-pass fallback when the
  primaries fill between planning and admission (the CAS refuses them), before any wait
  or shed; `wait` polls the reserve's slots too. A shed happens only when the reserve is
  full as well.
- **Capacity-driven, not error-driven.** A primary *with* headroom that fails (e.g. a
  429) does not pull the reserve into the pass — that is failover's job across
  primaries and the retry budget's across passes. A primary that keeps failing opens
  its breaker, which does count as "no headroom".
- **Fail-soft.** If only reserve upstreams can serve a request (e.g. only the cloud card
  supports tools), they serve it as primaries — spillover never refuses a request the
  fleet could answer. `spill_to` is scoped to the rule (or default) that ordered the
  request.
- **Fail-closed overrides.** A carried `override_upstream` naming a reserve upstream is
  dropped while a primary has headroom, so a request hint cannot pull a turn onto the
  paid tier early.
- **Observability.** `RouteEvent::Spilled { role }` → route-decision label `spilled`
  (once per pass that spills).
- Validated like `tags` on the registry path (≤ 32 entries, ≤ 64 bytes each).

```toml
[[route.upstreams]]
name = "mi50"            # primary: local GPU, 4 slots (endpoint/model/key as usual)
max_concurrency = 4
[[route.upstreams]]
name = "kimi"            # reserve: paid cloud
tags = ["cloud"]
max_concurrency = 16

[route]
on_saturation = "shed"
[route.default_prefer]
spill_to = ["cloud"]
```

### Turn pricing from the upstream card (gap §8.2)

The agent loop prices a turn under the configured `[provider] model`, which on a routed
turn is **not** the upstream that answered — and the built-in `PriceTable` only knows
Claude 3.x / GPT-4o, so Kimi / GLM / local turns used to cost **$0 "estimated"**. The
task-router knows the serving upstream, so it stamps `Usage.cost` from that upstream's
card (`input_cost` / `output_cost`, USD per Mtok, already clamped finite and
non-negative); the loop records a stamped cost as `actual` and only falls back to the
`PriceTable` when there is none.

- **Buffered and streamed.** `complete` prices the response; `stream` prices the
  terminal usage-bearing chunk. A failed-over turn is priced by the upstream that
  actually served it.
- **Unpriced ≠ free.** A card with both costs `0` stamps nothing, so the price-table
  fallback still applies. A cost the provider reported itself is never overwritten.
- **No cache double-billing.** The OpenAI-compatible `prompt_tokens` already *includes*
  cached tokens, and a card carries no cache discount, so cached input is billed once
  at the full input rate (an upper bound); the cache-read/-write lines stay `0`.
- **Re-pricing is live.** A registry `Put` that changes only a card's costs rebuilds
  the inner router (the snapshot fingerprint covers whole cards) but reuses the live
  connection (`provider_key` excludes routing metadata).
- Set `output_cost` beside `input_cost` on `[[route.upstreams]]` or the registry card.
  **Not yet** (rest of gap §8.7 item 7): cost-per-task ordering, fleet/tenant
  `max_cost`, spend budgets, cost metrics labelled by upstream.

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
  (feature `registry-sqlite`) and `postgres` (feature `registry-postgres`) —
  both `StoreRegistry` over the shared config-store `SqliteBackend`/`PgBackend`,
  prost-encoded card blobs (PG-11 retired the bespoke `SqliteRegistry`) — `grpc`
  (a central registry), `""` (off). `agent_registry_mutations_total{op}` +
  `agent_registry_upstreams{enabled}` meter the control plane.

**Security.** A card's `api_key_ref` is a kind-prefixed *reference* —
`env:NAME` / `file:/path` — never a secret: keys resolve on the host that
builds the concrete provider, so a compromised registry has no key to serve; a
raw value is rejected (without being echoed). Every id is `safe_segment`-gated
before it can become a storage path or label; every number (cost, window,
weight, retries, concurrency) is clamped at wire decode *and* on store ingest;
sizes and counts are capped (`MAX_REGISTRY_UPSTREAMS`, rule/tag caps, a 1 MiB
textproto cap applied before parsing). Every store shares one `ops` module,
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

Three registry-native card kinds synthesize at runtime, connection +
capabilities read straight off the model card: `openai-compat` (endpoint
required), `anthropic` (empty `base_url` = the public endpoint; `insecure_tls`
refused — the client has no cert bypass, and silently ignoring a TLS flag would
masquerade as a connect failure), and `grpc` (a lazily-dialed remote provider
seam; `base_url` required — no implicit localhost for a fleet entry).
Registered-**name** cards (`kind = ""`) are deliberately *not*
runtime-buildable: a registry entry — possibly written by a remote peer — must
never reach the local factory graph (a card naming `task-router` itself would
recurse into the router being rebuilt). They still work in the static TOML
`[route]` list, which is a local file.

Every registry-built fleet is stamped with its snapshot **fingerprint**: the
decision path emits it (`route.select` tracing target, `snapshot_version`
field; `0` = static fleet) so any routing choice is attributable to the exact
fleet version that produced it.

**Judge-env bridge** (opt-in): `[route] judge_from_env = true` maps the eval
harnesses' `AGENT_E2E_JUDGE_*` convention onto an appended `judge-env`
upstream plus a **lowest-precedence** `role = "judge"` rule — the harness
judge becomes a routed upstream instead of an env island. Explicit
upstreams/rules always win; without the env it is a warned no-op; the knob
survives a `--model-router-config` wholesale replace (deployment-local, not
fleet config); garbled or oversized env values fail closed.

**Live-signal ordering** (04, the 02-deferred `prefer.policy`):
`cost | latency | least-loaded` breaks ties among equally-preferred survivors
using the router's own dispatch accounting (an RAII in-flight counter + an
α=0.3 latency EWMA per upstream) — a *tie-break*, never an override: an
explicitly preferred upstream still wins regardless of its live numbers, and
unknown (`0`) values are neutral. **`least-loaded` orders by in-flight
*normalised* by `max_concurrency`** (the upstream's aggregate slot count), not
raw in-flight — so a multi-GPU **gateway** (one endpoint fronting N cards,
`max_concurrency ≈ N × per-GPU slots`) looks proportionally less loaded and
draws ~N× the traffic before it reaches parity. `max_concurrency = 0` (unset)
⇒ capacity 1, i.e. the raw-in-flight ordering, unchanged. See
[model-router 05](../design/model-router/05-capacity-aware.md). Per-upstream dispatch is metered:
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
