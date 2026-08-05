# Parity spec 47 — reasoning-effort / thinking controls

Per-feature parity spec for a **provider-agnostic reasoning control**: one
`reasoning_effort` (low/medium/high) + `reasoning_summary` knob on the request
contract that each `LlmProvider` maps to its vendor's native shape — Anthropic
**extended thinking** (`thinking: {type, budget_tokens}` / adaptive `effort`),
OpenAI **reasoning** (`reasoning: {effort, summary}`) — plus first-class handling
of the **returned** thinking/reasoning content and its **token accounting**.
Tracks what agent-seddon ships today, what the peers assert, and the concrete
behaviour + tests needed to be the most complete of the four.

> **Status: ⬜ spec written, not started.** Introduces a typed **request-contract
> extension** on `agent_core::CompletionRequest` — `reasoning: Option<Reasoning>`
> carrying an **effort enum** (`Off | Low | Medium | High`, forward-compatible
> like spec-26's block list) and a `summary` mode (`Auto | Concise | Detailed |
> None`) — selected by a `[agent] reasoning_effort` / `reasoning_summary` config
> key, back-compat by serde default (`None` ⇒ omit, exact current wire). Each
> `LlmProvider` **encodes it per vendor** (Anthropic thinking-budget/adaptive,
> OpenAI `reasoning`) behind a **capability gate** (`ModelCapabilities.supports_reasoning`);
> a non-reasoning provider drops it silently. Returned thinking becomes a typed
> `ContentBlock::Reasoning` (building on spec-26's block model) and `Usage` gains
> a `reasoning_tokens` field metered like every other token class.
> **Differentiator:** reasoning is a **first-class, metered request parameter
> across ALL providers behind one contract** (the spec-26 multimodal pattern
> applied to a request knob) — the **Router** (spec 25) can pin effort per route
> and the **Tokenizer/cost** seam (spec 23) accounts reasoning output tokens — no
> peer exposes reasoning as a swappable, reflection-introspectable, metered gRPC
> seam contract. **Deferred:** interleaved/streamed reasoning-delta surfacing in
> the TUI (needs a streaming event channel the loop lacks today), encrypted
> reasoning-content replay across turns (`reasoning.encrypted_content`), and
> per-model adaptive-vs-budget auto-selection tables (ship a two-branch mapping
> first, learn the catalog later).
>
> Original plan follows. This is the design of record; **nothing here is
> implemented yet** — the `Reasoning` request field, the per-provider encoders,
> the capability gate, the returned-reasoning block, and the token accounting do
> not exist.

## Feature & why it matters

Frontier models now expose a **reasoning budget** knob: spend more hidden
tokens "thinking" before answering for hard tasks, spend none for a trivial
rename or a title. Anthropic ships this as **extended thinking**
(`thinking: {type:"enabled", budget_tokens}`, or adaptive `effort` on Claude
4.6+); OpenAI ships it as **reasoning** on the Responses API
(`reasoning: {effort:"low|medium|high", summary}`). The knob materially changes
**latency, cost, and answer quality** — a `high`-effort turn can 10× the output
tokens (and the bill), while `off` makes the cheap models fast. A coding agent
that cannot set it either burns money reasoning about `git status` or under-thinks
a gnarly refactor, with no lever in between.

The unit of control is **one provider-agnostic level** mapped per vendor, not a
vendor-specific field bolted onto each adapter. The interesting engineering is at
the edges, exactly mirroring spec-26 multimodal:

- **Per-provider encoding.** The *same* `Medium` must become Anthropic
  `thinking:{budget_tokens: N}` (or adaptive `{type:"adaptive"}` + `effort`) and
  OpenAI `reasoning:{effort:"medium"}` — two different wire shapes from one enum.
- **Capability gate.** A model with no reasoning support (`supports_reasoning:false`)
  must have the field **dropped silently** — sending `reasoning` to a
  non-reasoning endpoint 400s the whole request.
- **Returned reasoning is content.** The model streams back **thinking blocks**
  (Anthropic `{type:"thinking"}`, OpenAI `reasoning` summary/`reasoning_content`);
  today agent-seddon drops them (see below). They should be a typed block so the
  loop can render/log them and — critically — **re-send** the signed thinking
  block on the next turn where the vendor requires it.
- **Token accounting.** Reasoning tokens are billed output tokens but are *not*
  in the visible answer; `Usage` must carry them as a distinct class so the
  cost model (spec 23) and compaction budget are accurate.
- **Hostile values.** `reasoning_effort` comes from config **and** (via routing
  policy / prompts) from model-influenced surfaces, so an out-of-range or
  injected value must **clamp to a valid enum**, never reach the wire raw.

## agent-seddon today

**No reasoning control exists — anywhere.** There is no request field, no config
key, no response parsing, no metric. The two "reasoning" touchpoints are inert.

- **Request contract has no reasoning field.**
  [`crates/agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs) —
  `CompletionRequest { messages, tools, max_tokens: u32, temperature: f32,
  response_format: Option<ResponseFormat> }` (lines ~88–99). No `reasoning`,
  `reasoning_effort`, `reasoning_summary`, `thinking`, or `effort` field. The
  seam trait `LlmProvider` (~lines 178–210: `capabilities`/`complete`/`stream`)
  has no reasoning method.
