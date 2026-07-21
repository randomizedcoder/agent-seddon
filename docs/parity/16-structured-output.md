# Parity spec 16 — structured output

Per-feature parity spec for schema-constrained completions: attach a caller-supplied
JSON Schema to a request, ask the provider to honour it (`response_format` / tool-choice
where native, prompt-injected schema otherwise), **validate** the model's output against
the schema, and do a **one-shot repair** round-trip when it does not match.

> **Status: implemented** (seam + validator + repair loop + observability + bench +
> leak). New **`OutputSchema` validator seam** in `agent-core`
> (`validate(&schema,&value) -> Verdict`), a `response_format: Option<ResponseFormat>`
> field on `CompletionRequest` + a `supports_response_format` capability, a
> dependency-free draft-07-subset validator in [`agent-validate`](../../crates/agent-validate),
> and a bounded **one-shot repair loop** (`agent_runtime::structured::complete_structured`,
> reached via `Agent::complete_structured`). **Differentiator landed:** the
> model-in-the-loop repair no peer has (hermes validates but doesn't repair;
> opencode/pi validate tool I/O, not a general completion). Metered
> (`agent_structured_total{outcome=pass|repaired|exhausted}` +
> `agent_structured_validate_seconds`) and traced (`structured.validate` /
> `structured.repair` spans). **Deferred to a follow-up** (staged like the
> tokenizer / web / tasks seams): native provider `response_format` serialization
> (the flag is `false` today → the helper prompt-injects the schema) and the
> `Validator` gRPC service + `ResponseFormat` proto field. See
> [`docs/components/structured-output.md`](../components/structured-output.md).

## Feature & why it matters

An agent is far more useful when a completion can be *forced* into a known shape: a
subagent that must return `{"files": [...], "confidence": 0.0..1.0}`, a classifier that
must return one of a fixed enum, a planning step whose output is fed to another tool.
Without a schema contract the caller string-parses free text and silently mis-handles the
frequent cases where the model wraps JSON in prose, emits a trailing comma, drops a
required field, or invents an extra key.

Structured output closes that gap in three moves:

1. **Attach** a JSON Schema to the request (`response_format`).
2. **Constrain** generation — use the provider's native `response_format` /
   `json_schema` tool-choice when the model supports it; fall back to **injecting the
   schema into the prompt** for providers that do not.
3. **Validate** the returned JSON against the schema, and on mismatch **repair once**:
   feed the validation error back to the model and retry, bounded, before surfacing a
   hard error.

The payoff is reliability: fewer parse failures at the call site, **validated subagent
returns** (the parent can trust the shape), and machine-consumable agent output that a
downstream system can deserialize without defensive parsing.

## agent-seddon today

**Absent.** The `LlmProvider` seam has no concept of a response schema, and nothing
validates *model* output against a schema.

- **Provider seam:** [`crates/agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs)
  — `trait LlmProvider` (≈ line 179) exposes only `capabilities`, `complete`, and
  `stream`. `struct CompletionRequest` (≈ line 130) carries `messages`, `tools`,
  `max_tokens`, `temperature` — **no `response_format`**. `CompletionResponse` /
  `CompletionChunk` return a `Message` with free-text `content`; there is no verdict, no
  parsed value.
- **Tool schemas are input-only.** `ToolSchema.parameters`
  ([`agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs) ≈ line 215) is a
  `serde_json::Value` JSON Schema for the *arguments* the model sends **to** a tool, and
  each `Tool::schema()` advertises it. That schema describes tool **input**; nothing
  validates the model's textual **output**, and tool args themselves are passed to
  `Tool::execute(args, ctx)` un-validated against `parameters`.
- **Wire contract:** [`crates/agent-proto/proto/agent/v1/common.proto`](../../crates/agent-proto/proto/agent/v1/common.proto)
  `message CompletionRequest` (line 107) mirrors the Rust struct — no `response_format`
  field, no validator service.
- **Available:** `serde_json` is already a workspace dep of both `agent-core` and
  `agent-tools` ([`crates/agent-core/Cargo.toml`](../../crates/agent-core/Cargo.toml),
  [`crates/agent-tools/Cargo.toml`](../../crates/agent-tools/Cargo.toml)), so a
  draft-07-subset validator can be written against `serde_json::Value` (or a small
  jsonschema crate behind the feature) with no new core dependency surface.

Closest seam to extend: `LlmProvider` (add the request field + native constraint
plumbing); the validate/repair logic is a **new** `OutputSchema` seam invoked by the
runtime, not a provider concern (so it works uniformly across every provider).

## Peer implementations & their tests

