# Design: tool-call verification (a measured, multi-model gate)

Status: **design / pre-implementation.** This document is the input to the
implementation phase. It states requirements, the intended behaviour, what was
deliberately ruled out, and a high-level implementation sketch. It does not commit
to line-level details.

## Motivation

Now that we run capable models locally (mistral-small:24b on the l2 MI50, plus
smaller/faster ones like llama3.1:8b), a real question opens up: **before executing
a tool call the model requested, can a second opinion catch a mistake — and is that
worth the latency?**

The motivating observations are concrete, from live runs against local models:

- mistral-small:24b, mid-task, called `bash` with **no `command` argument** — a
  *schema* error the loop surfaced only when the tool failed.
- llama3.1:8b wrote a **syntactically broken regex** (`re.findall(r\b\w+\b'`) and
  could not recover — a *semantic/quality* error.

These are two different failure classes, and conflating them is the first trap this
design avoids. The bigger idea the feature enables — and the reason it is worth more
than a one-off check — is **measurement**: send verifications to several models,
record every verdict and its outcome, and *learn over time* which model is worth
trusting for which kind of check. Small specialised models may turn out to be
excellent at narrow verification tasks; we should find that out from data, not
assume it.

## Goals

1. A **swappable gate** that can inspect a requested tool call (with goal + history
   context) before it executes, and Allow / Deny / **ask the model to Revise** it.
2. Support **multiple verifiers** (an ensemble) whose verdicts are combined, with
   the verifier set and each verifier's model+config selectable by config.
3. **Record every verification** — model, config, verdict, latency, and one or more
   *outcome* proxies — to ClickHouse, so the value of each verifier (per task type)
   can be analysed offline and turned into trust weights.
4. **Cache** verdicts so a repeated identical check costs nothing.
5. Be a first-class agent-seddon **seam**: metered, gRPC-serviceable, feature-gated,
   config-selected — off by default.

## Non-goals (and what replaces them)

- **Not a safety mechanism.** Dangerous-command / sensitive-path / secret screening
  is already the job of the `Policy` guard and the `Scanner` seam, deterministically
  and for free. The verifier is about *correctness*, not *safety*, and must not be
  sold as the latter. It composes *after* those gates.