- **`ModelCapabilities` cannot advertise reasoning.** Same file, ~lines 73–86:
  `{ supports_tools, context_window, supports_response_format, supports_vision }`.
  No `supports_reasoning` — the gate spec-26 added `supports_vision` for has no
  reasoning analogue.
- **`Usage` has no reasoning-token class.** ~lines 124–145:
  `{ prompt_tokens, completion_tokens, total_tokens, cache_read_tokens,
  cache_write_tokens, cost: Option<Cost> }`. Whether reasoning tokens are folded
  into `completion_tokens` is left entirely to whatever the upstream returns —
  the codebase neither separates nor accounts for them.
- **Anthropic sends no `thinking`; returned thinking is dropped.**
  [`anthropic.rs`](../../crates/agent-providers/src/anthropic.rs) `build_body`
  (~lines 72–149) emits only `model`, `max_tokens`, `temperature`, `messages`,
  `system`, `tools` — no `thinking` block, no `budget_tokens`. Inbound thinking
  blocks are explicitly ignored: the content-block enum buckets unknown types via
  `#[serde(other)]` (comment ~line 620 "Anything else (e.g. thinking blocks) is
  ignored"), and the test `corner_ignores_unknown_block` (~line 733) pins that a
  `{"type":"thinking",…}` block is dropped. Usage parsing
  (`SseUsage`, ~lines 423–432) has no reasoning field.
- **OpenAI-compat sends no `reasoning`; `reasoning_content` is log-only.**
  [`openai_compat.rs`](../../crates/agent-providers/src/openai_compat.rs)
  `WireReq` (~lines 402–416) carries no `reasoning_effort`. A returned
  `reasoning_content` (GLM-5.2 etc., `WireRespMsg.reasoning_content`, ~line 554)
  is emitted as a `tracing::debug!` "not resent" (~lines 189–194) and **never**
  placed into the returned `Message`. `WireUsage` (~lines 573–582) has no
  reasoning-token field.
- **No config key.** [`config/agent.toml`](../../config/agent.toml) mentions
  reasoning only in a comment (`max_tokens = 2048 # …reasoning models need more`);
  [`config.rs`](../../crates/agent-runtime/src/config.rs) `ProviderCfg`
  (`base_url`/`api_key`/`model`) and `RouterCfg`/`PoolMemberCfg` carry no
  reasoning field.
- **No metric.** [`metered.rs`](../../crates/agent-runtime/src/metered.rs) wraps
  `complete`/`stream` (`on_provider_request`/`on_provider_ttft`/`on_provider_error`)
  and token counters `add_tokens(model, prompt, completion)` /
  `add_cache_tokens` — no reasoning-token counter.

**Not the same thing:** the [`agent-mode`](../../crates/agent-mode/src/lib.rs)
crate (`HybridClassifier` implementing `agent_core::TaskClassifier`, enum
`TaskMode { Review, Implement, Design, Debug, Explain, Other }`) is a **task-mode
classifier** for adaptive cognition — *which kind of work* this turn is — not a
**provider reasoning-effort** knob. This spec adds the latter; the former could
one day *feed* it (a `Debug` turn requests `High` effort), which is the natural
spec-25 Router tie-in, not this schema change.

Closest seams to extend: `agent-core` (`CompletionRequest`/`Usage`/`ModelCapabilities`
+ the `ContentBlock` from spec 26), the two `LlmProvider` adapters, the config
schema, and `agent-metrics`. **No new seam trait** — like spec-26 multimodal, this
is a **typed extension to an existing contract**.

## Peer implementations & their tests

| Peer | Impl path | Test path | Framework |
| --- | --- | --- | --- |
| codex | `codex-rs/protocol/src/openai_models.rs` (`ReasoningEffort` enum), `codex-rs/protocol/src/config_types.rs` (`ReasoningSummary`), `codex-rs/codex-api/src/common.rs` (wire `Reasoning{effort,summary,context}`), `codex-rs/core/src/client.rs` (`build_reasoning` gate ~L816, `reasoning_effort_for_request` Ultra→Max ~L175), `codex-rs/protocol/src/protocol.rs` (`TokenUsage.reasoning_output_tokens` L2076) | `codex-rs/core/tests/suite/client.rs` (effort/summary mapping), `codex-rs/core/tests/suite/otel.rs` (reasoning-token count), `codex-api/src/sse/responses.rs` inline `#[test]` | cargo `#[test]` / insta |
| opencode | `packages/llm/src/schema/ids.ts` (`ReasoningEfforts`), `packages/opencode/src/provider/transform.ts` (`variants()` per-provider map ~L673: OpenAI `reasoningEffort` vs Anthropic `thinking{adaptive|enabled,budgetTokens}` vs Google `thinkingConfig`), `packages/llm/src/schema/messages.ts` (`ReasoningPart` ~L169) | `packages/opencode/test/provider/transform.test.ts`, `packages/llm/test/prepare.test.ts`, `packages/llm/test/provider/openai-{chat,responses}.test.ts` | bun:test + Effect |
| pi | `packages/ai/src/types.ts` (`ThinkingLevel`, `reasoning?`, `ThinkingContent` ~L333), `packages/ai/src/api/openai-responses.ts` (`reasoning:{effort,summary}` ~L270), `packages/ai/src/api/anthropic-messages.ts` (adaptive `effort` / `thinking:{budget_tokens}` ~L1006), `packages/ai/src/models.ts` (`clampThinkingLevel` ~L674) | `packages/ai/test/bedrock-thinking-payload.test.ts`, `anthropic-adaptive-thinking-models.test.ts`, `interleaved-thinking.test.ts`, `openai-completions-reasoning-details.test.ts` | vitest |
| hermes | `hermes_constants.py` (`VALID_REASONING_EFFORTS`, `parse_reasoning_effort`/`resolve_reasoning_config`), `agent/anthropic_adapter.py` (`THINKING_BUDGET`/`ADAPTIVE_EFFORT_MAP` ~L2624), `agent/transports/codex.py` (`reasoning:{effort}`), `agent/transports/chat_completions.py` (`reasoning_effort`/`thinkingConfig`) | `tests/test_hermes_constants.py`, `tests/agent/test_moa_reasoning_effort.py`, `tests/agent/test_anthropic_thinking_block_order.py`, `tests/plugins/model_providers/test_{minimax,upstage,opencode_go}_profile.py` | pytest |

All four peers ship the feature; **codex is the anchor** — a Rust codebase with the
exact typed-enum + wire-struct + capability-gate shape agent-seddon should mirror:

- **Typed effort enum, manual serde** ([`openai_models.rs`](../../../codex/codex-rs/protocol/src/openai_models.rs)
  L40): `ReasoningEffort { None, Minimal, Low, Medium (#[default]), High, XHigh,
  Max, Ultra, Custom(String) }` with a **hand-written** `Serialize`/`Deserialize`
  via `as_str()`/`FromStr` (L100–136) — an unknown string becomes `Custom`, empty
  is rejected. `ReasoningSummary { Auto (#[default]), Concise, Detailed, None }`
  ([`config_types.rs`](../../../codex/codex-rs/protocol/src/config_types.rs) L47,
  `rename_all="lowercase"`). Config keys `model_reasoning_effort` /
  `model_reasoning_summary` ([`core/src/config/mod.rs`](../../../codex/codex-rs/core/src/config/mod.rs) L950/L961).
- **Wire struct with per-field omission** ([`codex-api/src/common.rs`](../../../codex/codex-rs/codex-api/src/common.rs)
  L148): `Reasoning { effort: Option<…> #[skip_serializing_if=None], summary,
  context }`, hung on the request as `reasoning: Option<Reasoning>`.
- **Construction + capability gate** ([`core/src/client.rs`](../../../codex/codex-rs/core/src/client.rs)
  `build_reasoning` L816): effort falls back to the model catalog default; the
  **summary is only sent when `model_info.supports_reasoning_summary_parameter`**
  and not `None` (L825–827) — the exact capability-gate agent-seddon needs.
  `reasoning_effort_for_request` (L175) normalizes `Ultra→Max` before the wire.
  Tests: [`core/tests/suite/client.rs`](../../../codex/codex-rs/core/tests/suite/client.rs)
  `includes_configured_max_effort_in_request` (L2204),
  `includes_no_effort_in_request` (L2252),
  `includes_default_reasoning_effort_in_request_when_defined_by_model_info` (L2294),
  `configured_reasoning_summary_is_sent` (L2397),
  `model_without_summary_parameter_support_omits_configured_summary` (L2460),
  `reasoning_summary_is_omitted_when_disabled` (L2684).
- **Reasoning-token accounting** ([`protocol.rs`](../../../codex/codex-rs/protocol/src/protocol.rs)
  `TokenUsage.reasoning_output_tokens` L2076), parsed from SSE
  `output_tokens_details.reasoning_tokens`
  ([`codex-api/src/sse/responses.rs`](../../../codex/codex-rs/codex-api/src/sse/responses.rs)
  L141–160, inline `#[test]` L821 asserting `reasoning_output_tokens: 5`) and
  emitted to telemetry as `reasoning_token_count`
  ([`otel.rs`](../../../codex/codex-rs/core/tests/suite/otel.rs) asserts
  `reasoning_token_count=2`, L620).

**opencode** and **pi** both prove the *provider-agnostic level → two wire shapes*
mapping this spec centres on. opencode funnels one `ReasoningEffort` literal
through `ProviderTransform.variants()`
([`transform.ts`](../../../opencode/packages/opencode/src/provider/transform.ts)
~L673) to OpenAI `{reasoningEffort}` vs Anthropic
`{thinking:{type:"adaptive"|"enabled", budgetTokens}}` vs Google `{thinkingConfig}`,
with release-date tier gating (`gpt-5-chat` must **not** set it —
[`transform.test.ts`](../../../opencode/packages/opencode/test/provider/transform.test.ts)
`"gpt-5-chat should NOT set reasoningEffort"` L584). pi does the same from a
`reasoning?: ThinkingLevel` input clamped by `clampThinkingLevel`/`thinkingLevelMap`
([`models.ts`](../../../pi/packages/ai/src/models.ts) L674) → adaptive `effort` vs
`thinking:{budget_tokens}` ([`anthropic-messages.ts`](../../../pi/packages/ai/src/api/anthropic-messages.ts)
L1006–1022), tested in
[`bedrock-thinking-payload.test.ts`](../../../pi/packages/ai/test/bedrock-thinking-payload.test.ts)
(`"maps xhigh reasoning to effort=xhigh…"` L71, `"falls back to fixed-budget
thinking for non-adaptive Claude"` L230). Both model returned reasoning as a typed
part (`ReasoningPart` / `ThinkingContent`).

**hermes** is the config-normalization + clamp anchor:
`VALID_REASONING_EFFORTS = (minimal,low,medium,high,xhigh,max,ultra)` and
`parse_reasoning_effort` ([`hermes_constants.py`](../../../hermes-agent/hermes_constants.py)
L794/L799) coerce `none`/`false`/`disabled` → disabled, normalize case/whitespace,
and reject unknowns to the default — pinned by
[`test_hermes_constants.py`](../../../hermes-agent/tests/test_hermes_constants.py)
(`test_none_disables_reasoning` L435, `test_case_and_whitespace_normalized` L466,
the parametrized "every VALID level accepted" L451). Its adapters clamp
`xhigh→max` and collapse strong efforts
([`test_opencode_go_profile.py`](../../../hermes-agent/tests/plugins/model_providers/test_opencode_go_profile.py)
`test_strong_efforts_clamp_to_high` L54) — the model of the adversarial-clamp case.

## Completeness gaps

Behaviour agent-seddon must add/guarantee to be the most complete (spec only — do
**not** implement here). Each maps to a test case below.

- **Typed `Reasoning` on `CompletionRequest`.** Add `reasoning: Option<Reasoning>`
  where `Reasoning { effort: ReasoningEffort, summary: ReasoningSummary }`,
  `ReasoningEffort { Off, Low, Medium, High }` (a small, forward-compatible enum —
  an unknown serde string deserializes to a default, never errors, mirroring
  codex's `Custom`/hermes's default coercion), `ReasoningSummary { Auto, Concise,
  Detailed, None }`. `None`/absent ⇒ omit ⇒ byte-identical to today's wire. (Port
  codex `ReasoningEffort`/`ReasoningSummary`.)