| Peer | Impl path | Test path | Framework |
| --- | --- | --- | --- |
| hermes-agent | `agent/plugin_llm.py` (`complete_structured`, `_build_structured_messages`, `_parse_structured_text`) | `tests/agent/test_plugin_llm.py` | pytest + `unittest.mock` |
| opencode | `packages/core/src/tool/tool.ts` (`definition`/`settle`: typed `Input`/`Output`/`structured` schemas, `outputSchema`) | `packages/core/test/session-runner-tool-registry.test.ts`, `packages/core/test/tool-output-store.test.ts` | bun:test + `effect` |
| pi | `packages/ai/src/utils/validation.ts` (`validateToolArguments`, TypeBox `Compile`/`Value.Check`/`Value.Errors` + coercion); `packages/ai/src/api/openai-completions.ts` (`tool_choice`) | `packages/ai/test/validation.test.ts`, `packages/ai/test/openai-completions-tool-choice.test.ts`, `packages/ai/test/mistral-tool-schema.test.ts` | vitest |

**hermes-agent — the closest analogue (schema-constrained generation + validation).**
`agent/plugin_llm.py` exposes `complete_structured(instructions=…, input=[…],
json_mode=…, json_schema=…)` (+ an async `acomplete_structured`). `_build_structured_messages`
**injects the schema into the prompt** — a system directive when `json_mode` is set, and
the JSON-serialized schema appended to the header when `json_schema` is given
(`test_json_mode_adds_system_directive`, `test_schema_name_appended_to_header`), and it
**emits a provider `response_format` extra-body** (`test_complete_structured_emits_response_format_extra_body`)
— i.e. native constraint *plus* prompt fallback. `_parse_structured_text` strips code
fences, `json.loads`, and — when a schema is present — runs `jsonschema.validate`, raising
`"…did not match schema: …"` on mismatch (`_parse_structured_text` ≈ line 456). Tests:
`test_parse_valid_json_with_json_mode`, `test_parse_returns_text_on_invalid_json`,
`test_schema_validation_accepts_match`, `test_schema_validation_rejects_mismatch`,
`test_complete_structured_returns_parsed_json`,
`test_complete_structured_returns_text_on_unparseable_response`,
`test_complete_structured_validates_against_schema`. Note: hermes **raises** on
validation failure — there is **no repair round-trip** (the differentiator agent-seddon
adds). (Optional skills `outlines`/`instructor` document grammar-constrained decoding and
Pydantic-retry patterns but are skill docs, not core code.)

**opencode — typed tool I/O schemas.** `packages/core/src/tool/tool.ts` gives every tool
a typed `Input`, `Output`, and optional `structured` `effect/Schema`; `settle` runs
`Schema.decodeUnknownEffect(config.input)(call.input)` on the way in (→ `"Invalid tool
input: …"`) and `Schema.encodeEffect(config.output)(output)` on the way out (→ `"Tool
returned an invalid value for its output schema: …"`), and `definition` publishes both
`inputSchema` and `outputSchema` as JSON Schema. This is **schema-validated tool output**,
tested via the registry-settlement suite (`session-runner-tool-registry.test.ts`) and
output bounding (`tool-output-store.test.ts`). It validates *tool* output, not a general
model completion, and does not repair.

**pi — TypeBox tool-arg validation + tool-choice.** `packages/ai/src/utils/validation.ts`
compiles the tool's TypeBox `parameters` (`Compile`), runs `validator.Check`, coerces
primitives/unions (`Value.Convert`, `coerceWithJsonSchema`), and formats
`validator.Errors` with a JSON path — the model's tool-call **arguments** are validated
against the tool schema (`validation.test.ts`, `mistral-tool-schema.test.ts` for symbol
stripping). `openai-completions.ts` sets `params.tool_choice` from `options.toolChoice`
(`openai-completions-tool-choice.test.ts`) — the *steering* half of structured output.
pi validates tool **inputs**, not free-form structured output, and does not repair.

Summary of what agent-seddon exceeds: hermes validates but **does not repair**; opencode
and pi validate *tool I/O*, not a general completion; **none** run validation as a
distributed, metered, gRPC-served seam, and **none** do a bounded model-in-the-loop
repair.

## Completeness gaps

Behavioural targets to be the most complete of the four (spec only — do **not** implement
here):

- **Schema attach on the request.** Add `response_format: Option<ResponseFormat>` to
  `CompletionRequest`, where `ResponseFormat` is `{ schema: serde_json::Value, strict:
  bool, name: Option<String> }`. Absent ⇒ today's free-text behaviour, unchanged (every
  existing provider keeps working).