- **Not a schema validator.** Mechanical correctness (does the tool call match the
  tool's JSON schema — the mistral `bash`-without-`command` class) should be a
  **deterministic pre-dispatch check**, not an LLM call. It is faster, free, and
  reliable. See "Cheap win" below; it is a prerequisite, not part of the verifier.
- **Not automatic trust in a weaker judge.** "A small model verifies a big model"
  is *one configuration to measure*, not a built-in assumption. See "The direction
  problem".

## Two kinds of verification — the core framing

| Kind | Example | Who handles it |
|---|---|---|
| **Mechanical** | `bash` called with no `command`; args don't match the tool schema; path escapes the workspace | **Deterministic** — schema validation pre-dispatch + existing `confine()`/`Policy` guard/`Scanner`. No LLM. |
| **Semantic** | Wrong tool for the goal; a hallucinated path; code that won't run; an off-plan action | **The verifier** — an LLM (or ensemble) judging the call against the goal + history. |

The verifier owns only the semantic column. The mechanical column is cheaper and
better served by code, and some of it already exists.

### The direction problem (honest constraint)

"Verification is easier than generation" is true for *format*, false for *judgment*.
A **weaker** model judging a **stronger** model's semantic intent tends to add noise
(false denials on plans it doesn't grasp), not signal. So:

- The verifier's model is **configurable**, not fixed to "the small one".
- A weaker verifier's verdict should bias toward a **soft** signal (a `Revise` hint
  or a low-confidence flag that can trigger a retry), **not** a hard `Deny`.
- Whether any given (verifier model → generator model) pairing helps is **an
  empirical question the recording layer answers** — it is not assumed here.

## How it works

### The hook

Tool calls are dispatched in `agent-runtime/src/agent.rs` (today ~L711–756):
`Policy::authorize` runs per call (sequentially), then a `pre_tool` hook may narrow
the decision, then allowed calls execute (concurrently when parallel-safe). The
verifier runs **at this site, after Policy+hook**, where `self` has the goal, the
message history, and the tool schemas in scope — context the narrow
`Policy::authorize(&ToolCall)` trait deliberately does not receive. This is why the
verifier is a **new seam with a context-carrying call**, not another `Policy` impl.

### The seam

```rust
// agent-core
pub struct VerifyCtx<'a> {
    pub call: &'a ToolCall,
    pub goal: &'a str,
    pub history: &'a [Message],        // recent, budgeted
    pub tool_schema: Option<&'a ToolSchema>,
}

pub enum Verdict {
    Allow,
    Revise(String),        // feed this hint back to the model; it retries the call
    Deny(String),          // block outright (rare; safety stays with Policy/Scanner)
}

pub struct VerifierReport {
    pub verdict: Verdict,
    pub confidence: f32,   // 0.0..=1.0, clamped (hostile values are untrusted)
    pub model: String,     // for the audit/analysis record
}

#[async_trait]
pub trait Verifier: Send + Sync {
    async fn verify(&self, ctx: &VerifyCtx<'_>) -> VerifierReport;
}
```

`Revise` is the important addition over `Policy`'s binary `Allow|Deny`: a failed
semantic check most usefully *feeds a correction back to the model*, which retries,
rather than hard-failing the turn. `Deny` stays reserved and rare — safety is not
this seam's job.

### The ensemble (composite)

"Send to multiple LLMs and combine" is the existing **composite / multi-backend**
pattern in this codebase — the `Router` (multi-provider, in-order/round-robin +
circuit breaker) and multi-backend search (`tantivy + vector`, RRF fusion). The
verifier reuses it: an `EnsembleVerifier` fans out to N `Verifier` backends
concurrently and reduces their `VerifierReport`s to one verdict.

- **Combine rule** starts simple and configurable: e.g. any `Deny` with confidence
  ≥ threshold denies; else a confidence-weighted vote across `Allow`/`Revise`; a
  `Revise` majority surfaces the highest-confidence hint. The **weights** come from
  the trust table (below); until there is data, weights are uniform.
- Each backend may be local (in-process, hitting a provider) or `= "grpc"` (a
  remote verifier service), exactly like every other seam — this is the "quick
  gRPC" the design wants, and it is free from the seam machinery.
- Fan-out is **concurrent** and **bounded by a timeout**; a slow/broken verifier
  contributes nothing rather than stalling the loop (fail-open, like the `Scanner`).

### Caching

A verdict is small and deterministic in its inputs, and the check sits on the hot
path. The cache is therefore **in-process, bounded, TTL'd** — the exact shape of the
existing `agent-web-search` `ResultCache` (`Mutex<HashMap>` + `ttl_ms` +
`max_entries`), or `moka` for proper concurrent LRU/TinyLFU + TTL if we want to
upgrade the pattern once.

- **Key** = hash of `(tool_name, canonicalised_args, goal/context fingerprint,
  verifier_model + config fingerprint)`. The **model+config must be in the key** — a
  verdict from mistral is not valid for llama; changing the verifier invalidates old
  verdicts. TTL is a backstop.
- A hit **skips the verifier entirely** (no LLM call). This is the cheapest and most
  unambiguously good part of the feature.
- The cache is **not** the ClickHouse record — see the next section. One is for
  speed (ephemeral, evicts); the other is for learning (permanent, append-only).

### Recording + the trust table (the measurement platform)

Every verification writes a row to a **new `agent_verifications` ClickHouse table**,
modelled on the existing `agent_events`/`agent_usage` MergeTree tables and written
through the same native-protocol bounded-channel sink (drops rather than blocks).
Proposed columns:

```sql
CREATE TABLE IF NOT EXISTS agent.agent_verifications
(
    session_id     String,
    ts             DateTime64(3, 'UTC'),
    iter           UInt32,
    tool_name      String,
    args_hash      String,
    goal_hash      String,
    task_type      String,          -- classification of the call/goal (see below)
    verifier_model String,
    verifier_cfg   String,          -- JSON of the verifier's config fingerprint
    verdict        String,          -- allow | revise | deny
    confidence     Float32,
    latency_ms     UInt32,
    cached         UInt8,
    -- outcome proxies, filled in as they become known (see "ground truth"):
    call_errored     Nullable(UInt8),   -- did the executed tool return is_error?
    revised_after    Nullable(UInt8),   -- did the agent revise this target soon after?
    task_succeeded   Nullable(UInt8)    -- did the run reach a good final state?
)
ENGINE = MergeTree
ORDER BY (session_id, ts, iter);
```

An **offline** component reads this table and produces per-`(verifier_model,
task_type)` **trust weights** consumed on the next run. It is deliberately *not* in
the hot path: weighting is auditable and recomputed periodically, and the loop just
loads a small weights blob. This is what lets the system "observe over time what is
worthwhile and what is not" — and, crucially, what surfaces "this small model is
unexpectedly good at *this* task type".

## The crux: what is the ground truth?

**This is the design decision everything else hangs on, and it has no clean
answer.** To score how "successful" a verifier was, we need to know whether the tool
call was *actually* correct. There is no free label; there are only proxies, each
imperfect:

- **`call_errored`** — did the executed tool return `is_error`? Cheap and immediate,
  but a call can succeed and still be wrong.
- **`revised_after`** — did the agent edit/redo the same target within N steps? A
  genuine "that was probably bad" signal, and locally attributable.
- **`task_succeeded`** — did the run reach a good final state (compiles / tests pass
  / an e2e-style check)? The truest signal, but delayed and sparse, and it suffers
  the **credit-assignment** problem: when a multi-step task fails, *which*
  verification caused it is not cleanly recoverable.

**Decision: record several proxies and let the data reveal which one correlates with
real quality.** Do not hardcode "success = didn't error" — that trains the trust
weights on noise. The recording layer is built to carry multiple, deferred-fill
outcome columns for exactly this reason. Human labelling stays available as an
optional high-quality signal but is not required to start.

## What we ruled out (and when we'd revisit)

- **External cache daemons — nginx / memcached / NATS-KV.** All add a network hop to
  avoid an LLM call, plus an operational daemon, to store a few MB of tiny verdicts.
  An in-process map hit is ~100ns vs ~0.5ms+ for a network cache. Ruled out for the
  hot cache. **Future:** if verdicts ever need to be **shared across hosts** (l ↔
  l2), that is not "add memcached" — it is a **second impl of the cache behind a
  seam** (local `moka`; optional `= "grpc"` backed by Redis/NATS-KV), so the shared
  store stays off the hot path and swappable. Deferred until cross-host sharing is
  *shown* to be needed.
- **nginx specifically** — it caches HTTP *responses*; we cache verdict objects
  in-process. Wrong layer entirely.
- **Weaker-verifies-stronger as a built-in default** — kept as a *configurable,
  measured* option, not an assumption (see "The direction problem").
- **Hard `Deny` as the default verifier action** — semantic verification prefers
  `Revise` (feed a hint, let the model retry); `Deny` is rare and safety stays with
  `Policy`/`Scanner`.
- **An LLM for mechanical/schema checks** — deterministic validation instead;
  faster, free, reliable.

## Phasing (each phase earns the next)

1. **Cheap win + recording pipeline.** (a) Deterministic **pre-dispatch schema
   validation** of tool-call args (catches the `bash`-without-`command` class with
   zero LLM calls — valuable on its own). (b) The `Verifier` seam + hook + cache +
   the `agent_verifications` table, wired with a **trivial verifier** and the outcome
   proxies. Ships the data pipeline and the plumbing without betting on the LLM idea.
2. **Analyse.** With rows accumulating: does *any* verifier's verdict correlate with
   a quality proxy? Which models are good at which `task_type`? This is the go/no-go
   for the sophisticated part — decided from data, per this project's ethos.
3. **Ensemble + trust-weighting**, built **only if** phase-2 data justifies it —
   the composite verifier and the offline weight computation consuming the history.

## Fit with agent-seddon (adding the seam)

Follows the standard seam recipe (`docs/extending.md`): trait in `agent-core` → impl
crate behind a cargo feature → factory line in `register_builtins`
(`agent-runtime/src/registry.rs`) → config selection (`config/agent.toml`) →
optional `Grpc<Verifier>` client + `VerifierService` server + a generated port →
metered decorator (`metered.rs`) → a component doc. Off by default (`[verifier]
backend = ""` ⇒ no gate, no cost). Prometheus families for verdict counts, verifier
latency, cache hit-rate, and per-verdict outcome; ClickHouse for the historical
analysis. Testing per the house rules — table-driven `#[rstest]` with the four case
classes, and **`adversarial_` cases are mandatory**: the verifier consumes
model-produced text (goal, args, hints) and provider-produced numbers (confidence),
all attacker-controlled — clamp confidences, cap history/hint sizes, treat a
verifier's own output as untrusted.

## Wire & plumbing (remote verifier only)

**None of this is needed for the local, in-process verifier — only for
`[verifier] backend = "grpc"` (the cross-host ensemble case). Phase 1 needs no
proto and no gRPC.** When a remote verifier is wanted, the additions are small
because the context types already exist:

- **Proto** — a new `agent/v1/verifier.proto`, modelled on `policy.proto`
  (`service Policy { rpc Authorize(ToolCall) returns (Decision); }`). It **reuses
  `common.proto`** — `ToolCall`, `Message`, `ToolSchema` all already exist:
  ```proto
  message VerifyRequest { ToolCall call = 1; string goal = 2;
                          repeated Message history = 3; optional ToolSchema tool_schema = 4; }
  enum VerifyVerdict { VERIFY_VERDICT_UNSPECIFIED = 0; ALLOW = 1; REVISE = 2; DENY = 3; }
  message VerifierReport { VerifyVerdict verdict = 1; optional string hint = 2;
                           float confidence = 3; string model = 4; }
  service Verifier { rpc Verify(VerifyRequest) returns (VerifierReport); }
  ```
  The `= 0` value is the decode-rule fallback: per the codebase convention
  (unknown enum → do the least), `VERIFY_VERDICT_UNSPECIFIED` decodes to *no
  opinion → allow-through*, which is the verifier's fail-open stance (a broken
  verifier must not block the loop). Plus `convert.rs` core↔proto and one
  generated port in `nix/constants.nix`.
