# Verifier — a correctness gate on tool calls

A second opinion on a tool call the model requested, checked **before it runs**.
Selected by `[verifier] backend`; **off by default**. Full design and phasing:
[`../design/tool-call-verification.md`](../design/tool-call-verification.md).

**It is about correctness, not safety.** Dangerous-command / sensitive-path /
secret screening stays with the [`Policy`](policy.md) guard and the
[`Scanner`](scanner.md). The verifier answers a different question — *is this the
right call?* — and, unlike `Policy::authorize` (which sees only the bare
`ToolCall`), it sees the **goal and recent history**, because judging relevance
needs them.

## The seam

```rust
pub struct VerifyCtx<'a> {
    pub call: &'a ToolCall,
    pub goal: &'a str,
    pub history: &'a [Message],
    pub tool_schema: Option<&'a ToolSchema>,
}

pub enum VerifyVerdict {
    Allow,            // no objection
    Revise(String),   // probably wrong — feed this hint back so the model retries
    Deny(String),     // block outright (rare; safety stays with Policy/Scanner)
}

#[async_trait]
pub trait Verifier: Send + Sync {
    fn name(&self) -> &str;
    async fn verify(&self, ctx: &VerifyCtx<'_>) -> VerifierReport; // fails OPEN
}
```

`Revise` is the preferred non-allow outcome: a mistaken call most usefully feeds a
correction back to the model, which retries, rather than hard-failing the turn. A
verifier **fails open** like the `Scanner` — one that cannot form an opinion
returns `Allow` rather than blocking the loop.

## Modes: shadow and enforce

`[verifier] mode` gates whether a non-allow verdict changes behaviour:

- **`shadow`** (default) — the verifier evaluates each *allowed* call and its
  verdict is recorded (a `verifier` span + the `agent_verifier_verdicts_total`
  metric), but behaviour is unchanged: a `Revise`/`Deny` does not block or rewrite
  the call. Turning a verifier on stays safe. This is the measurement-first
  default — prove the verifier produces sane verdicts before letting it enforce.
- **`enforce`** — a `Revise`/`Deny` **blocks** the call (it never runs) and its
  message is fed back to the model as the tool result, so the model can reissue a
  corrected call. `Revise`'s hint is phrased as guidance; `Deny` as a block.

Only calls the `Policy` already allowed reach the verifier (a policy-denied call
is never verified). Every verdict increments `agent_verifier_verdicts_total`
(labels: `verifier`, `verdict`, `mode`).

## Recording (the measurement platform)

Every verified call also emits one **`verification` record** — routed by the
telemetry sink to the `agent_verifications` ClickHouse table (alongside
`agent_events`/`agent_usage`), through the same bounded-channel writer that drops
rather than blocks. It carries the verdict, the verifier model + config
fingerprint, `confidence` (clamped), `latency_ms`, and *outcome proxies* filled as
they become known:

- **`call_errored`** — did the executed tool return `is_error`? Known immediately;
  `NULL` for a call the verifier blocked (which never ran).
- **`revised_after`**, **`task_succeeded`** — deferred to a later increment; `NULL`
  for now (the columns exist so the analysis can pick which proxy correlates with
  real quality — see the design doc's "ground truth" section).

Args and the goal are **hashed** (`fnv1a_hex`), not stored raw, keeping
model-produced (possibly sensitive) text out of the analytics table. `task_type`
is a coarse phase-1 placeholder (currently the tool name). Recording is
best-effort telemetry: a dropped row never affects the loop. With telemetry off it
still lands in the episodic JSONL, which is how the e2e tests assert it.

Follow-ups, in order: an LLM-backed verifier; an ensemble with per-model
trust-weighting plus the offline weight computation over this table; and filling
the deferred outcome proxies. gRPC serviceability is additive and remote-only (see
the design doc).

## Backends

| `backend` | What it does |
|---|---|
| *(empty)* | Off — no verifier, no cost. The default. |
| `schema` | Deterministic, model-free. Validates the call's arguments against the tool's JSON Schema (required fields present, top-level types match) and asks the model to `Revise` on a mismatch — e.g. a required argument missing, the class a live model produced when it called `bash` with no `command`. Certain (confidence 1.0), and only ever `Allow`/`Revise`. |

The check is deliberately **shallow** (required-field presence + a top-level type
check) — bounded work regardless of how large or deeply nested the model's
arguments are, which matters because those arguments are attacker-controlled.

## Extending

Add a backend the usual way ([`../extending.md`](../extending.md)): implement
`agent_core::Verifier` in `agent-verifier` behind a cargo feature, register a
factory in `register_builtins`, select it with `[verifier] backend`. A verifier's
self-reported `confidence` is untrusted (clamp with `VerifierReport::clamped_confidence`
before it reaches a total or a weight), and `adversarial_` tests are mandatory —
the arguments, hints and history it consumes are all model-produced.