- **Native `response_format` vs prompt-injected fallback.** When
  `capabilities().supports_response_format` (a new capability flag), pass the schema to
  the provider's native `response_format` / `json_schema` tool-choice. Otherwise
  **inject** the serialized schema into the prompt (mirroring hermes
  `_build_structured_messages`) so *every* provider gains structured output, degrading to
  best-effort steering — validation still gates the result.
- **Validation (draft-07 subset).** The `OutputSchema` seam validates the parsed JSON
  against the schema: `type` (incl. arrays/nested), `required`, `properties`,
  `additionalProperties: false` (reject unknown keys), `enum`, and basic numeric/string
  bounds. Strip code fences and `json.loads`-equivalent before validating (hermes
  `_strip_code_fences`). Non-JSON output ⇒ a validation failure, not a panic.
- **One-shot repair loop with bounded retries.** On mismatch, re-prompt **once** with the
  original request plus the validation error (and the offending output) appended as a
  user/system turn, then re-validate. Bound by `max_repairs` (default 1); if the repaired
  attempt still fails, surface a hard error. This is the piece **no peer** has.
- **Error surfacing.** Distinguish (a) *unparseable* output (not JSON), (b)
  *schema-mismatch* (valid JSON, wrong shape — name the failing JSON path, like pi's
  `formatValidationPath`), and (c) *repair-exhausted* (still invalid after the bounded
  retries) as distinct, machine-readable error variants.
- **Proto-typed verdict + service (differentiator).** Extend `common.proto` with
  `ResponseFormat` and add it to `CompletionRequest`; add a `Validator` service
  (`Validate(schema, value) -> Verdict{ ok, errors[] }`) so validation itself is a
  dialable seam, reflection-introspectable via `grpcurl`.
- **Metrics + spans (differentiator).** Meter validation-pass / repair-triggered /
  repair-exhausted rates and validation latency; open a `structured.validate` span (attrs:
  schema name, ok, error-count) and a `structured.repair` span (attr: attempt) per
  attempt.

Each maps to a test case below.

## Table-driven test plan

Two tables. **(A)** a pure-validator table for the `OutputSchema` seam (no provider — just
schema × value → verdict), matching the `edit_cases` shape (`Ok`/`Err(substr)`). **(B)** a
runtime-integration table driving the repair loop with a scripted provider. Doubles come
from `agent-testkit`: **`ScriptedProvider`**
([`crates/agent-testkit/src/lib.rs`](../../crates/agent-testkit/src/lib.rs) ≈ line 64) —
its `complete` replays `responses` **in order** and **clamps to the last** once exhausted
(`fetch_add(1).min(len-1)`), so a two-element `[bad, good]` script replays bad-then-good
exactly, which is precisely what a one-shot repair needs. `tool_turn` / `final_turn`
build the turns; `RecordingMemory` / `StaticContext` supply the rest of the loop.

**Prefixes:** `positive_` passes, `negative_` rejects, `corner_` odd-but-valid,
`boundary_` schema edges. Tags: `(port: peer)` names the peer the case mirrors; `(new:
agent-seddon)` marks cases with no peer origin (notably every repair case).