- **buf** — purely additive (new service + messages + enum), so `buf breaking`
  passes against the committed baseline untouched; no baseline bump. `buf lint`
  applies to the new file; optionally record the additions with
  `nix run .#buf-image`.
- **gRPC** — the standard per-seam machinery: `client/verifier.rs`
  (`GrpcVerifier`), `server/verifier.rs` (`VerifierService`), a `SEAMS` row →
  `--serve-verifier`; health + reflection come free. **`Verify` is read-only (no
  side effects), so it is retryable** on the standard `agent-retry` unary path
  with no special-casing — unlike the non-idempotent ops
  (`checkpoint`/`append`/`exec`/`pty.write`). An unreachable remote verifier fails
  open (contributes nothing), like the remote `Scanner`.
- **Cache-control headers — none.** The verdict cache is in-process and
  content-keyed, not an HTTP cache. The **`Vary` equivalent is the cache key** (the
  verifier model+config fingerprint is *in* the key, so a verdict is valid only for
  the model that produced it); the **`max-age` equivalent is a local config TTL**,
  not a server header. If a remote verifier were ever to *suggest* cacheability it
  would be a **clamped field in `VerifierReport`** (attacker-controlled, like the
  capped backoff hints), never a header, and the client stays free to ignore it.

## Open questions for the implementation phase

- **`task_type` classification** — what taxonomy, and computed how (tool name +
  cheap heuristic? a classifier? deferred to phase 2)? It gates the per-task
  analysis, so it needs at least a coarse first cut in phase 1.
- **Which outcome proxy leads** — start by recording all three; the analysis picks.
- **Selective verification** — verifying *every* call multiplies latency×N. Do we
  gate on tool kind (only side-effecting calls?), on model self-reported confidence,
  or verify asynchronously and only block on side-effecting tools? Likely a config
  knob; default to side-effecting-only.
- **Weights bootstrapping** — uniform until enough data; what is "enough", and how
  are weights versioned/loaded per run.
- **Cache: reuse `ResultCache` vs adopt `moka`** — a one-time pattern decision.