- **`supports_reasoning` capability gate.** Add `supports_reasoning: bool` to
  `ModelCapabilities` (proto already carries `supports_tools`/`supports_vision`).
  A provider whose selected model isn't reasoning-capable **drops the field
  silently** — never emits an unsupported knob that 400s the request. (Port codex
  `supports_reasoning_summary_parameter` gate / opencode `gpt-5-chat` skip.)
- **Per-provider encoding (the crux, mirrors spec-26).** Anthropic adapter maps
  `effort` → `thinking:{type:"enabled", budget_tokens: N}` (a low/medium/high →
  budget table) or adaptive `{type:"adaptive"}` + `effort` for capable models, and
  sets `temperature: 1` where thinking requires it; OpenAI-compat adapter maps →
  `reasoning:{effort:"low|medium|high"}` (+ `summary` when supported). Same enum,
  two wire shapes. (Port opencode `variants()` / pi `streamSimple`.)
- **Returned reasoning as a typed block.** Extend spec-26's `ContentBlock` with
  `Reasoning { text: String, signature: Option<String> }`; Anthropic
  `{type:"thinking"}` blocks and OpenAI `reasoning_content`/summary deltas decode
  into it instead of being dropped/log-only. Providers **re-send** the signed
  block on the next turn where the vendor requires it (the `reasoning_details`
  echo hermes does). (Port pi `ThinkingContent` / opencode `ReasoningPart` /
  hermes `_copy_reasoning_content_for_api`.)
