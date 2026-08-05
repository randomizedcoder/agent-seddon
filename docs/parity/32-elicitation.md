# Parity spec 32 — elicitation / request_user_input

Per-feature parity spec for an **`Elicitation` seam** and a model-invocable
`request_user_input` tool: a way for the *model* to pause mid-turn, pose one or
more **schema-typed questions** to the human operator, block until an answer is
supplied, **validate** that answer against the schema it declared, and resume with
the answer folded into the tool result — instead of guessing, or dumping a
free-text "please tell me X" into its final message and ending the turn.

> **Status: ⬜ spec written, not started.** Proposes a new **`Elicitation` seam**
> (async trait in `agent-core`: `elicit(request) -> Answers`) with pluggable
> backends selected by config like every other seam — a **`stdin`** backend (TTY
> prompt, the in-process default), a **`grpc`** client (dial a remote elicitor,
> the mirror of every other served seam), and a **`denied`** null backend that
> fails closed on every request — impl in a new **`agent-elicit`** crate behind an
> `elicit` cargo feature, wired by one factory line in `register_builtins` and
> selected by the **`elicitation`** config key (`config/agent.toml`). The model
> reaches it through a `request_user_input` tool (in `agent-tools`) whose args are
> the untrusted question set. **Differentiator:** none of the four peers exposes a
> *distributed, reflection-introspectable* ask-user seam whose request/answer pair
> crosses gRPC (`--serve-elicit`, dialable like any other seam), is **metered**
> (pending-elicitation gauge + outcome counter), carries a **per-request OTel
> span**, and is gated deterministically under `nix flake check` (iai bench on the
> schema-validation hot path, dhat leak over the request→answer path) — an
> ask-user path exactly as inspectable, and as remotable, as every other
> agent-seddon seam. **Unimplemented** — unlike the fundamentals (specs 01–10),
> the `Elicitation` trait, its backends, the `request_user_input` tool, the proto
> service, and the CLI/serve wiring do not exist yet; this is the design of record.
> **Deferred:** a portal/GUI elicitor backend (the agent-portal `AgentSessionService`
> is the natural transport — a `Send`-style push of the question to a Flutter form,
> answer routed back through the actor-per-session channel); a `secret` field type
> that never logs/spans the typed value (codex's `isSecret`); and multi-user
> answer routing (which operator answers a served elicitation under multi-session,
> spec set #150) — all out of scope for the first increment.

## Feature & why it matters

agent-seddon's turn loop is **one-directional**: the model calls tools, reads
`Observation`s, and eventually emits a final assistant message. When it hits a
genuine fork — an ambiguous instruction, a destructive choice, a missing
credential, "which of these three refactors do you want" — it has exactly two bad
options today:

- **Guess and keep going.** It picks a branch, acts on it, and may do the wrong
  destructive thing (delete the wrong file, pick the wrong migration) before the
  human ever sees the choice.
- **End the turn with a plain-text question.** It writes "Do you want A or B?" as
  its final message and stops. The human answers in the *next* turn, but all the
  mid-turn context (the half-applied plan, the tool state) is gone or must be
  re-derived, and the "answer" is unstructured prose the model has to re-parse.

What's missing is a **structured, blocking, mid-turn ask**: the model declares a
question *with a schema* (an enumerated choice set, or a free-form string), the
turn **pauses**, the operator answers, the answer is **validated against that
schema**, and it comes back as a tool result so the model continues in the *same*
turn with the decision made. This is the difference between an agent that stalls
on ambiguity and one that resolves it in-line.

The unit of work is a **request**, not a message: one or more questions, each with
an id (so answers map back), a prompt string, and an answer schema (enumerated
options ± a free-form "other", or a typed scalar). Because the question text and
the answer schema are **authored by the model** — which is prompt-injectable — the
seam is **untrusted on both sides**: the prompt shown to a human must not be a
vector for social-engineering or terminal escapes, the schema must be bounded
before it drives a UI, and the human's typed answer must be validated against the
declared schema before it re-enters the model's context. And when **no interactive
channel exists** (a batch/CI run, a served seam with no attached operator), the
seam must **fail closed** with a distinct, machine-readable error — never hang,
never silently auto-answer.

## agent-seddon today

**No ask-user tool exists anywhere.** There is no `elicit`, `request_user_input`,
`ask_user`, or `prompt_user` in the tree — the model has no way to pose a
structured question and block for a typed answer.

- **The only user-interaction surface is approval gating, not questioning.**
  [`crates/agent-runtime/src/policy.rs`](../../crates/agent-runtime/src/policy.rs)
  has an `Interactive` policy (`struct Interactive`, ~line 31) and a `Guard` policy
  with `GuardMode::Prompt` (~line 116) that, **when a tool call the policy flagged**
  is about to run, prompt the operator on stdin for a `y/N` and map the answer to
  `Decision::Allow` / `Decision::Deny` (`decide_from_answer`, ~line 22). This is a
  **binary approve/deny on a call the runtime chose to gate** — the operator answers
  a yes/no about an action, they are never asked a *question the model composed*, and
  there is no schema, no free-form answer, no multi-question request. It is the wrong
  shape for elicitation (and it belongs to the `Policy` seam, parity
  [`08-permissions-policy.md`](08-permissions-policy.md), not a new one).
- **It already fails closed off a TTY — the right instinct, wrong seam.** The
  `Interactive`/`Guard` prompt reads stdin on a blocking thread
  (`std::io::stdin().read_line`, ~line 46) and, crucially, **denies without reading
  a byte when stdin is not a TTY** (~line 300: "If stdin is **not** a TTY we deny
  without reading a byte: there is no operator"). That non-interactive fail-closed
  rule is exactly the discipline the `Elicitation` seam needs — but it produces a
  `Decision::Deny`, not a validated *answer*, so it cannot be reused directly; it is
  precedent, not implementation.
- **The tool surface is request/response with no pause.**
  [`crates/agent-tools/src/lib.rs`](../../crates/agent-tools/src/lib.rs) tools take
  args, do work, return an `Observation` — none of them block for out-of-band human
  input. There is no channel from a running tool back to the operator except stdout.
- **The served-seam + metered + traced scaffolding is all reusable.** Every seam
  can run as its own gRPC service with reflection and a `--serve-<seam>` flag; the
  metered-decorator pattern lives in
  [`metered.rs`](../../crates/agent-runtime/src/metered.rs), the gauge/counter
  primitives in [`agent-metrics`](../../crates/agent-metrics/src/lib.rs), and the
  per-op span pattern in [`agent-telemetry`](../../crates/agent-telemetry/). An
  `Elicitation` service is a *unary* request/answer RPC — simpler wiring than spec
  29's streaming PTY.

Honest gap: no seam lets the model pose a **structured question (schema-typed)** and
receive a **validated answer**. The `Elicitation` trait, its `stdin`/`grpc`/`denied`
backends, the `request_user_input` tool, the answer-validation layer, the proto
service, and the CLI/serve wiring **do not exist yet**.

## Peer implementations & their tests

| Peer | Impl path | Test path | Framework |
| --- | --- | --- | --- |
| codex | `codex-rs/core/src/tools/handlers/request_user_input_spec.rs` (tool schema + `normalize_request_user_input_tool_args` + per-mode availability), `codex-rs/core/src/tools/handlers/request_user_input.rs` (handler: subagent reject, blocking-by-mode), `codex-rs/core/src/elicitation.rs` (`ElicitationService`: ref-counted pause of tool-result delivery), `codex-rs/protocol/src/request_user_input.rs` (`RequestUserInputQuestion`/`…Args`/`…Answer`, `isOther`/`isSecret`/`isBlocking`) | `codex-rs/core/src/tools/handlers/request_user_input_spec_tests.rs`, `.../request_user_input_tests.rs`, `codex-rs/protocol/src/request_user_input_tests.rs` | cargo `#[test]` / insta |
| opencode | `packages/core/src/tool/question.ts` (model-facing `question` tool: `questions[]`, per-question `options`, `multiple`, auto "Type your own answer"), `packages/core/src/question.ts` (`QuestionV2` service: ask lifecycle, publish events, settle/reject a pending reply) | `packages/core/test/question.test.ts`, `packages/core/test/tool-question.test.ts` | bun:test + Effect |
| pi | — (no own ask-user tool and **no MCP elicitation client**; `AskUserQuestion` appears only as a Claude-Code tool **name** in a static passthrough lookup — `packages/ai/src/api/anthropic-messages.ts` ~L86, `claudeCodeTools`) | — | vitest |
| hermes | `tools/mcp_tool.py` (`ElicitationHandler`: `elicitation/create` callback, MCP SDK ≥1.11.0, flat-object schema summary, form-mode → approval, url-mode declined), `tools/approval.py` (`request_elicitation_consent` → CLI/TUI/gateway/Telegram/Slack surface) — **client side** of MCP elicitation, not a model-invocable tool | `tests/tools/test_mcp_elicitation.py` (`test_mcp_elicitation_for_hermes_tools_auto_accepts`, `test_mcp_elicitation_for_other_servers_declines`) | pytest |

**codex is the deep anchor** — a first-class, model-invocable `request_user_input`
tool with a real answer schema, and it pins exactly the behaviours we need:

- **Schema-typed question set** (`request_user_input_spec.rs`
  `create_request_user_input_tool`): the tool arg is `questions: Vec<RequestUserInputQuestion>`,
  each with a snake_case `id` ("Stable identifier for mapping answers"), a short
  `header` ("12 or fewer chars"), a single-sentence `question`, and an `options`
  array of `{ label, description }` choices ("Provide 2-3 mutually exclusive
  choices … the client will add a free-form 'Other' option automatically"). Both the
  question object and each option are `additionalProperties: false` — a **closed
  schema**, so a hostile extra field is rejected at parse time.
- **Server-side normalization + validation** (`normalize_request_user_input_tool_args`,
  ~line 105): rejects a request where **any** question has empty/missing options
  (`"request_user_input requires non-empty options for every question"`) and forces
  `is_other = true` on every question (the free-form escape hatch is always present).
  Test: `normalize_request_user_input_tool_args_rejects_missing_options`,
  `…_sets_other_on_every_question`.
- **Availability gating by mode** (`request_user_input_unavailable_message`,
  `request_user_input_tool_description`): the tool is only exposed in certain
  `ModeKind`s; asked in a disallowed mode it returns
  `"request_user_input is unavailable in {mode} mode"` rather than blocking. Tests:
  `request_user_input_unavailable_messages_respect_default_mode_feature_flag`,
  `…_tool_description_mentions_available_modes`.
- **Reject sub-agent callers** (`request_user_input.rs` handler, ~line 58:
  `if turn.session_source.is_non_root_agent()`): a **non-root/sub-agent thread may
  not elicit** the human — only the top-level session can. Test:
  `multi_agent_v2_request_user_input_rejects_subagent_threads`. (Directly relevant to
  agent-seddon's multi-session/sub-agent story.)
- **Blocking vs non-blocking is a policy, not the model's whim** (`RequestUserInputArgs.is_blocking`;
  handler sets it from the turn's mode): `is_blocking` is derived from context
  (blocking in plan mode, non-blocking elsewhere), with a **legacy default of
  `true`** when the field is absent (`protocol/src/request_user_input_tests.rs`
  `request_user_input_event_defaults_legacy_missing_is_blocking_to_true`). Tests:
  `request_user_input_sets_blocking_from_turn_mode`,
  `request_user_input_sets_non_blocking_outside_plan_mode`.
- **Pause coordination** (`elicitation.rs` `ElicitationService`): a **ref-counted**
  pause — `register()` bumps `outstanding`, the first registration flips a
  `watch::<bool>` to `paused=true`, `wait_until_clear()` awaits the drop back to
  zero, and a `Drop` guard decrements — so several concurrent elicitations keep the
  session paused until *all* are answered. This is the "block the turn until the
  human answers" mechanism, done leak-safely.
- **Secret answers** (`protocol/src/request_user_input.rs`
  `RequestUserInputQuestion.is_secret`): a question can be flagged `isSecret` so the
  UI masks input and the value is handled carefully — the seed for agent-seddon's
  deferred `secret` field.

**opencode** ships the same idea transport-free: a model-facing `question` tool
(`tool/question.ts`) whose input is `questions[]` (each a `QuestionV2.Prompt` with
`options`, `multiple`, and a default `custom` "Type your own answer" option) and
whose output is answers projected back to the model as
`User has answered your questions: "…"="…"` (`toModelOutput`). The `QuestionV2`
service (`question.ts`) owns the **ask lifecycle**: it publishes `question.v2.*`
lifecycle events, **settles a pending reply**, **rejects unknown ids**, and
**isolates pending requests per location-layer instance** (rejecting them on
finalization). Its tests are the richest behavioural spec of the four:
`"publishes lifecycle events and settles a pending reply"`,
`"publishes rejection, fails the ask, and rejects unknown IDs"`,
`"isolates pending requests by location-layer instance and rejects them on
finalization"`, and (tool-level)
`"omits a denied built-in question and terminally settles a stale call"`,
`"registers question and projects user answers without a permission assertion"`,
`"keeps dismissed questions out of model-facing output"`. Notably opencode's MCP
client **does not** expose MCP-level elicitation — the capability is commented out
(`packages/opencode/src/mcp/index.ts` ~line 44: `// elicitation: {},`) — so its
ask-user path is the native `question` tool, not MCP.

**hermes** covers the *other* direction: it is an MCP **client** that handles
`elicitation/create` requests from downstream MCP **servers** (`ElicitationHandler`
in `tools/mcp_tool.py`, gated on MCP SDK ≥ 1.11.0). A server asking Hermes to
collect structured user input is routed through Hermes' **existing approval
surface** (`request_elicitation_consent` in `tools/approval.py` → CLI/TUI/gateway/
Telegram/Slack); form-mode elicitations are collected, url-mode ones are declined
as unsupported, Hermes' own tool-server auto-accepts while other servers are
declined (tests `test_mcp_elicitation_for_hermes_tools_auto_accepts`,
`test_mcp_elicitation_for_other_servers_declines`). This is a useful second data
point on **routing an elicitation to whatever channel owns the operator** and on
**declining unsupported request shapes** — but it is the model→server→client path,
not a model-invocable tool, so it informs the routing/fail-closed design rather
than the tool schema.

**pi** has neither: no own ask-user tool and no MCP elicitation client. `AskUserQuestion`
appears only as a **string** in pi's Claude-Code tool-name lookup table
(`packages/ai/src/api/anthropic-messages.ts` ~L86) used to canonicalize casing when
proxying Claude's built-in tools — pi recognizes the *name* but implements nothing.
Marked "—". This is a feature where **codex is the deep anchor**, **opencode a
strong second** (lifecycle + per-instance isolation), **hermes a routing/
fail-closed data point**, and agent-seddon can leapfrog all three on distribution
(unary gRPC + reflection, `--serve-elicit`) and observability (metered pending
count + outcome counter + per-request span), plus a validated-answer layer none of
them expose as a swappable seam.

## Completeness gaps

Behaviour agent-seddon must add to be the most complete (spec only — do **not**
implement here). Each maps to a test case below.

- **`Elicitation` seam.** New async trait in `agent-core`:
  `elicit(request: ElicitRequest) -> Result<Answers, ElicitError>`, where
  `ElicitRequest` is a bounded set of `Question { id, header, prompt, schema }` and
  `schema` is either `Choice { options: Vec<Option>, allow_other: bool }` or a typed
  `FreeForm { kind: Text | Integer | Bool }`. Impl in a new **`agent-elicit`** crate
  behind an `elicit` cargo feature; one factory line in
  [`register_builtins`](../../crates/agent-runtime/src/registry.rs); config-selected
  by the `elicitation` key. (Port codex's `RequestUserInputQuestion` shape.)
- **`request_user_input` tool.** A model-invocable tool in `agent-tools` whose args
  are the untrusted `questions[]`, which calls the configured `Elicitation` backend
  and returns the validated answers as an `Observation`. A **closed** arg schema
  (`additionalProperties: false`, per codex) so a hostile extra field is rejected at
  parse time. (Port codex `create_request_user_input_tool` / opencode `question`
  tool.)
- **Backends, config-selected.** `stdin` (prompt the operator on a TTY, the
  in-process default), `grpc` (dial a remote elicitor — the served-seam mirror),
  and `denied` (a null backend that fails every request closed, for headless/CI).
  Chosen by config string in the registry exactly like `SearchBackend`. (New —
  no peer offers a swappable elicitor backend.)
- **Fail-closed on no interactive channel (mandatory).** The `stdin` backend, like
  today's `Interactive` policy, **detects a non-TTY stdin and returns a distinct,
  typed `ElicitError::NoInteractiveChannel`** *without* reading a byte and *without*
  blocking — the tool surfaces it to the model as a clear "no operator available"
  observation, never a hang and never a fabricated answer. (Port codex's
  unavailable-message discipline + reuse agent-seddon's own non-TTY deny rule.)
- **Answer validation against the declared schema (mandatory, untrusted).** The
  operator's typed answer is validated against the question's schema before it
  re-enters the model's context: a `Choice` answer must be one of the offered option
  ids (or the free-form "other" when `allow_other`), a `FreeForm { Integer }` must
  parse as an integer, etc. An out-of-range / unparseable answer is **re-prompted**
  (bounded retries) or rejected — never passed through unvalidated. (New — codex/
  opencode validate the *request* shape; agent-seddon additionally validates the
  *answer*.)
- **Request-shape normalization + caps (untrusted).** Reject a `Choice` question
  with zero options (codex's rule); **cap** the number of questions per request, the
  option count per question, and the byte length of every model-authored string
  (prompt/header/label) *before* any of it is rendered to a human — a hostile,
  oversized schema must be truncated/rejected, not forwarded. (Port codex
  `normalize_…`, extend with DoS caps per the security model.)
- **Prompt sanitization before display (untrusted, mandatory).** The model authors
  the text a human will read; that text must be neutralized before it hits a
  terminal — strip/escape ANSI/control sequences (no cursor-hijack, no fake prompt
  spoofing the shell) and clearly frame it as *model-authored* ("The agent asks:")
  so an injected instruction can't masquerade as the tool/operator's own words.
  (New — no peer sanitizes the displayed prompt; this is agent-seddon's untrusted
  posture.)
- **Sub-agent / non-root callers cannot elicit.** Only the top-level session may
  block the operator; a sub-agent's `request_user_input` returns a typed refusal,
  not a pause (codex `is_non_root_agent`). Cross-references the multi-session /
  sub-agent model (spec set #150). (Port codex.)
- **Metered pending-elicitations + outcome + per-request span (differentiator).** An
  `elicit_pending` gauge (inc on request, dec on answer/deny/timeout), an
  `elicit_requests_total{outcome=answered|denied|no_channel|invalid|timeout}`
  counter, and a per-request `elicit.request` OTel span (attrs `question_count`,
  `schema_kind`, `outcome`, `duration_ms`, `retries`; **never** the secret/answer
  text) reusing [`agent-metrics`](../../crates/agent-metrics/src/lib.rs) +
  [`agent-telemetry`](../../crates/agent-telemetry/). (New — no peer analogue.)
- **gRPC service.** `elicit.proto` with a unary `Elicit(ElicitRequest) returns
  (ElicitResponse)` RPC, reflection, `--serve-elicit`; a remote elicitor is dialable
  like any other seam (the client is the `grpc` backend above). (New — no peer
  distributes elicitation over a reflectable RPC.)

## Table-driven test plan

New `#[rstest]` tables in the `agent-elicit` crate (request normalization + answer
validation + backend selection), plus a `request_user_input` tool table in
`agent-tools` and a gRPC roundtrip case. **This seam is UNTRUSTED on both sides** —
the model controls the question text *and* the answer schema, and (for `grpc`) the
served request is attacker-shaped — so `adversarial_` cases are **mandatory** and
each must assert the rejection/neutralization, alongside the four standard prefixes.

Doubles from [`agent-testkit`](../../crates/agent-testkit/src/lib.rs): a **new**
`ScriptedElicitor` (an `Elicitation` double returning a pre-programmed answer per
question id, so a "human answers B" flow is deterministic with no TTY), a
`DeniedElicitor` (always `NoInteractiveChannel`, modelling headless/CI), and the
existing `tempdir()`. A `TestClock` (injected `now()`) makes the timeout case
deterministic — advance the clock by hand, never wall-clock `sleep`. Prefixes:
`positive_` succeeds, `negative_` rejects, `corner_` odd-but-valid, `boundary_`
edge, `adversarial_` a hostile/injection input that must be rejected or
neutralized. `(port: <peer>)` marks a case mined from a peer test;
`(new: agent-seddon)` are ours.

```rust
// ---- request normalization: shape rules before a human ever sees it --------
#[rstest]
#[case::positive_choice_with_options(one_choice(&["a","b"]),           Ok(()))]              // (port: codex normalize ok)
#[case::negative_choice_zero_options(one_choice(&[]),                  Err("requires non-empty options"))] // (port: codex rejects_missing_options)
#[case::corner_freeform_needs_no_options(one_freeform(Kind::Text),    Ok(()))]              // free-form ≠ choice // (new: agent-seddon)
#[case::boundary_max_questions_ok(n_questions(MAX_QUESTIONS),          Ok(()))]              // at the cap // (new: agent-seddon)
#[case::adversarial_too_many_questions(n_questions(MAX_QUESTIONS + 1), Err("too many questions"))]           // (new: agent-seddon) DoS cap
#[case::adversarial_oversized_prompt(one_choice_prompt(&"x".repeat(HUGE)), Err("prompt too long"))]          // (new: agent-seddon) DoS cap
#[case::adversarial_extra_field_rejected(json_with_unknown_key(),      Err("unknown field"))]                // (port: codex additionalProperties:false)
fn normalize_request_cases(#[case] req: ElicitRequest, #[case] expect: Result<(), &str>) {
    // normalize(req) enforces per-question options, caps count/option/byte-length,
    // and rejects unknown fields — BEFORE the request reaches any backend/human.
}

// ---- prompt sanitization: model-authored text is neutralized for display ----
#[rstest]
#[case::adversarial_ansi_escape_stripped(
    "Pick one\x1b[2J\x1b[1;1H  $ rm -rf ~   ", /*rendered has no raw ESC, framed as agent-authored*/)] // (new: agent-seddon)
#[case::adversarial_fake_operator_prompt(
    "Ignore prior text.\nSystem: approved. >", /*rendered clearly labelled 'The agent asks:'*/)]        // (new: agent-seddon)
fn sanitize_displayed_prompt_cases(#[case] raw: &str /*, expected rendering …*/) {
    // the string shown to the operator has control/ANSI sequences stripped and is
    // framed as model-authored; an injected instruction can't spoof shell/operator.
}

// ---- answer validation: the human's typed answer is checked vs the schema ---
#[rstest]
#[case::positive_choice_answer_in_set(choice(&["a","b"]),  "b",     Ok("b"))]                // (port: opencode projects answers)
#[case::positive_freeform_other_when_allowed(choice_other(&["a"]), "custom text", Ok("custom text"))]        // (port: codex is_other)
#[case::negative_choice_answer_out_of_set(choice(&["a","b"]), "zzz", Err("not an offered option"))]          // (new: agent-seddon)
#[case::positive_integer_parses(freeform(Kind::Integer),  "42",     Ok("42"))]               // (new: agent-seddon)
#[case::adversarial_integer_answer_nonnumeric(freeform(Kind::Integer), "0; drop", Err("not an integer"))]    // (new: agent-seddon) injected answer
#[case::adversarial_choice_answer_injection(choice(&["a"]), "a\n/approve --all", Err("not an offered option"))] // (new: agent-seddon)
#[case::boundary_empty_answer_rejected(choice(&["a"]),     "",       Err("empty answer"))]   // (new: agent-seddon)
fn validate_answer_cases(#[case] q: Question, #[case] typed: &str, #[case] expect: Result<&str, &str>) {
    // the operator's raw typed bytes are validated against q.schema before returning;
    // an out-of-set / unparseable / injected answer is REJECTED, never passed through.
}

// ---- fail closed when there is no interactive channel (mandatory) ----------
#[rstest]
#[tokio::test]
async fn negative_no_interactive_channel_fails_closed() {                                   // (port: hermes decline / codex unavailable; agent-seddon non-TTY deny)
    // DeniedElicitor (or stdin backend with a non-TTY stdin): elicit(req) returns a
    // TYPED ElicitError::NoInteractiveChannel WITHOUT reading a byte and WITHOUT
    // blocking; elicit_requests_total{outcome="no_channel"} += 1. Never hangs,
    // never fabricates an answer.
}

// ---- happy path via a scripted human ---------------------------------------
#[rstest]
#[tokio::test]
async fn positive_scripted_answer_roundtrip() {                                             // (port: opencode "settles a pending reply")
    // ScriptedElicitor answers question "which_refactor" -> "b". elicit() returns
    // Answers{which_refactor:"b"}; pending gauge 1->0; outcome="answered".
}

// ---- sub-agent callers cannot block the operator ---------------------------
#[rstest]
#[case::negative_subagent_cannot_elicit(Caller::SubAgent, Err("only the root session may elicit"))] // (port: codex rejects_subagent_threads)
#[case::positive_root_can_elicit(Caller::Root,           Ok("answered"))]                    // (port: codex)
#[tokio::test]
async fn caller_gate_cases(#[case] caller: Caller, #[case] expect: Result<&str, &str>) {
    // a non-root/sub-agent request_user_input returns a typed refusal, not a pause.
}

// ---- answer timeout is deterministic + fails closed ------------------------
#[rstest]
#[tokio::test]
async fn boundary_answer_timeout_denies() {                                                 // (new: agent-seddon; cf. hermes per-elicitation timeout)
    // elicit(req) with a pending answer; advance TestClock past the deadline ->
    // ElicitError::Timeout, outcome="timeout", pending gauge back to 0.
    // Determinism: injected clock only, never wall-clock sleep.
}
```

gRPC roundtrip (extend [`crates/agent-grpc/tests/roundtrip.rs`](../../crates/agent-grpc/tests/roundtrip.rs)):
`Elicit` a scripted choice question over the wire (TCP + UDS), assert the validated
answer comes back as an `ElicitResponse` — asserting the seam is identical
in-process vs. served (the pattern every other seam's roundtrip uses) — plus an
**adversarial served-request case**: a wire `ElicitRequest` with too many questions
/ an oversized prompt / an unknown field must be rejected with `InvalidArgument`
*before* it reaches the backend, proving the caps hold across the transport, not
just in-process.

Prefix legend (repo convention): `positive_` expected success, `negative_` expected
error, `corner_` odd-but-valid, `boundary_` at a limit, `adversarial_` a hostile
input that must be rejected/neutralized. `(port: <peer>)` names the peer a case was
mined from (codex the request-schema + subagent-gate + is_other anchor; opencode the
ask-lifecycle/answer-projection; hermes the decline/timeout routing);
`(new: agent-seddon)` marks the answer-validation, prompt-sanitization, DoS-cap,
metered-outcome, and non-TTY-fail-closed assertions that have no peer analogue.

## Harness obligations

The implementing PR must satisfy all (follows the #21–46 pattern; green under
`nix flake check`):

- **Seam + registry:** `Elicitation` trait in `agent-core`; impls (`stdin` /
  `grpc` / `denied`) in a new `agent-elicit` crate behind an `elicit` cargo
  feature; the `request_user_input` tool in `agent-tools`; one factory line in
  [`register_builtins`](../../crates/agent-runtime/src/registry.rs), selected by the
  `elicitation` config key; a `MeteredElicitation` in
  [`metered.rs`](../../crates/agent-runtime/src/metered.rs); doc in
  `docs/components/elicitation.md`. Reuse the non-TTY fail-closed discipline from
  [`policy.rs`](../../crates/agent-runtime/src/policy.rs) but return a **typed
  answer/error**, not a `Decision`.
- **Proto + gRPC:** `crates/agent-proto/proto/agent/v1/elicit.proto`
  (unary `Elicit(ElicitRequest) returns (ElicitResponse)`) + `build.rs` entry +
  server/client in `agent-grpc` + `--serve-elicit` + reflection; extend
  [`roundtrip.rs`](../../crates/agent-grpc/tests/roundtrip.rs) (incl. the
  adversarial served-request case); commit the `buf.image.binpb` bump
  (`nix run .#buf-image`); add the endpoint constant to `nix/constants.nix` →
  `nix run .#gen-constants`.
- **Metrics + OTel:** `elicit_pending` gauge, `elicit_requests_total{outcome}`
  counter (`answered|denied|no_channel|invalid|timeout`) in
  [`agent-metrics`](../../crates/agent-metrics/src/lib.rs); a per-request
  `elicit.request` span (attrs `question_count`, `schema_kind`, `outcome`,
  `duration_ms`, `retries` — **never** the answer/secret text) reusing
  [`agent-telemetry`](../../crates/agent-telemetry/) — the metered differentiator.
- **Bench (real hot path):** an iai-callgrind bench over the **deterministic**
  work — request normalization + schema validation + answer validation over a bounded
  question set — with an Ir ceiling in `nix/checks/bench.nix`. (The stdin/TTY read
  itself is I/O-bound with no deterministic CPU path — document that half as a skip,
  as `bash`/`pty` did; the pure validate/normalize helpers are the bench.)
- **Leak:** a dhat `tests/leak.rs` (iteration-based, `dhat-heap` feature) over the
  **request → pending-registration → answer/deny** path, asserting a request frees
  its parsed schema + pending registration on resolution (the codex `ElicitationService`
  ref-count / `Drop`-guard analogue — an outstanding-registration leak would pin the
  session paused forever) and stays under an allocation budget.

## References

- **agent-seddon:**
  [`crates/agent-runtime/src/policy.rs`](../../crates/agent-runtime/src/policy.rs) (`Interactive` ~L31 + `Guard`/`GuardMode::Prompt` ~L116 — the y/N approval gate this seam is *not*; `decide_from_answer` ~L22; non-TTY fail-closed deny ~L300 — the discipline to reuse),
  [`crates/agent-tools/src/lib.rs`](../../crates/agent-tools/src/lib.rs) (the tool surface the `request_user_input` tool joins; `confine`/DoS-cap posture),
  [`crates/agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs) (`Decision`, seam traits — add `Elicitation`),
  [`crates/agent-runtime/src/registry.rs`](../../crates/agent-runtime/src/registry.rs) (`register_builtins`),
  [`crates/agent-runtime/src/metered.rs`](../../crates/agent-runtime/src/metered.rs) (metered-seam pattern),
  [`crates/agent-metrics/src/lib.rs`](../../crates/agent-metrics/src/lib.rs) (gauges/counters to extend),
  [`crates/agent-telemetry/`](../../crates/agent-telemetry/) (per-request span),
  [`crates/agent-grpc/tests/roundtrip.rs`](../../crates/agent-grpc/tests/roundtrip.rs) (roundtrip pattern),
  [`crates/agent-testkit/src/lib.rs`](../../crates/agent-testkit/src/lib.rs) (`tempdir`, doubles — add `ScriptedElicitor` + `DeniedElicitor` + `TestClock`);
  dependencies: [`08-permissions-policy.md`](08-permissions-policy.md) (the approve/deny gate this is distinct from), multi-session spec set #150 (sub-agent caller gate + served-answer routing).
- **codex (anchor):** `codex-rs/core/src/tools/handlers/request_user_input_spec.rs` (`create_request_user_input_tool` closed schema, `normalize_request_user_input_tool_args` — non-empty options + `is_other`, `request_user_input_unavailable_message`/`…_tool_description` per-mode),
  `codex-rs/core/src/tools/handlers/request_user_input.rs` (handler: `is_non_root_agent` reject ~L58, blocking-by-mode),
  `codex-rs/core/src/elicitation.rs` (`ElicitationService` — ref-counted pause via `watch::<bool>`, `register`/`wait_until_clear`/`Drop` decrement),
  `codex-rs/protocol/src/request_user_input.rs` (`RequestUserInputQuestion` `{id,header,question,isOther,isSecret,options}`, `RequestUserInputArgs.is_blocking`, `RequestUserInputAnswer`);
  tests: `codex-rs/core/src/tools/handlers/request_user_input_spec_tests.rs` (`request_user_input_tool_includes_questions_schema`, `normalize_…_sets_other_on_every_question`, `normalize_…_rejects_missing_options`, `request_user_input_unavailable_messages_respect_default_mode_feature_flag`, `request_user_input_tool_description_mentions_available_modes`),
  `codex-rs/core/src/tools/handlers/request_user_input_tests.rs` (`multi_agent_v2_request_user_input_rejects_subagent_threads`, `request_user_input_sets_non_blocking_outside_plan_mode`, `request_user_input_sets_blocking_from_turn_mode`),
  `codex-rs/protocol/src/request_user_input_tests.rs` (`request_user_input_event_defaults_legacy_missing_is_blocking_to_true`).
- **opencode:** `packages/core/src/tool/question.ts` (model-facing `question` tool: `questions[]`, `options`, `multiple`, auto "Type your own answer", `toModelOutput`),
  `packages/core/src/question.ts` (`QuestionV2` service: ask lifecycle, publish `question.v2.*` events, settle/reject reply);
  MCP elicitation **not** exposed (`packages/opencode/src/mcp/index.ts` ~L44 `// elicitation: {},`);
  tests: `packages/core/test/question.test.ts` (`publishes lifecycle events and settles a pending reply`, `publishes rejection, fails the ask, and rejects unknown IDs`, `isolates pending requests by location-layer instance and rejects them on finalization`),
  `packages/core/test/tool-question.test.ts` (`omits a denied built-in question and terminally settles a stale call`, `registers question and projects user answers without a permission assertion`, `keeps dismissed questions out of model-facing output`).
- **pi:** — (no own ask-user tool, no MCP elicitation client; `AskUserQuestion` appears only as a Claude-Code tool name in the passthrough lookup `packages/ai/src/api/anthropic-messages.ts` ~L86 `claudeCodeTools`).
- **hermes (routing/decline data point):** `tools/mcp_tool.py` (`ElicitationHandler` — `elicitation/create` callback, MCP SDK ≥1.11.0, `_format_elicitation_schema_summary` flat-object, form-mode → approval / url-mode declined),
  `tools/approval.py` (`request_elicitation_consent` — route to CLI/TUI/gateway/Telegram/Slack surface);
  tests: `tests/tools/test_mcp_elicitation.py` (`test_mcp_elicitation_for_hermes_tools_auto_accepts`, `test_mcp_elicitation_for_other_servers_declines`).