```rust
use agent_core::Result;
use rstest::rstest;
use serde_json::{json, Value};

// --- Table A: the OutputSchema validator seam (pure) ------------------------
// schema × candidate value → Ok(()) on match, Err(substr) naming the failure.

#[rstest]
#[case::positive_type_match(
    json!({"type": "object", "properties": {"n": {"type": "integer"}}, "required": ["n"]}),
    json!({"n": 42}),
    Ok(()))] // (port: pi validation.test.ts | hermes accepts_match)
#[case::positive_enum_member(
    json!({"type": "string", "enum": ["a", "b", "c"]}),
    json!("b"),
    Ok(()))] // (port: pi)
#[case::negative_required_missing(
    json!({"type": "object", "properties": {"n": {"type": "integer"}}, "required": ["n"]}),
    json!({}),
    Err("required"))] // required key `n` absent // (port: hermes rejects_mismatch)
#[case::negative_type_mismatch(
    json!({"type": "object", "properties": {"n": {"type": "integer"}}, "required": ["n"]}),
    json!({"n": "not-a-number"}),
    Err("type"))] // (port: hermes | pi)
#[case::negative_additional_properties(
    json!({"type": "object", "properties": {"n": {"type": "integer"}},
           "additionalProperties": false}),
    json!({"n": 1, "extra": true}),
    Err("additional"))] // unknown key `extra` // (new: agent-seddon)
#[case::negative_enum_nonmember(
    json!({"type": "string", "enum": ["a", "b"]}),
    json!("z"),
    Err("enum"))] // (port: pi)
#[case::corner_nested_object_ok(
    json!({"type": "object", "properties": {
        "outer": {"type": "object", "properties": {"inner": {"type": "boolean"}},
                  "required": ["inner"]}}, "required": ["outer"]}),
    json!({"outer": {"inner": true}}),
    Ok(()))] // (new: agent-seddon)
#[case::negative_nested_mismatch(
    json!({"type": "object", "properties": {
        "outer": {"type": "object", "properties": {"inner": {"type": "boolean"}},
                  "required": ["inner"]}}, "required": ["outer"]}),
    json!({"outer": {"inner": 1}}),
    Err("inner"))] // path names the offending nested field // (port: pi formatValidationPath)
#[case::boundary_array_of_typed_items(
    json!({"type": "array", "items": {"type": "integer"}}),
    json!([1, 2, 3]),
    Ok(()))] // (new: agent-seddon)
fn validate_cases(
    #[case] schema: Value,
    #[case] value: Value,
    #[case] expected: std::result::Result<(), &str>,
) { /* OutputSchema::validate(&schema, &value) → assert Ok / err contains substr */ }

// --- Table B: the runtime one-shot repair loop ------------------------------
// A ScriptedProvider replays [attempt1, attempt2(, …)]; the loop validates each
// against `schema`, repairs up to `max_repairs`, and returns the parsed value or
// a hard error.

enum Want<'a> {
    /// Final validated JSON the loop returns.
    Value(Value),
    /// The loop returns Err whose message contains this substring.
    Error(&'a str),
    /// Also assert how many provider round-trips happened (1 = no repair).
    ValueAfter(Value, usize),
}

#[rstest]
// (new: agent-seddon) first output already valid → returned as-is, no repair.
#[case::positive_valid_first_try(
    /* schema: {n:int required} */
    /* script: [ final_turn(r#"{"n": 7}"#) ] */
    /* max_repairs: 1 */
    Want::ValueAfter(json!({"n": 7}), 1))]
// (new: agent-seddon) bad-then-good: invalid first output triggers ONE repair,
// second output validates → returned. ScriptedProvider replays [bad, good].
#[case::positive_repair_then_pass(
    /* script: [ final_turn(r#"{"n": "x"}"#), final_turn(r#"{"n": 7}"#) ] */
    /* max_repairs: 1 */
    Want::ValueAfter(json!({"n": 7}), 2))]
// (new: agent-seddon) repair exhausted: both outputs invalid, max_repairs=1 →
// hard error naming exhaustion. ScriptedProvider clamps to the last (still bad).
#[case::negative_repair_exhausted(
    /* script: [ final_turn(r#"{"n": "x"}"#), final_turn(r#"{"n": "y"}"#) ] */
    /* max_repairs: 1 */
    Want::Error("repair"))]
// (port: hermes returns_text_on_unparseable_response) non-JSON output is a
// validation failure; one repair to valid JSON → passes.
#[case::corner_unparseable_then_repaired(
    /* script: [ final_turn("here is your answer: none"), final_turn(r#"{"n": 1}"#) ] */
    Want::ValueAfter(json!({"n": 1}), 2))]
// (port: hermes _strip_code_fences) fenced ```json block is stripped, then
// validates on the first try (no repair).
#[case::corner_fenced_json_stripped(
    /* script: [ final_turn("```json\n{\"n\": 3}\n```") ] */
    Want::ValueAfter(json!({"n": 3}), 1))]
// (new: agent-seddon) additionalProperties violation on first try → repair →
// clean object passes.
#[case::negative_additionalprops_then_repaired(
    /* schema adds additionalProperties:false */
    /* script: [ final_turn(r#"{"n":1,"junk":2}"#), final_turn(r#"{"n":1}"#) ] */
    Want::ValueAfter(json!({"n": 1}), 2))]
// (new: agent-seddon) max_repairs=0 disables repair: one bad output → immediate error.
#[case::negative_no_repair_budget(
    /* max_repairs: 0; script: [ final_turn(r#"{}"#) ] */
    Want::Error("schema"))]