- **Reasoning-token accounting (spec-23 tie-in).** `Usage` gains
  `reasoning_tokens: u32`, populated from Anthropic/OpenAI usage details; the cost
  model bills it as output and compaction budgets with it. (Port codex
  `reasoning_output_tokens`.)
- **Config key.** `[agent] reasoning_effort = "off|low|medium|high"` +
  `reasoning_summary = "auto|concise|detailed|none"` in `ProviderCfg`, and a
  per-route override in `RouterCfg`/`PoolMemberCfg` so spec-25 routing can pin a
  cheap `off` model for compaction and a `high` model for the hard turn. (Port
  hermes `resolve_reasoning_config` per-model override > global.)
- **Hostile-value clamp (fail closed).** `reasoning_effort` from config **or** a
  model-influenced routing/prompt surface is clamped to the valid enum before it
  reaches `build_body` — an unknown/over-range/injected value becomes the default,
  never rides raw onto the wire. (Port hermes clamp/normalize; new adversarial
  requirement per repo security rules.)
- **Metered by effort + reasoning tokens (differentiator).** A request counter
  labelled `reasoning_effort = off|low|medium|high` (× provider), a
  `reasoning_tokens_total{model}` counter, and a `provider.reasoning` span
  attribute set (`effort`, `summary`, `reasoning_tokens`) reusing
  [`agent-metrics`](../../crates/agent-metrics/src/lib.rs) +
  [`agent-telemetry`](../../crates/agent-telemetry/) — no peer meters reasoning as
  a seam. (New.)

