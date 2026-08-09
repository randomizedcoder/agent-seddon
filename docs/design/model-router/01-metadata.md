# 01 — Rich per-upstream metadata

Status: **planned** — see [`STATUS.md`](STATUS.md). The foundation increment: give every
upstream a **model card** (per-upstream context window, cost, capability tags) and, along the
way, make the pool usable for **authenticated / self-signed hosted endpoints** — which is what
lets Kimi + GLM go into one pool secret-safely (the immediate motivation).

> Implementation note: this increment is **in-process + TOML only** — no proto control plane yet
> (that is [03](03-registry-proto.md)). It widens the metadata the *existing* pool/router carry,
> so the wire changes are additive fields on `PoolMemberHealth`/`ModelCapabilities` — no baseline
> bump. It also fixes a real bug: inline pool members currently share one global context window.

## Motivation

Two problems, one metadata gap:

1. **The pool can't hold a real hosted endpoint.** The inline `[[pool.members]]` synth
   (`builder.rs:635-644`) hardcodes `insecure_tls: false` and reads only an inline `api_key`, and
   `PoolMemberCfg` has no `api_key_file`/`api_key_env`/`insecure_tls` fields. So GLM (self-signed
   + key-in-a-file) and Kimi (key-in-a-file) **cannot** be pool members without committing a
   secret — the pool was built for local, keyless GPU boxes.
2. **Routing has nothing to route on.** `ModelCapabilities` carries only `{supports_tools,
   context_window, supports_response_format, supports_vision}`. There is no cost, no capability
   taxonomy, and **context window is not per-upstream** — every inline member inherits
   `cfg.agent.context_window` (`builder.rs:641`). Per-token pricing lives in `agent-tokenizer`'s
   `PriceTable`, disconnected from candidates.

## What already exists (and its gaps)

- `ModelCapabilities` (`agent-core/src/lib.rs:73`) — 4 bools/ints, no cost/tags/latency.
- `PoolSpec` (`pool.rs:145`) / `PoolMember` (`pool.rs:157`) — `tier, cost, weight,
  max_concurrency` + measured signals; **no context window, no tags, no per-token cost.**
- `PoolMemberCfg` (`config.rs:329`) — `name, endpoint, model, api_key, tier, weight, cost,
  max_concurrency`; **no key-file/env, no insecure_tls, no context/tags.**
- `ProviderCfg` (`config.rs:1536`) — has `api_key_file`/`api_key_env`/`insecure_tls` and
  `resolve_api_key` (`builder.rs:1391`, precedence inline > env > file); **no `context_window`**
  (that is on `[agent]`).
- `is_capable(caps, req)` (`router.rs:109`) — tools/vision only; never consults context window.

## Design

### Per-upstream metadata on the card

Add to `ModelCapabilities` (and thread onto `PoolSpec`/`PoolMember`):

```rust
pub struct ModelCapabilities {
    pub supports_tools: bool,
    pub context_window: u32,            // now PER-upstream, not the global one
    pub supports_response_format: bool,
    pub supports_vision: bool,
    pub max_output_tokens: u32,         // additive
    pub input_cost_per_mtok: f32,       // additive — was only in PriceTable
    pub output_cost_per_mtok: f32,      // additive
    pub tags: Vec<String>,              // additive — coding|reasoning|long-context|cheap|…
}
```

`tags` is the free-form axis 02's routing rules match on; the numeric fields are the hard
filters (`context_window ≥ min_context`) and the cost-policy inputs the gpu-pool + router docs
list as deferred-pending-price-metadata. Cost defaults resolve from `agent-tokenizer`'s
`PriceTable` by model id when not set explicitly, so existing configs get real prices for free.

### Pool member gains key-file / env / TLS (the enabler)

Extend `PoolMemberCfg`:

```rust
pub struct PoolMemberCfg {
    // … existing …
    #[serde(default)] pub api_key_env: String,     // additive
    #[serde(default)] pub api_key_file: String,    // additive
    #[serde(default)] pub insecure_tls: bool,      // additive
    #[serde(default)] pub context_window: Option<u32>,  // additive — per-member window
    #[serde(default)] pub max_output_tokens: Option<u32>,
    #[serde(default)] pub input_cost: Option<f32>,
    #[serde(default)] pub output_cost: Option<f32>,
    #[serde(default)] pub tags: Vec<String>,
}
```

