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

## Status: shadow mode (increment 1)

Today the verifier runs in **shadow**: it evaluates each *allowed* tool call and
its verdict is recorded on a `verifier` span, but it does **not** change the
loop's behaviour — a `Revise`/`Deny` does not yet block or rewrite the call. This
is the measurement-first first step: prove the verifier produces sane verdicts
before letting it enforce. Only calls the `Policy` already allowed are shadowed
(a policy-denied call is never verified).

Follow-ups, in order: enforcement (revise/deny affecting the loop), an LLM-backed
verifier, an ensemble with per-model trust-weighting, and ClickHouse recording of
verdicts + outcomes. gRPC serviceability is additive and remote-only (see the
design doc).

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