## Table-driven test plan

New `#[rstest]` tables in `agent-core` (enum serde/clamp) and the two provider
crates' `mod tests` (per-provider encoding + gate), plus a `Usage`-accounting
case and a proto roundtrip. No network: providers build request JSON from a
`CompletionRequest` and assert the emitted body; returned-usage cases feed a
canned response through the existing parse path. Doubles from
[`agent-testkit`](../../crates/agent-testkit/src/lib.rs). Case prefixes:
`positive_` succeeds, `negative_` rejects/omits, `corner_` odd-but-valid,
`boundary_` edge, `adversarial_` hostile-input (mandatory, asserts the clamp).
`(port: <peer>)` marks cases mined from a peer test; `(new: agent-seddon)` are ours.

```rust
// ---- effort maps to the RIGHT vendor wire field, per provider --------------
// One provider-agnostic ReasoningEffort -> two different wire shapes.
#[rstest]
#[case::positive_anthropic_medium_thinking_budget(                         // (port: opencode variants / pi)
    Provider::Anthropic, Effort::Medium,
    // asserts body["thinking"] == {"type":"enabled","budget_tokens": <med>} and temperature==1
    Wire::AnthropicThinking { budget: MED })]
#[case::positive_anthropic_high_thinking_budget(                           // (port: pi bedrock-thinking-payload)
    Provider::Anthropic, Effort::High, Wire::AnthropicThinking { budget: HIGH })]
#[case::positive_openai_medium_reasoning_effort(                           // (port: codex client.rs / opencode)
    Provider::OpenAiCompat, Effort::Medium,
    // asserts body["reasoning"] == {"effort":"medium"}, NOT a thinking block
    Wire::OpenAiReasoning { effort: "medium" })]
#[case::positive_openai_high_reasoning_effort(                             // (port: codex includes_configured_max_effort)
    Provider::OpenAiCompat, Effort::High, Wire::OpenAiReasoning { effort: "high" })]
#[case::corner_off_omits_field_entirely(                                   // (port: codex reasoning_summary_is_omitted_when_disabled)
    Provider::Anthropic, Effort::Off, Wire::NoReasoningKey)]               // byte-identical to today's body
#[tokio::test]
async fn effort_encodes_per_provider(#[case] p: Provider, #[case] e: Effort, #[case] want: Wire) {
    // build CompletionRequest{ reasoning: Some(Reasoning{ effort:e, .. }) };
    // build_body(&req); assert the emitted JSON matches `want` for provider `p`.
}

// ---- non-reasoning provider ignores the knob gracefully (capability gate) --
#[rstest]
#[case::negative_non_reasoning_model_drops_effort(                         // (port: codex model_without_summary_parameter, opencode gpt-5-chat)
    /*supports_reasoning=*/ false, Effort::High, Wire::NoReasoningKey)]    // dropped, request still valid
#[case::positive_reasoning_model_keeps_effort(
    /*supports_reasoning=*/ true, Effort::High, Wire::OpenAiReasoning { effort: "high" })]
#[tokio::test]
async fn capability_gate(#[case] supports: bool, #[case] e: Effort, #[case] want: Wire) {
    // ModelCapabilities{ supports_reasoning: supports, .. }; a non-reasoning model
    // MUST omit the field (no 400-triggering knob), never error the whole request.
}

// ---- summary only sent when supported (second gate) ------------------------
#[rstest]
#[case::positive_summary_sent_when_supported(true,  Summary::Concise, Some("concise"))] // (port: codex configured_reasoning_summary_is_sent)
#[case::negative_summary_omitted_when_unsupported(false, Summary::Concise, None)]        // (port: codex model_without_summary_parameter_support)
#[case::corner_summary_none_omits(true, Summary::None, None)]                            // (port: codex reasoning_summary_none_overrides)
#[tokio::test]
async fn summary_gate(#[case] supports_summary: bool, #[case] s: Summary, #[case] want: Option<&str>) { /* … */ }

// ---- reasoning tokens are counted in Usage ---------------------------------
#[rstest]
#[case::positive_anthropic_reasoning_tokens_parsed(                        // (port: codex reasoning_output_tokens)
    anthropic_usage_json(/*output=*/ 40, /*reasoning=*/ 25), Usage{ completion_tokens: 40, reasoning_tokens: 25, .. })]
#[case::positive_openai_reasoning_tokens_parsed(                           // (port: codex responses.rs #[test])
    openai_usage_json(/*completion=*/ 40, /*reasoning=*/ 5), Usage{ completion_tokens: 40, reasoning_tokens: 5, .. })]
#[case::boundary_no_reasoning_tokens_field_defaults_zero(                  // (new: agent-seddon; back-compat)
    usage_json_without_reasoning(), Usage{ reasoning_tokens: 0, .. })]
fn usage_accounts_reasoning_tokens(#[case] raw: Value, #[case] want: Usage) {
    // parse provider usage; assert reasoning_tokens is separated and the cost
    // model (spec 23) bills it as output. Missing field => 0 (serde default).
}

// ---- ADVERSARIAL: hostile effort value from config/model is clamped --------
#[rstest]
#[case::adversarial_unknown_effort_string_clamps_to_default(               // (port: hermes parse_reasoning_effort / clamp)
    json!("wildly-high; drop table"), Effort::Medium)]                    // unknown => default, never Custom-to-wire
#[case::adversarial_case_and_whitespace_normalized(                        // (port: hermes test_case_and_whitespace_normalized)
    json!("  HIGH \n"), Effort::High)]
#[case::adversarial_injected_object_rejected(                             // (new: agent-seddon; no wire injection)
    json!({"effort":"high","extra":"$(rm -rf)"}), Effort::Medium)]        // non-scalar => default
#[case::adversarial_numeric_effort_clamped(json!(999), Effort::Medium)]    // (new: agent-seddon)
#[case::boundary_off_alias_disables(json!("off"), Effort::Off)]           // (port: hermes none/false/disabled)
fn effort_deserialize_clamps(#[case] raw: Value, #[case] want: Effort) {
    // a hostile reasoning_effort from config or a model-influenced surface
    // deserializes to a VALID enum; assert the emitted wire body never contains
    // the raw hostile string (build_body(clamped) is injection-free).
    assert_eq!(serde_json::from_value::<Effort>(raw).unwrap_or_default(), want);
}
```

