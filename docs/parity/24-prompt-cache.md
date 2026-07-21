# Parity spec 24 — prompt caching

Per-feature parity spec for a new `CacheStrategy` seam: decide **where** to place
cache breakpoints on the assembled prompt (Anthropic `cache_control` blocks;
OpenAI/other automatic-prefix reuse) so a multi-turn loop stops re-billing its
stable prefix on every turn. Tracks what agent-seddon ships today, what the peers
assert, and the concrete behaviour + tests needed to be the most complete of the
four.

> **Status: implemented** (`CacheStrategy` seam + `agent-cache` with
> `StablePrefix`/`TailWindow`, Anthropic `cache_control` serialization, OpenAI
> `prompt_cache_key`, config + span + bench + leak; doc in
> `docs/components/prompt-cache.md`). Departures from the plan below: **the
> provider holds the strategy** rather than the loop threading marks through
> `CompletionRequest` — placement depends on provider capabilities, which the
> provider knows, and it avoids touching 26 request-literal sites; this mirrors
> how `bash` receives the `Sandbox`. Observability is **span-only**
> (`cache.place`): the registry's provider factories are `Fn(&Config)` with no
> `Metrics` handle, and the cost-relevant numbers (cache-read/write tokens, hence
> hit-rate and tokens-saved) are already metered from `Usage` by spec 23.
>
> Implementation note worth carrying forward: wire indices are **not** input
> indices — `to_anthropic_messages` extracts system messages and coalesces
> same-role turns, so anchoring by input index lands on the wrong message and, in
> the common case, directly on the volatile tail. Valid JSON, silently destroyed
> hit rate. The converter now returns an input→wire map and placement refuses to
> anchor the tail's wire message; both are regression-tested.
>
> **Deferred:** the compaction cost/benefit policy (the strategy *sees*
> `compacted`, but nothing decides whether to compact based on cache value), the
> 1-hour TTL tier, and per-provider anchor dialects beyond Anthropic/OpenAI.
>
> Original plan follows. Introduces a new **`CacheStrategy` seam**
> in `agent-core` — `place_breakpoints(assembled_prompt) -> marked_prompt` — that
> annotates the model-ready message list with cache anchors *after* assembly but
> *before* the provider serializes the wire request. The provider (Anthropic
> `cache_control: {type: "ephemeral"}` on ≤4 blocks; OpenAI-family
> automatic-prefix reuse via a stable `prompt_cache_key`) consumes the marks and
> gates on its own capability. **Differentiator vs peers:** an *explicit,
> swappable* cache-**placement** policy (system prompt / tool defs / stable prefix
> vs volatile tail) that is itself a seam — reflection-introspectable, benchmarked,
> leak-tested — with **metered cache hit-rate and tokens-saved** read back from the
> provider `Usage` (cache-read / cache-write fields added in spec 23). Peers bury
> placement inside a provider adapter (hermes, opencode) or a gateway plugin
> (opencode Cloudflare); none expose it as a swappable, metered policy.

## Feature & why it matters

A coding agent's turn is dominated by a **large, stable prefix**: the system
prompt, the tool definitions (often the biggest block), and the accumulated
conversation history. Only the tail — the newest user/tool message — changes turn
to turn. Without prompt caching, the provider re-tokenizes and **re-bills the
entire prefix on every single turn** of the loop; on a 64K-token prompt over a
20-turn session that is >1M re-billed input tokens for zero new information, plus
the latency of re-ingesting them.

Anthropic's prompt cache lets the client mark up to **four** cache breakpoints
with `cache_control: {type: "ephemeral"}`; everything up to a marked block is
cached (5-minute default TTL, `0.1×` read pricing, `1.25×` write pricing on the
first miss). The cache is a **prefix** cache: a breakpoint only hits if every byte
before it is byte-identical to a prior request, so *where* you place the anchors
and *how stable* the prefix is decides the hit rate. OpenAI-family providers cache
automatically on a stable prefix (no explicit marks) but reward a stable
`prompt_cache_key`. hermes' field data shows the swing: with caching a Claude turn
progresses `1% → 67% → 84% → 97%` cache share within a session; a misconfigured
route (their MoA aggregator bug) silently dropped to `2%`, re-billing "tens of
millions of input tokens per benchmark run."