Refactor `resolve_api_key(p: &ProviderCfg)` into a free `resolve_key_opt(inline, env, file) ->
Result<String>` that applies the same **inline > env > file** precedence but **returns `""` when
none is set** (keyless local members stay valid; it still errors on an unreadable file).
`resolve_api_key` keeps its "no API key" hard error on top (the top-level provider still requires
one). In the member synth (`builder.rs:635-644`) replace `api_key: c.api_key.clone()` with
`resolve_key_opt(&c.api_key, &c.api_key_env, &c.api_key_file)?`, `insecure_tls: false` with
`insecure_tls: c.insecure_tls` (emit the same self-signed `warn!` as the top-level path,
`builder.rs:1268`), and `context_window: cfg.agent.context_window` with the per-member window
when set. Model prices fill from `PriceTable` when unset.

### Context-aware capability gate

Extend the one shared gate — both seams inherit it:

```rust
pub(crate) fn is_capable(caps: &ModelCapabilities, req: &CompletionRequest) -> bool {
    // … existing tools/vision …
    if req.min_context() > caps.context_window && caps.context_window != 0 { return false; }
    true
}
```

`min_context` derives from the request's message token estimate for now (a `RouteHint.min_context`
in [02](02-routing.md) makes it explicit); `0` = unknown window ⇒ don't filter (back-compat).

### The Kimi + GLM payoff

With the above, one pool holds both, no secret committed:

```toml
[agent]
provider = "pool"

[pool]
policy = "cost"                 # kimi (cost 0) preferred; glm on failover

[[pool.members]]
name = "kimi"
endpoint = "https://…-4000.proxy.runpod.net/v1"
model = "moonshotai/Kimi-K3"
api_key_file = "~/Downloads/runpod/glm/kimi-api-key"
context_window = 131072
tags = ["reasoning", "long-context"]
cost = 0

[[pool.members]]
name = "glm"
endpoint = "https://213.173.96.56:8000/v1"
model = "/model"
api_key_file = "~/Downloads/runpod/glm/glm-api-key"
insecure_tls = true             # self-signed dev endpoint
cost = 1
```

## Threading — no seam change

`LlmProvider` / `LlmPool` methods are untouched. Metadata lives on `ModelCapabilities` and the
pool member structs.

## Protobuf (additive — no baseline bump)

`ModelCapabilities` (in `common.proto`) gains `max_output_tokens`, `input_cost_per_mtok`,
`output_cost_per_mtok`, repeated `tags`; `PoolMemberHealth` gains `context_window` +
`input_cost_per_mtok`/`output_cost_per_mtok` so a remote pool exposes the card. `convert.rs` both
directions; all new fields default to `0`/empty for an old server; remote-reported numbers
**clamped on receipt**.

## Prometheus metrics

No new families required; `agent_pool_member_*` gain nothing here. (Cost-in-dollars accounting
stays on the existing `Cost`/`Usage` path.) A per-upstream `context_window` is exported as a
debug gauge only if it proves useful.

## Tracing + logs

`DEBUG` on member build: "member kimi: window=131072 tags=[reasoning,long-context] price=…".
No new spans.

## Testing (table-driven + adversarial)

- `positive_` — member with `api_key_file` resolves the key; per-member `context_window` reaches
  `OpenAiCompatConfig`; tags/costs round-trip; `PriceTable` fills an unset cost by model id.
- `negative_` — precedence env>file / inline>env; a member with `context_window` smaller than the
  request is filtered by `is_capable`.
- `corner_` — a **keyless** member (all three key fields empty) is allowed → empty key (local
  box, no regression); `context_window = 0` ⇒ not filtered.
- `boundary_` — window exactly equal to `min_context` passes; the largest-window member survives
  when others are filtered.
- `adversarial_` (**mandatory**) — `api_key_file` at a missing path fails **closed** with a clear
  error; `insecure_tls = true` reaches the config (and warns); **hostile** cost/window
  (NaN/neg/inf) clamped before any selection/price math; an over-long tag list is capped.

## Benchmark + leak

- **Bench** — the `eligible()` Ir ceiling holds; the added `context_window` compare is O(1) per
  member. Re-baseline only if the number legitimately moves.
- **Leak** — unchanged; no new allocation on the hot path (tags compared by ref).

## Security

- **No secret committed or logged** — keys via `api_key_file`/`api_key_env`; `resolve_key_opt`
  reads the file at build time only.
- `insecure_tls` per member is opt-in and **warns** (matches the top-level provider).
- All new numeric metadata (cost/window/max_output), including remote-reported, is clamped
  (finite, `≥ 0`) before it enters selection or pricing.

## Deferred to later increments

- The explicit `RouteHint.min_context` / tags-based routing ([02](02-routing.md)) — 01 only makes
  the metadata *exist* and filters on context window.
- Moving the card into proto/gRPC storage ([03](03-registry-proto.md)) — 01 stays TOML.