Proto roundtrip (extend [`crates/agent-grpc/tests/roundtrip.rs`](../../crates/agent-grpc/tests/roundtrip.rs)):
send a `CompletionRequest` with `reasoning: Some(Medium/Concise)` over TCP + UDS,
assert the `Reasoning` message survives the seam byte-identical, and a response
carrying a `ContentBlock::Reasoning` + `Usage.reasoning_tokens` decodes unchanged
(the additive-oneof, reflection-descriptor-stable assertion spec-26 uses for image
blocks). A legacy request with `reasoning: None` decodes exactly as today.

Prefix legend (repo convention): `positive_` expected success, `negative_`
expected omission/error, `corner_` odd-but-valid, `boundary_` at a limit,
`adversarial_` hostile input that must be rejected/clamped. `(port: <peer>)` names
the peer a case was mined from (`codex` for the enum/gate/token matrix, `opencode`/`pi`
for per-provider encoding, `hermes` for the clamp/normalize cases);
`(new: agent-seddon)` marks the reasoning-token default, injected-object rejection,
and metered-effort assertions with no peer analogue.

## Harness obligations

The implementing PR must satisfy all (follows the spec-26 multimodal shape):

- **Seam/schema:** **no new seam trait** — a typed extension to `agent-core`
  (`CompletionRequest.reasoning`, `ReasoningEffort`/`ReasoningSummary` enums with
  clamp-on-deserialize, `ModelCapabilities.supports_reasoning`,
  `Usage.reasoning_tokens`, `ContentBlock::Reasoning`) rippling into both
  `LlmProvider` adapters and the config schema. A `content_reasoning()` accessor
  and serde `#[serde(default)]` keep every existing text-only call site and old
  session/config untouched.