So this is a cost/latency multiplier that is trivial to get *wrong*: place a
breakpoint after a volatile block and you cache nothing; place it before the tools
and you miss the biggest cacheable block; let compaction rewrite the middle and
you **invalidate the whole downstream cache**. A first-class placement policy —
tested at exactly those boundaries — is the win.

## agent-seddon today

**Absent.** Prompts are assembled and sent, but **no cache breakpoints are ever
placed** and no cache tokens are accounted for.

- **Assembly (upstream of the gap):**
  [`crates/agent-context/src/lib.rs`](../../crates/agent-context/src/lib.rs)
  `assemble_messages` folds context + recalled memory into the system prompt and
  appends the trailing context; `ContextStrategy::assemble`
  ([`crates/agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs)) produces
  the `Vec<Message>`. There is **no hook** between assembly and provider send where
  a cache mark could be attached.
- **Anthropic provider:**
  [`crates/agent-providers/src/anthropic.rs`](../../crates/agent-providers/src/anthropic.rs)
  `build_body` emits `{model, max_tokens, messages, system, tools}` (via
  `to_anthropic_messages`). It sends `system` as a **plain string** and `tools` as
  a plain array — **no `cache_control` field is set anywhere** (confirmed:
  `rg -i cache crates/agent-providers/src/anthropic.rs` is empty). To carry a
  breakpoint the system prompt and tool defs would need to become **content-block
  arrays** so a block can carry `cache_control`.
- **OpenAI-compatible provider:**
  [`crates/agent-providers/src/openai_compat.rs`](../../crates/agent-providers/src/openai_compat.rs)
  sends no `prompt_cache_key` and does nothing to keep the prefix stable across
  turns (same grep: empty).
- **Usage accounting (ties to spec 23):**
  [`crates/agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs) `Usage`
  currently has **only** `prompt_tokens` / `completion_tokens` / `total_tokens` —
  **no cache-read / cache-write fields**, so even if the provider reported
  `cache_read_input_tokens` there is nowhere to store it and hit-rate/tokens-saved
  cannot be computed. Spec 23 adds those fields; this spec consumes them.

Net: to reach parity agent-seddon needs (1) a `CacheStrategy` seam and a call site
between `assemble` and provider send, (2) `cache_control`-capable serialization in
the Anthropic provider (+ a `prompt_cache_key` in the OpenAI provider), (3) the
`Usage` cache fields from spec 23, and (4) metrics reading them back.

## Peer implementations & their tests

| Peer     | Impl path | Test path | Framework |
| -------- | --------- | --------- | --------- |
| pi       | `pi/packages/ai/src/api/anthropic-messages.ts` (`getCacheControl`, breakpoint placement on system/tools/last-user block), `pi/packages/ai/src/api/openai-prompt-cache.ts` (`clampOpenAIPromptCacheKey`) | `pi/packages/ai/test/cache-retention.test.ts`, `.../openai-completions-cache-control-format.test.ts`, `.../openai-completions-prompt-cache.test.ts`, `.../anthropic-cache-write-1h-cost.test.ts`, `.../openai-codex-cache-affinity-e2e.test.ts` | vitest |
| hermes   | `hermes-agent/agent/agent_runtime_helpers.py` (`anthropic_prompt_cache_policy` — decide *whether*/which layout), `hermes-agent/agent/anthropic_adapter.py` (`_apply_assistant_cache_control_to_last_cacheable_block`, native vs envelope layout) | `hermes-agent/tests/agent/test_anthropic_adapter.py` | pytest |
| opencode | `opencode/packages/opencode/src/provider/transform.ts` (`applyCaching` — system[:2] + tail[-2] anchors, per-provider option key), `opencode/packages/core/src/plugin/provider/cloudflare-ai-gateway.ts` (gateway `cacheTtl`/`cacheKey`/`skipCache`) | `opencode/packages/opencode/test/provider/transform.test.ts` (39 cache refs), `.../test/session/llm-native.test.ts` | bun:test |

**pi** is the richest and closest to the proposed seam. `getCacheControl`
(`anthropic-messages.ts:57`) resolves an `{type:"ephemeral", ttl?}` control from a
`cacheRetention` option, then places it on: the **system** block, the **last tool
def** (`index === tools.length - 1`, gated by
`supportsCacheControlOnTools`), and the **last user message block** to "cache
conversation history" (`:1229`). It reads the results back from streamed usage —
`cache_read_input_tokens → usage.cacheRead`, `cache_creation_input_tokens →
usage.cacheWrite`, and the `ephemeral_1h_input_tokens` 1-hour split (`:569`). For
OpenAI-family it has no explicit marks but stabilizes a `prompt_cache_key`
(`openai-prompt-cache.ts`, clamped to 64 chars) so the provider's automatic prefix
cache hits — and tests the cost math (`cache-retention.test.ts`,
`anthropic-cache-write-1h-cost.test.ts`), the wire format
(`openai-completions-cache-control-format.test.ts`), and cache **affinity** across
requests (`openai-codex-cache-affinity-e2e.test.ts`).

**hermes** reuses a *per-conversation prefix cache* and a **placement-policy**
function: `anthropic_prompt_cache_policy` (`agent_runtime_helpers.py:1576`) returns
`(should_cache, use_native_layout)` — native layout puts markers on **inner
content blocks** (direct Anthropic / Anthropic-wire gateways), the looser
**envelope** layout puts them on the message dict (OpenRouter, OpenAI-wire proxies,
Kimi/Moonshot, Qwen/Alibaba). `anthropic_adapter.py` applies the control to "the
last cacheable block" and forwards any `cache_control` present on tool dicts. Their
comments are a catalog of the failure mode this spec guards: the MoA aggregator bug
("85% cache share solo vs 2% via MoA") and the Kimi fall-through ("~1% cache hits
on 64K-token prompts, re-billing the full prompt every turn").

**opencode** places anchors structurally in `applyCaching` (`transform.ts:322`):
the **first two `system` messages** plus the **last two non-system messages**
(`system[:2] ∪ tail[-2]`), deduped — i.e. anchor the stable head and the recent
tail, exactly the two-ended prefix pattern. It emits a **per-provider** option key
(`anthropic.cacheControl`, `openrouter.cacheControl`, `bedrock.cachePoint`,
`openaiCompatible.cache_control`, `copilot.copilot_cache_control`,
`alibaba.cacheControl`) and chooses **message-level vs content-level** placement by
provider — `transform.test.ts` asserts this per provider (39 cache references).
Separately, its **Cloudflare AI Gateway** plugin
(`cloudflare-ai-gateway.ts`) offers a *response-level* gateway cache keyed by
`cacheKey` with a `cacheTtl`/`skipCache` — a different, coarser cache than the
provider prefix cache, but part of the same cost story.

## Completeness gaps

Behaviour agent-seddon must add/guarantee to be the most complete (spec only — do
**not** implement here):

- **`CacheStrategy` seam.** New async trait in `agent-core`:
  `place_breakpoints(&self, prompt: AssembledPrompt, caps: &CacheCapabilities) ->
  MarkedPrompt`. `AssembledPrompt` exposes the logical regions (system, tool defs,
  history head, volatile tail) so a strategy reasons about placement, not raw
  bytes. Default impl `StablePrefix` (anchor after system + tool defs + last stable
  history block, leave the volatile tail unmarked); an alternative `TailWindow`
  (opencode-style head + recent-tail). Registered in
  [`agent-runtime/src/registry.rs`](../../crates/agent-runtime/src/registry.rs)
  `register_builtins`, config-selected in `config/agent.toml`.
- **Anthropic `cache_control` serialization.** Teach
  [`anthropic.rs`](../../crates/agent-providers/src/anthropic.rs) `build_body` to
  emit `system`/`tools`/message blocks as **content-block arrays** carrying
  `cache_control: {type:"ephemeral"}` where the strategy marked them. Enforce the
  provider's hard limit of **≤4 breakpoints** (drop lowest-priority marks past 4).
- **Stable-prefix detection.** A breakpoint only hits if everything before it is
  byte-identical to the prior turn. The strategy must place anchors **only on the
  provably-stable prefix** (system + tool defs are stable within a session; a
  history block is stable until compaction rewrites it) and **never** on the
  volatile tail (the newest message) — anchoring the tail caches a prefix that
  changes next turn (write cost, no read benefit).
- **Provider capability gating.** A `CacheCapabilities` probe per provider:
  Anthropic → explicit `cache_control`, ≤4, ephemeral, `supports_on_tools`;
  OpenAI-family → **no marks**, automatic prefix, honour a stable
  `prompt_cache_key` (clamp to 64 chars per pi). A non-caching provider ⇒ the
  strategy is a **no-op** (marks stripped, request unchanged byte-for-byte).
- **Metrics (cache-hit ratio + tokens-saved).** Requires the spec-23 `Usage`
  cache fields (`cache_read_tokens`, `cache_write_tokens`). Counters:
  `agent_cache_read_tokens_total`, `agent_cache_write_tokens_total`,
  `agent_prompt_tokens_total`; a **hit-rate** gauge = `cache_read / (cache_read +
  non_cached_input)`; **tokens-saved** = `cache_read_tokens` (billed at `0.1×`).
  A `cache.place` OTel span carries `{strategy, breakpoints_placed,
  provider_supports_cache, prefix_bytes}`.
- **Interaction with compaction (cost/benefit).** Compaction (spec 09,
  [`summarizing.rs`](../../crates/agent-context/src/summarizing.rs) /
  [`sliding_window.rs`](../../crates/agent-context/src/sliding_window.rs)) rewrites
  the middle of the window and thereby **invalidates every cache breakpoint
  downstream of the edit** — the next turn is a full cache **write** (`1.25×`), not
  a read. The strategy must (a) treat a just-compacted window as having **no stable
  history prefix** (anchor only system + tools that turn), and (b) expose the
  trade so a policy can decide whether to compact at all — compacting to save
  window space can cost more than it saves if it nukes a hot cache. This boundary
  is the marquee test.

## Table-driven test plan

Cases live next to the new impl in `crates/agent-cache/src/` (new crate, mirroring
`agent-context`): a `place_cases` table for placement, a `serialize_cases` table
for the Anthropic wire form, and an `accounting_cases` table for metered
hit-rate/tokens-saved driven by a scripted `Usage`. Reuse
[`agent-testkit`](../../crates/agent-testkit/src/lib.rs) `ScriptedProvider` (extend
it with a `with_usage(Usage)` builder so a turn can report cache-read/write tokens)
and `tempdir()` where a session log is needed. Placement is pure computation over a
message array — no I/O, no LLM call in the placement tests.

Case-prefix key: `positive_` places/accounts correctly, `negative_` rejects or
no-ops, `corner_` odd-but-valid input, `boundary_` a limit edge (≤4, empty,
compaction line). `(port: peer)` names the peer a case came from; `(new:
agent-seddon)` marks a case with no peer origin.

```rust
// crates/agent-cache/src/strategy.rs — placement over the assembled prompt.
// Signature idea: (prompt spec, provider caps) -> assertion on the marked prompt
// (which regions carry a breakpoint, total breakpoint count).
#[rstest]
#[case::positive_breakpoints_after_system_and_tools(
    // system + tool defs stable, one history block, volatile tail
    Prompt { system: 1, tools: 8, history: 4, tail: 1 },
    Caps::anthropic(),
    // expect anchors on: end-of-system, last tool def, last stable history block
    Expect::marked(&[Region::System, Region::Tools, Region::History]))] // (port: pi)
#[case::positive_tail_window_head_and_recent(
    Prompt { system: 2, tools: 4, history: 6, tail: 2 },
    Caps::anthropic(),
    Expect::strategy(TailWindow).marked(&[Region::System, Region::Tail]))] // (port: opencode)
#[case::negative_no_anchor_on_volatile_tail(
    Prompt { system: 1, tools: 4, history: 2, tail: 1 },
    Caps::anthropic(),
    // the newest message is NEVER marked (would cache a prefix that changes)
    Expect::unmarked(&[Region::Tail]))] // (new: agent-seddon)
#[case::boundary_at_most_four_breakpoints(
    // more markable regions than the Anthropic limit
    Prompt { system: 1, tools: 12, history: 10, tail: 1 },
    Caps::anthropic(),
    Expect::breakpoint_count_le(4))] // (port: pi|opencode)
#[case::negative_noop_for_non_caching_provider(
    Prompt { system: 1, tools: 8, history: 4, tail: 1 },
    Caps::none(), // e.g. a plain OpenAI-compat endpoint with no prefix cache
    // request must be byte-identical to the unmarked prompt
    Expect::breakpoint_count_eq(0).and_request_unchanged())] // (new: agent-seddon)
#[case::corner_openai_sets_stable_cache_key_no_marks(
    Prompt { system: 1, tools: 8, history: 4, tail: 1 },
    Caps::openai(),
    // no cache_control blocks; a stable prompt_cache_key (<=64 chars) instead
    Expect::breakpoint_count_eq(0).and_cache_key_stable_len_le(64))] // (port: pi)
#[case::boundary_tools_gating_off(
    // provider that does NOT support cache_control on tools
    Prompt { system: 1, tools: 8, history: 4, tail: 1 },
    Caps::anthropic_no_tool_cache(),
    Expect::unmarked(&[Region::Tools]).marked(&[Region::System]))] // (port: pi|hermes)
fn place_cases(#[case] prompt: Prompt, #[case] caps: Caps, #[case] expect: Expect) {
    // build AssembledPrompt from `prompt`; run CacheStrategy::place_breakpoints;
    // assert the marked regions + total breakpoint count match `expect`.
}
```

```rust
// crates/agent-cache/src/anthropic_wire.rs — the marked prompt serializes to the
// Anthropic content-block form with cache_control on exactly the marked blocks.
#[rstest]
#[case::positive_system_becomes_block_array_with_control(
    Prompt { system: 1, tools: 4, history: 0, tail: 1 },
    Expect::system_is_block_array().last_block_has(json!({"type":"ephemeral"})))] // (port: hermes|pi)
#[case::negative_envelope_layout_for_openrouter(
    // hermes native-vs-envelope split: OpenRouter puts the marker on the message,
    // not the inner content block
    Prompt::openrouter_claude(),
    Expect::marker_on_message_envelope())] // (port: hermes)
#[case::boundary_more_than_four_marks_drops_lowest_priority(
    Prompt { system: 1, tools: 12, history: 10, tail: 1 },
    Expect::exactly_four_cache_control_blocks_in_body())] // (port: pi)
fn serialize_cases(#[case] prompt: Prompt, #[case] expect: Expect) {
    // run place_breakpoints then build_body; assert the JSON body shape.
}
```

```rust
// crates/agent-cache/src/accounting.rs — metered hit-rate + tokens-saved read
// back from a scripted Usage (spec-23 cache fields). Uses ScriptedProvider
// extended with with_usage(...).
#[rstest]
#[case::positive_tokens_saved_from_cache_read(
    // provider reports a cache HIT: most input served from cache
    Usage { prompt_tokens: 1000, cache_read_tokens: 900, cache_write_tokens: 0, .. },
    Expect::tokens_saved(900).hit_rate_ge(0.9))] // (port: pi)
#[case::corner_first_turn_is_a_cache_write(
    // cold cache: everything written, nothing read (1.25x cost, 0 savings)
    Usage { prompt_tokens: 1000, cache_read_tokens: 0, cache_write_tokens: 1000, .. },
    Expect::tokens_saved(0).hit_rate_eq(0.0).wrote(1000))] // (port: pi)
#[case::boundary_compaction_invalidates_cache(
    // turn N: warm (900 read). turn N+1 after compaction: cold write again.
    Script::two_turns(
        Usage::hit(1000, 900),
        Usage::cold_after_compaction(1200)),
    Expect::turn2_hit_rate_eq(0.0).and_strategy_anchored_only_system_and_tools())] // (new: agent-seddon)
#[case::negative_no_cache_fields_zero_accounting(
    Usage { prompt_tokens: 1000, cache_read_tokens: 0, cache_write_tokens: 0, .. },
    Expect::tokens_saved(0).hit_rate_eq(0.0))] // (new: agent-seddon)
#[tokio::test]
async fn accounting_cases(#[case] script: Script, #[case] expect: Expect) {
    // ScriptedProvider::new(...).with_usage(script) drives a loop turn;
    // assert the cache metric family (read/write counters, hit-rate gauge,
    // tokens-saved) matches `expect` via agent-testkit MetricsProbe.
}
```

The `boundary_compaction_invalidates_cache` case is the marquee: it pins that a
just-compacted window reports a cold cache next turn **and** that the strategy
responds by anchoring only the still-stable system + tool defs (not a history
prefix the summarizer just rewrote).

## Harness obligations

The implementing PR (one feature, green under `nix flake check`) must land:

- **Seam + registry:** `CacheStrategy` trait in
  [`agent-core`](../../crates/agent-core/src/lib.rs) (`place_breakpoints`,
  `CacheCapabilities`); impls (`StablePrefix`, `TailWindow`) in a new
  `agent-cache` crate behind a `cache` cargo feature; one factory line in
  [`register_builtins`](../../crates/agent-runtime/src/registry.rs); doc in
  `docs/components/cache.md`.
- **Proto + gRPC:** extend the completion request wire form with **cache-marker
  metadata** (which blocks carry `cache_control`) and/or expose `CacheStrategy` as
  its own `--serve-cache` service with reflection; commit the `buf.image.binpb`
  bump via `nix run .#buf-image`; add the endpoint constant to `nix/constants.nix`
  → `nix run .#gen-constants`.
- **Tests:** the three `#[rstest]` tables above; extend the gRPC roundtrip test
  for the new seam; extend `ScriptedProvider` in `agent-testkit` with
  `with_usage(...)` so a scripted turn reports cache-read/write tokens.
- **Metrics + OTel:** `agent_cache_read_tokens_total` / `_write_tokens_total`,
  a cache-**hit-rate** gauge and a **tokens-saved** counter in `agent-metrics`; a
  metered decorator in [`metered.rs`](../../crates/agent-runtime/src/metered.rs);
  a `cache.place` span with `{strategy, breakpoints_placed,
  provider_supports_cache, prefix_bytes}` (matching the #44 attribute pattern).
- **Bench:** an iai-callgrind bench for the **CPU hot path** — `place_breakpoints`
  over a representative message array (system + N tool defs + M history + tail),
  with an absolute Ir ceiling in `nix/checks/bench.nix` (placement runs every
  turn, so it must stay cheap).
- **Leak:** a dhat `tests/leak.rs` (`dhat-heap` feature) asserting the placement +
  marked-prompt-serialization path frees everything it allocates and stays under
  budget across iterations.

## References

- **agent-seddon:** [`crates/agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs) (`Usage`, `CompletionRequest`/`Response`, `ContextStrategy`), [`crates/agent-context/src/lib.rs`](../../crates/agent-context/src/lib.rs) (`assemble_messages`), [`crates/agent-providers/src/anthropic.rs`](../../crates/agent-providers/src/anthropic.rs) (`build_body`, `to_anthropic_messages` — no `cache_control` today), [`crates/agent-providers/src/openai_compat.rs`](../../crates/agent-providers/src/openai_compat.rs) (no `prompt_cache_key`), [`crates/agent-context/src/summarizing.rs`](../../crates/agent-context/src/summarizing.rs) / [`sliding_window.rs`](../../crates/agent-context/src/sliding_window.rs) (compaction — cache invalidation source), [`crates/agent-testkit/src/lib.rs`](../../crates/agent-testkit/src/lib.rs) (`ScriptedProvider`), [`docs/parity/23-tokenizer-cost.md`](23-tokenizer-cost.md) (`Usage` cache fields), [`docs/parity/09-context-compaction.md`](09-context-compaction.md).
- **pi:** `pi/packages/ai/src/api/anthropic-messages.ts` (`getCacheControl`, breakpoints on system / last-tool / last-user block, `cache_read_input_tokens`/`cache_creation_input_tokens` → `usage.cacheRead`/`cacheWrite`), `pi/packages/ai/src/api/openai-prompt-cache.ts` (`clampOpenAIPromptCacheKey`, 64-char cap); tests `pi/packages/ai/test/cache-retention.test.ts`, `.../openai-completions-cache-control-format.test.ts`, `.../openai-completions-prompt-cache.test.ts`, `.../anthropic-cache-write-1h-cost.test.ts`, `.../openai-codex-cache-affinity-e2e.test.ts`.
- **hermes:** `hermes-agent/agent/agent_runtime_helpers.py` (`anthropic_prompt_cache_policy` — `(should_cache, use_native_layout)`, per-conversation prefix cache, MoA/Kimi/Qwen fall-through notes), `hermes-agent/agent/anthropic_adapter.py` (`_apply_assistant_cache_control_to_last_cacheable_block`, native vs envelope layout, `cache_control` on tool dicts); tests `hermes-agent/tests/agent/test_anthropic_adapter.py`.
- **opencode:** `opencode/packages/opencode/src/provider/transform.ts` (`applyCaching` — `system[:2] ∪ tail[-2]`, per-provider option key, message-level vs content-level), `opencode/packages/core/src/plugin/provider/cloudflare-ai-gateway.ts` (gateway `cacheTtl`/`cacheKey`/`skipCache`); tests `opencode/packages/opencode/test/provider/transform.test.ts`, `.../test/session/llm-native.test.ts`.