#[tokio::test]
async fn structured_repair_cases(
    #[case] schema: Value,
    #[case] provider: agent_testkit::ScriptedProvider,
    #[case] max_repairs: usize,
    #[case] want: Want<'_>,
) {
    // Build a request with response_format = ResponseFormat{ schema, strict:true, .. },
    // run the structured completion helper (OutputSchema seam + bounded repair loop),
    // then assert on the returned Value / Err substring and the provider call count.
}
```

The pure Table A exercises the validator seam directly (the CPU hot path benched below).
Table B proves the loop semantics — critically the **bad-then-good** replay via
`ScriptedProvider`'s ordered-then-clamped `complete`, so the one-shot repair is a
deterministic, provider-free test.

**Harness obligations** (the implementing PR must land all of these, green under `nix
flake check`):

- **Seam + registry:** new `trait OutputSchema` (`validate(&schema, &value) -> Verdict`)
  in `agent-core`; impl in a sibling crate behind a cargo feature (e.g.
  `agent-validate`); one factory line in
  [`agent-runtime/src/registry.rs`](../../crates/agent-runtime/src/registry.rs)
  (`register_builtins`), config-selected (`[structured] validator = "…"`); doc in
  `docs/components/`.
- **Proto + gRPC:** add `ResponseFormat` to `message CompletionRequest` in
  [`common.proto`](../../crates/agent-proto/proto/agent/v1/common.proto), and a new
  `Validator` service (`crates/agent-proto/proto/agent/v1/validator.proto`,
  `Validate(schema, value) -> Verdict{ ok, errors[] }`) + `build.rs` entry +
  server/client in `agent-grpc` + `--serve-validator` + reflection; commit the
  `buf.image.binpb` bump via `nix run .#buf-image`; endpoint constants in
  `nix/constants.nix` → `nix run .#gen-constants`; extend
  [`agent-grpc/tests/roundtrip.rs`](../../crates/agent-grpc/tests/roundtrip.rs).
- **Metrics + OTel:** metric families in `agent-metrics` for validation-pass /
  repair-triggered / repair-exhausted counts + validation-latency histogram; a metered
  decorator in [`agent-runtime/src/metered.rs`](../../crates/agent-runtime/src/metered.rs);
  a `structured.validate` span (attrs: schema name, ok, error-count) and a
  `structured.repair` span (attr: attempt) per the #44 span-attribute pattern.
- **Bench (real CPU hot path):** an iai-callgrind bench of `OutputSchema::validate` over a
  representative nested schema (deterministic instruction count), with an Ir ceiling in
  `nix/checks/bench.nix`. The repair loop is provider-bound and is validated by the tests,
  not benched.
- **Leak:** a dhat `tests/leak.rs` (`dhat-heap` feature) asserting the validate path frees
  everything it allocates across iterations and stays under an allocation budget.

## References

- **agent-seddon:** [`crates/agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs)
  (`trait LlmProvider` ≈ 179, `struct CompletionRequest` ≈ 130, `struct ToolSchema` ≈ 215
  / `Tool::schema`), [`crates/agent-proto/proto/agent/v1/common.proto`](../../crates/agent-proto/proto/agent/v1/common.proto)
  (`message CompletionRequest` line 107),
  [`crates/agent-testkit/src/lib.rs`](../../crates/agent-testkit/src/lib.rs)
  (`ScriptedProvider` ≈ 64, `tool_turn` / `final_turn`, `RecordingMemory`, `StaticContext`),
  [`crates/agent-core/Cargo.toml`](../../crates/agent-core/Cargo.toml) /
  [`crates/agent-tools/Cargo.toml`](../../crates/agent-tools/Cargo.toml) (`serde_json`),
  [`agent-runtime/src/registry.rs`](../../crates/agent-runtime/src/registry.rs),
  [`agent-runtime/src/metered.rs`](../../crates/agent-runtime/src/metered.rs).
- **hermes-agent:** `agent/plugin_llm.py` (`complete_structured`,
  `_build_structured_messages`, `_parse_structured_text`, `_strip_code_fences`),
  `tests/agent/test_plugin_llm.py` (`test_json_mode_adds_system_directive`,
  `test_schema_name_appended_to_header`, `test_schema_validation_accepts_match`,
  `test_schema_validation_rejects_mismatch`,
  `test_complete_structured_validates_against_schema`,
  `test_complete_structured_returns_text_on_unparseable_response`,
  `test_complete_structured_emits_response_format_extra_body`).
- **opencode:** `packages/core/src/tool/tool.ts` (`definition`/`settle`,
  `inputSchema`/`outputSchema`, `Schema.decodeUnknownEffect` / `encodeEffect`),
  `packages/core/test/session-runner-tool-registry.test.ts`,
  `packages/core/test/tool-output-store.test.ts`.
- **pi:** `packages/ai/src/utils/validation.ts` (`validateToolArguments`, `Compile`,
  `Value.Check` / `Value.Errors`, `formatValidationPath`),
  `packages/ai/src/api/openai-completions.ts` (`tool_choice`),
  `packages/ai/test/validation.test.ts`,
  `packages/ai/test/openai-completions-tool-choice.test.ts`,
  `packages/ai/test/mistral-tool-schema.test.ts`.