- **Proto + gRPC:** extend
  [`common.proto`](../../crates/agent-proto/proto/agent/v1/common.proto) with a
  `Reasoning` message on `CompletionRequest`, `ContentBlock` `reasoning` oneof arm,
  `ModelCapabilities.supports_reasoning`, and `Usage.reasoning_tokens` — all
  **additive**, so `buf breaking` passes untouched; commit the `buf.image.binpb`
  bump (`nix run .#buf-image`) recording the additive change. The gRPC roundtrip
  test is extended (TCP + UDS); reflection descriptors change only by the additive
  fields.
- **Metrics + OTel:** a request counter labelled `reasoning_effort` (× provider)
  and a `reasoning_tokens_total{model}` counter in
  [`agent-metrics`](../../crates/agent-metrics/src/lib.rs), wired through the
  [`metered.rs`](../../crates/agent-runtime/src/metered.rs) provider decorator; a
  `provider.reasoning` span attribute set (`effort`, `summary`, `reasoning_tokens`)
  reusing [`agent-telemetry`](../../crates/agent-telemetry/).
- **Router + tokenizer awareness (the differentiator):** the spec-25 `Router`
  candidate config gains a per-route `reasoning_effort` (pin `off` for a compaction
  route, `high` for the hard route); the spec-23 cost model bills
  `Usage.reasoning_tokens` as output — document both ties in
  `docs/components/reasoning.md`.
- **Bench:** an iai-callgrind bench over the **effort→wire encoding / clamp** path
  (deterministic CPU: parse a hostile value, clamp, emit body) with an Ir ceiling
  in `nix/checks/bench.nix` — a bounded, no-I/O hot path, unlike the network
  completion it feeds. (The network turn itself is I/O-bound and documents the skip.)
- **Leak:** a dhat `tests/leak.rs` (`dhat-heap` feature) over the
  request-build + returned-reasoning-block decode path, asserting the reasoning
  block and its buffers free after the turn and stay under an allocation budget.
- **Doc:** `docs/components/reasoning.md` — the per-provider mapping table
  (effort → Anthropic budget/adaptive vs OpenAI `reasoning.effort`), the capability
  gate, the clamp contract, and the spec-25/spec-23 ties.

## References

