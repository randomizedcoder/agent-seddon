# structured output — the `OutputSchema` seam

Constrain a completion to a known shape: attach a JSON Schema to a request, ask
the provider to honour it, **validate** the model's JSON against the schema, and
**repair once** on mismatch. Turns free-text parsing at the call site into a
schema contract — useful for validated subagent returns, classifiers, and
machine-consumable agent output. See parity spec
[`16-structured-output.md`](../parity/16-structured-output.md).

- **Validator seam:** `agent_core::OutputSchema` ([`agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs)) —
  `validate(&schema, &value) -> Verdict { ok, errors }`. Pure + synchronous (a CPU
  check), so it benches directly and any impl shares the `Verdict` contract.
- **Request contract:** `CompletionRequest.response_format: Option<ResponseFormat>`
  (`{ schema, strict, name }`) + a `ModelCapabilities.supports_response_format`
  flag. `None` ⇒ today's free-text behaviour, unchanged (every existing provider
  keeps working).
- **Impl crate:** [`agent-validate`](../../crates/agent-validate).
- **Shipped validator:** `draft07` (`validate-draft07`) — a dependency-free
  validator for the draft-07 **subset** that matters: `type` (incl. arrays +
  nested), `required`, `properties`, `additionalProperties: false`, `enum`,
  `items`, and basic numeric/string/array bounds. Errors name the offending JSON
  path (`/outer/inner`), like pi's `formatValidationPath`. Ships no external
  jsonschema crate, so the default build stays hermetic.
- **Repair loop:** `agent_runtime::structured::complete_structured` — reached via
  `Agent::complete_structured(request, schema, max_repairs)`. It attaches the
  schema as `response_format`, steers natively when the provider supports it and
  otherwise **injects the schema into the prompt**, then per attempt fence-strips,
  parses, and validates the output; on mismatch it feeds the validation error back
  and re-prompts, bounded by `max_repairs` (config default 1), before a hard error.
- **Runtime feature:** `structured` (default). **Config:** `[structured] validator
  = "draft07"`, `max_repairs = 1`.

## What agent-seddon exceeds

- **hermes** validates but **does not repair**; **opencode**/**pi** validate *tool
  I/O*, not a general completion, and don't repair. agent-seddon adds the bounded
  **model-in-the-loop one-shot repair** — the piece no peer has.
- Error surfacing distinguishes *unparseable* (not JSON), *schema-mismatch* (valid
  JSON, wrong shape — path named), and *repair-exhausted* (still invalid after the
  bounded retries).

## Observability

- **Metrics** (`agent-metrics`): `agent_structured_total{outcome}` counter
  (`pass` / `repaired` / `exhausted`) recorded per completion, and
  `agent_structured_validate_seconds` latency per validation.
- **Tracing:** a `structured.validate` span (`ok` / `errors` attrs) per validation
  (via the `MeteredValidator` decorator) and a `structured.repair` span (`attempt`
  attr) per repair.

## Tests, bench, leak

- **Validator:** a pure `#[rstest]` table over schema × value (type / required /
  additionalProperties / enum / nested / arrays / numeric+string bounds).
- **Repair loop:** a table driving a `ScriptedProvider` (`[bad, good]` replay via
  its ordered-then-clamped `complete`) — valid-first-try, repair-then-pass,
  repair-exhausted, unparseable-then-repaired, fenced-JSON-stripped,
  additionalProps-then-repaired, no-repair-budget — asserting the returned value
  and the provider round-trip count, plus a metrics-outcome test.
- **Bench:** `agent-validate/benches/validate.rs` — the validator over a nested
  schema (deterministic Ir ceiling). The repair loop is provider-bound → not benched.
- **Leak:** `agent-validate/tests/leak.rs` asserts the validate path frees its
  path-strings + error vec across iterations.

## Not distributed over gRPC, deliberately

``OutputSchema``'s primary operation is a **synchronous, pure, local function** — a CPU-bound JSON-schema check.
A gRPC client cannot implement a sync trait method (there is nowhere to await),
and making the trait `async` to allow it would add an `async_trait` heap
allocation to every call while buying nothing: there is no I/O to overlap, no
credential to isolate, and no hardware or shared state worth a network hop.

The full reasoning — and what to measure if the decision is ever revisited — is
in [`../grpc.md`](../grpc.md#three-seams-are-deliberately-not-distributed).

## Deferred (staged like the tokenizer / web / tasks seams)

- **Native provider `response_format` serialization** — providers advertise
  `supports_response_format = false` today, so the helper prompt-injects the
  schema; wiring the OpenAI `json_schema` response_format (and flipping the flag)
  is a follow-up. Validation gates the result regardless.
- **The `Validator` gRPC service** (`--serve-validator`) + the `ResponseFormat`
  proto field on `CompletionRequest` (it defaults `None` on the wire today).