- **agent-seddon:**
  [`crates/agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs)
  (`CompletionRequest` ~L88, `Usage` ~L124, `ModelCapabilities` ~L73,
  `LlmProvider` ~L178; the `ContentBlock` from spec 26 this extends),
  [`crates/agent-providers/src/anthropic.rs`](../../crates/agent-providers/src/anthropic.rs)
  (`build_body` ~L72, ignored-thinking-block `corner_ignores_unknown_block` ~L733,
  `SseUsage` ~L423),
  [`crates/agent-providers/src/openai_compat.rs`](../../crates/agent-providers/src/openai_compat.rs)
  (`WireReq` ~L402, log-only `reasoning_content` ~L189, `WireUsage` ~L573),
  [`crates/agent-runtime/src/config.rs`](../../crates/agent-runtime/src/config.rs)
  (`ProviderCfg`/`RouterCfg`/`PoolMemberCfg`),
  [`crates/agent-runtime/src/metered.rs`](../../crates/agent-runtime/src/metered.rs)
  (provider decorator + token counters),
  [`crates/agent-metrics/src/lib.rs`](../../crates/agent-metrics/src/lib.rs),
  [`crates/agent-telemetry/`](../../crates/agent-telemetry/),
  [`crates/agent-proto/proto/agent/v1/common.proto`](../../crates/agent-proto/proto/agent/v1/common.proto),
  [`crates/agent-grpc/tests/roundtrip.rs`](../../crates/agent-grpc/tests/roundtrip.rs),
  [`crates/agent-testkit/src/lib.rs`](../../crates/agent-testkit/src/lib.rs);
  related specs [`26-multimodal.md`](26-multimodal.md) (the typed request-contract
  extension + capability-gate pattern this mirrors),
  [`25-model-routing.md`](25-model-routing.md) (per-route effort pin),
  [`23-tokenizer-cost.md`](23-tokenizer-cost.md) (reasoning-token cost accounting).
- **codex (anchor):**
  `codex-rs/protocol/src/openai_models.rs` (`ReasoningEffort` L40, manual serde L100–136),
  `codex-rs/protocol/src/config_types.rs` (`ReasoningSummary` L47),
  `codex-rs/codex-api/src/common.rs` (`Reasoning` wire struct L148),
  `codex-rs/core/src/client.rs` (`build_reasoning` gate L816, `reasoning_effort_for_request` L175),
  `codex-rs/protocol/src/protocol.rs` (`TokenUsage.reasoning_output_tokens` L2076),
  `codex-rs/codex-api/src/sse/responses.rs` (`reasoning_tokens` parse L141–160 + inline `#[test]` L821);
  tests `codex-rs/core/tests/suite/client.rs` (`includes_configured_max_effort_in_request` L2204,
  `includes_no_effort_in_request` L2252, `configured_reasoning_summary_is_sent` L2397,
  `model_without_summary_parameter_support_omits_configured_summary` L2460,
  `reasoning_summary_is_omitted_when_disabled` L2684),
  `codex-rs/core/tests/suite/otel.rs` (`reasoning_token_count=2` L620).
- **opencode:** `packages/llm/src/schema/ids.ts` (`ReasoningEfforts`),
  `packages/opencode/src/provider/transform.ts` (`variants()` per-provider map ~L673),
  `packages/llm/src/schema/messages.ts` (`ReasoningPart` ~L169),
  `packages/llm/src/providers/openai-options.ts`;
  tests `packages/opencode/test/provider/transform.test.ts`
  (`gpt-5-chat should NOT set reasoningEffort` L584, google `thinkingConfig` gating L345),
  `packages/llm/test/prepare.test.ts`,
  `packages/llm/test/provider/openai-{chat,responses}.test.ts`.
- **pi:** `pi/packages/ai/src/types.ts` (`ThinkingLevel` L77, `reasoning?` L296, `ThinkingContent` L333),
  `pi/packages/ai/src/api/openai-responses.ts` (`reasoning:{effort,summary}` ~L270),
  `pi/packages/ai/src/api/anthropic-messages.ts` (adaptive/budget mapping ~L1006–1022),
  `pi/packages/ai/src/models.ts` (`clampThinkingLevel` L674),
  `pi/packages/ai/src/api/simple-options.ts` (`clampReasoning`/`adjustMaxTokensForThinking` L47–76);
  tests `pi/packages/ai/test/bedrock-thinking-payload.test.ts`
  (`maps xhigh reasoning to effort=xhigh…` L71, `falls back to fixed-budget thinking…` L230),
  `anthropic-adaptive-thinking-models.test.ts`, `interleaved-thinking.test.ts`,
  `openai-completions-reasoning-details.test.ts`.
- **hermes:** `hermes-agent/hermes_constants.py` (`VALID_REASONING_EFFORTS` L794,
  `parse_reasoning_effort` L799, `resolve_reasoning_config` ~L959),
  `hermes-agent/agent/anthropic_adapter.py` (`THINKING_BUDGET`/`ADAPTIVE_EFFORT_MAP` ~L58/L67/L2624),
  `hermes-agent/agent/transports/codex.py` (`reasoning:{effort}` L173),
  `hermes-agent/agent/transports/chat_completions.py` (`_reasoning_config_for_model` L21),
  `hermes-agent/agent/transports/anthropic.py` (thinking-block parse ~L94–131);
  tests `hermes-agent/tests/test_hermes_constants.py` (`test_none_disables_reasoning` L435,
  `test_case_and_whitespace_normalized` L466, parametrized VALID-levels L451),
  `hermes-agent/tests/agent/test_moa_reasoning_effort.py`
  (`test_call_llm_builder_translates_reasoning_config_to_extra_body` L55),
  `hermes-agent/tests/plugins/model_providers/test_{minimax,upstage,opencode_go}_profile.py`
  (`test_strong_efforts_clamp_to_high` L54).
