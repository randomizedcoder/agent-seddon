# Parity spec 42 — pre/post-compact hooks + remote-compaction fallback

Per-feature parity spec for **user-definable pre/post-compact hooks** and a
**remote-compaction fallback**: two lifecycle points that fire *around* a
compaction (before the live window is mutated, and after) so an operator can
inject behaviour — persist a summary, snapshot state, veto/defer — plus a path
that delegates the *summarization itself* to a service and **falls back to local
compaction on failure**. This extends the `Hook` seam (spec 22) and the
`ContextStrategy` seam (spec 09), and reuses the real `Tokenizer` counts (spec 23)
that already drive the compaction budget.

> **Status: ⬜ spec written, not started.** Proposed extension of the typed
> **`Hook` seam** ([spec 22](22-hooks.md)) with two new lifecycle points —
> `pre_compact(&WorkingSet, &CompactionPlan)` (runs *before* the window is
> mutated, may **observe / steer / defer**) and `post_compact(&CompactionInfo)`
> (a richer successor to today's coarse `on_compact` observer) — plus a
> **remote-compaction fallback** in [`ContextStrategy`](09-context-compaction.md):
> a `RemoteCompactor` the strategy delegates summarization to, dialed as a
> `= "grpc"` seam, that **falls back to the local `SummarizingWindow`/
> `SlidingWindow` path on any error, timeout, or hostile response**. New config
> under `[context]`: `compact_hooks = true`, `remote_compactor = "grpc" | "off"`,
> `remote_compact_deadline_ms`. The invariant is load-bearing: **a hook can
> observe or steer, but the spec-09 compaction invariants must still hold** — the
> window ends under budget (or at the 2-message floor), the tail never begins with
> an orphan `tool` result, and the volatile recent tail is never lost. Cross-refs
> [spec 09](09-context-compaction.md) (the compaction seam + its invariants),
> [spec 22](22-hooks.md) (the typed Hook seam this extends),
> [spec 23](23-tokenizer-cost.md) (the real token counts the budget uses).
> **Deferred:** streaming the pre/post-compact events over the `HookService.Subscribe`
> bus (spec 22's deferred tail); a *distributed* multi-node compaction quorum; and
> cost-aware routing of the remote compactor (spec 25) — a compactor is billed
> like any other completion, but choosing *which* remote to summarize on is out of
> scope here. Unimplemented; the §7 plan is the design of record.

## Feature & why it matters

Compaction is the one place in the loop that **destructively rewrites the live
window**: `ContextStrategy::compact` drops or summarizes the middle of the
conversation to fit the token budget (parity [09](09-context-compaction.md)). It
is exactly the moment an operator most wants a seam, for two independent reasons:

- **Behaviour *around* the rewrite.** Before the window is mutated, a user may
  want to **persist the about-to-be-summarized turns** to durable memory,
  **snapshot** the session (a checkpoint the summary can't reconstruct), or
  **veto/defer** this cycle (e.g. "we're mid-edit, don't compact yet"). After the
  rewrite, a user may want to **record what was dropped**, notify a dashboard, or
  re-derive a title. Today none of this is reachable without editing the loop.
- **Delegating the summarization.** Local summarization spends a full LLM
  completion on the agent's own provider, on the critical path, every time the
  budget is crossed. A dedicated **compaction service** (a cheaper/faster model, a
  shared summary cache, a server that already holds the transcript) can do it
  out-of-process — but only if a failure **falls back to the local path** rather
  than stalling the loop. Compaction that can't complete is worse than a lossy
  one: the provider then rejects the whole request for overflowing its window.

The subtlety, and the reason this is a *spec* and not a one-liner: a hook that can
touch compaction can also **break** it. So the design's core obligation is that
the extension points are **inspectable and bounded** — a pre-compact hook sees the
plan but the [spec-09 invariants](09-context-compaction.md) (under-budget-or-floor,
no leading orphan `tool`, volatile tail preserved) are re-checked *after* any
hook or remote path runs, and a failing hook or remote is **isolated** and falls
through to the deterministic local compaction. That is the natural, auditable
extension of a codebase that already has a typed Hook seam, budget-gated
auto-compaction, and real token counts.

## agent-seddon today

**One coarse, observe-only, post-hoc compact surface exists — nothing more.**

- **Auto-compaction runs every iteration, always local.** The loop
  ([`crates/agent-runtime/src/agent.rs`](../../crates/agent-runtime/src/agent.rs)
  ~1266–1279) measures `tokens_before`, calls
  `context.compact(working, budget, pending_switch.take())` under a
  `context.compact` span, then measures `tokens_after`. The strategy self-gates on
  the budget (`target = max_context_tokens − reserve_output`):
  [`summarizing.rs`](../../crates/agent-context/src/summarizing.rs) keeps a head +
  a `keep_recent_tokens` tail and LLM-summarizes the middle (falling back to
  truncation on summarizer error — parity 09), and
  [`sliding_window.rs`](../../crates/agent-context/src/sliding_window.rs) drops the
  oldest turns. A `ModeAwareWindow` reshape variant
  ([`mode_aware.rs`](../../crates/agent-context/src/mode_aware.rs)) exists.
  Compaction budgets on the real `Tokenizer` when wired, else `estimate_tokens`
  (parity [23](23-tokenizer-cost.md)). Manual `/compact` also exists
  (`Session::compact`). **All of this is in-process; there is no remote path.**
- **The only compact hook is a coarse observer.** The typed `Hook` seam (spec 22,
  [`crates/agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs) ~2510)
  ships five points, and exactly one touches compaction: **`on_compact(&CompactionInfo)`**,
  dispatched right *after* `context.compact` returns (`agent.rs` ~1275). It is
  (a) **post-only** — there is no point that runs *before* the window is mutated,
  so "persist these turns before they're summarized away" / "veto this cycle" is
  unreachable; (b) **observe-only** — it returns `()` and cannot steer or defer,
  unlike `pre_tool`'s `HookOutcome::Deny`; and (c) **coarse** —
  `CompactionInfo { strategy, tokens_before, tokens_after }` carries only token
  deltas, not *what* was dropped or which tail survived. It is telemetry, not a
  compaction hook in the peer sense.
- **The scaffolding to extend is all present.** The `Hook` trait +
  `HookRegistry` dispatch, the `pre_tool` **veto** path (`HookOutcome`), the
  metered-hook decorator, and the `= "grpc"` remote-hook client already exist
  (spec 22). `SummarizingWindow` already holds an `Arc<dyn LlmProvider>` and
  already has a **summarizer-error → local-truncation fallback** — the exact shape
  a *remote*-summarizer fallback mirrors. So both halves are additive: two new
  trait points on an existing seam, and one new delegate on an existing strategy.

Honest gap: there is **no pre-compact point**, the sole post-compact surface is a
telemetry-grade observer that **cannot steer or veto**, and **compaction is always
local** — there is no service-delegated summarization and therefore no
delegate→local fallback. This spec adds all three.

## Peer implementations & their tests

| Peer | Impl path | Test path | Framework |
| --- | --- | --- | --- |
| codex | `codex-rs/core/src/hook_runtime.rs` (`run_pre_compact_hooks`/`run_post_compact_hooks` → `PreCompactHookOutcome`/`PostCompactHookOutcome::{Continue,Stopped}`), `codex-rs/hooks/src/events/compact.rs` (`PreCompactRequest`/`PostCompactRequest`), `codex-rs/core/src/compact_token_budget.rs` (lifecycle wiring), `codex-rs/core/src/compact_remote.rs` + `compact_remote_v2.rs` (server-side remote compaction), `codex-rs/core/src/compact_model_fallback.rs` (`should_retry_with_current_model` → retry on a different model), `codex-rs/core/src/state/auto_compact_window.rs` (`AutoCompactWindowIds`) | `codex-rs/core/tests/suite/token_budget.rs` (`PreCompact`/`PostCompact` hooks.json + `HookEventName::PreCompact` event), `codex-rs/core/tests/suite/compact_remote.rs` (`remote_pre_turn_compaction_failure`, `…context_window_exceeded`), `codex-rs/core/tests/suite/compact.rs`, `codex-rs/core/src/compact_tests.rs` | cargo `#[test]` + insta snapshots |
| opencode | `packages/plugin/src/index.ts` (`"experimental.session.compacting"` — inject context / replace prompt; `"experimental.compaction.autocontinue"` — gate auto-continue), `packages/opencode/src/session/compaction.ts` (`plugin.trigger(...)` at both points, `select`/`buildPrompt`) | `packages/opencode/test/session/compaction.test.ts` (`experimental.session.compacting` / `experimental.compaction.autocontinue` handler cases), `packages/core/test/session-compaction.test.ts` | bun:test + Effect |
| pi | `packages/coding-agent/src/core/extensions/types.ts` (`SessionBeforeCompactEvent` — "can be cancelled or customized"; `CompactOptions`/`CompactionResult`), `packages/coding-agent/src/core/compaction/compaction.ts` (`SUMMARIZATION_PROMPT`, `generateSummary`), `packages/coding-agent/src/core/compaction/utils.ts` | `packages/coding-agent/test/compaction-extensions.test.ts` (`allow extensions to cancel compaction`, `provide custom compaction`, `continue with default compaction if extension throws error`, `call multiple extensions in order`, `before_compact and compact events`), `test/trigger-compact-extension.test.ts` | vitest |
| hermes | — for pre/post-compact *hooks* (`VALID_HOOKS` has `pre_tool_call`/`post_tool_call`/`pre_llm_call`/`post_llm_call`/`pre_verify`/session hooks — **no compact hook**); the *summarization-fallback* half exists in `trajectory_compressor.py` (per-attempt retry with `jittered_backoff`, then a basic `"[CONTEXT SUMMARY]: …failed…"` fallback) + `agent/auxiliary_client.py` (`fallback_chain`, `_call_fallback_candidate_sync`) | `tests/agent/test_compression_fallback_budget.py`, `tests/test_trajectory_compressor.py`, `tests/agent/test_context_compressor_summary_continuity.py` | pytest |

**codex is the anchor for *both* halves** — it is the only peer that ships
first-class pre/post-compact hooks *and* remote compaction with a local fallback:

- **Pre/post-compact command hooks** (`hook_runtime.rs`): `run_pre_compact_hooks`
  returns `PreCompactHookOutcome::{Continue, Stopped}` and `run_post_compact_hooks`
  returns `PostCompactHookOutcome::{Continue, Stopped}` — i.e. a pre-compact hook
  can **stop** the compaction (the veto/defer analogue), and both are dispatched as
  a real lifecycle around the rewrite. `hooks/src/events/compact.rs` defines the
  typed payloads (`PreCompactRequest`/`PostCompactRequest`: `session_id`,
  `turn_id`, `transcript_path`, `model`, `trigger`) — note the **`trigger`** field
  distinguishes `manual` vs auto compaction, matcher-selectable in `hooks.json`.
  `compact_token_budget.rs` documents the design intent verbatim: token-budget
  compaction "is still modeled as compaction **so compact hooks** and
  `ContextCompaction` turn items observe the same lifecycle as local or remote
  compaction" — one lifecycle, three implementations.
- **Remote compaction + fallback** (`compact_remote.rs`, `compact_remote_v2.rs`):
  `run_remote_compact_task_inner_impl` runs a `run_remote_compact_attempt`; on
  `Err(error)` it consults an optional `fallback_step_context` and
  `should_retry_with_current_model(&error)` (`compact_model_fallback.rs` — retry on
  `ContextWindowExceeded`/`ServerOverloaded`/`UsageLimitReached`/`RetryLimit`/…),
  re-attempts with the fallback context, records a
  `codex.compaction.model_fallback{reason,implementation,outcome}` counter, and
  **only if the fallback also fails does it return the original error** — the exact
  delegate→fallback shape this spec ports, with the metered outcome.
- **Tests** pin it end-to-end: `token_budget.rs` writes a `hooks.json` with
  `PreCompact`/`PostCompact` command handlers and asserts the `HookEventName::PreCompact`
  event fires around a manual compaction; `compact_remote.rs` has
  `remote_pre_turn_compaction_failure` and `…context_window_exceeded` snapshot cases
  driving the remote path (and its failure) via mounted SSE mocks.

**opencode** exposes the two compaction plugin points but no *fallback* seam:
`"experimental.session.compacting"` lets a plugin **inject context or replace the
compaction prompt** (`compaction.ts`: `nextPrompt = compacting.prompt ?? buildPrompt(...)`),
and `"experimental.compaction.autocontinue"` gates whether the loop auto-continues
after compacting. Its `compaction.test.ts` drives both by name-matching the trigger
in a stub plugin. There is no delegate-to-service-then-fall-back path.

**pi** has the richest *hook* semantics but, again, no remote fallback: a
`session_before_compact` event that extensions can **cancel** or **replace with a
custom compaction** (`extensions/types.ts` "can be cancelled or customized";
`CompactOptions.onComplete`). `compaction-extensions.test.ts` is the closest peer
analogue to our test plan — it pins `allow extensions to cancel compaction`,
`provide custom compaction`, and crucially **`continue with default compaction if
extension throws error`** (the error-isolation invariant), plus multi-extension
ordering. pi's summarization is local (its own model); there is no service delegate.

**hermes** has **no compact hook** in `VALID_HOOKS` (marked `—`), so the hook half
has no hermes analogue. Its `trajectory_compressor.py` supplies the *fallback*
data point only: summarization retries with `jittered_backoff`, and on exhausting
`max_retries` emits a deterministic basic summary rather than failing — plus an
`auxiliary_client` `fallback_chain` so a summarization call can escalate across
candidates (`test_compression_fallback_budget.py`). That is a summarizer fallback,
not a local-vs-remote *strategy* fallback, so it informs only the remote half.

## Completeness gaps

Behaviour agent-seddon must add to be the most complete (spec only — do **not**
implement here). Each maps to a test case below.

- **`pre_compact` hook point (new).** Add `pre_compact(&WorkingSet, &CompactionPlan)
  -> HookOutcome` to the `Hook` trait, dispatched **before** `context.compact`
  mutates the window. It sees the plan (target budget, tokens-over, which turns are
  the summarize-candidates vs the protected head/tail) so it can persist/snapshot
  the about-to-be-lost turns. Returning `HookOutcome::Deny(reason)` **defers** this
  compaction cycle (the window is left untouched; the next iteration re-evaluates),
  mirroring codex `PreCompactHookOutcome::Stopped` and pi "cancel compaction". Like
  every other point it defaults to a no-op. *(Port codex/pi; do not implement here.)*
- **`post_compact` hook point (upgrade the coarse observer).** Promote today's
  `on_compact(&CompactionInfo)` into `post_compact(&CompactionInfo)` carrying **what
  actually happened** — `{ strategy, implementation: Local|Remote|Fallback,
  tokens_before, tokens_after, dropped_turns, summary_len }` — so an observer can
  record the delta, not just the token counts. Keeps the existing observe-only
  semantics (no veto after the fact). *(Upgrade; codex `PostCompact` is the model.)*
- **Invariants re-checked after any hook/remote path (the load-bearing rule).**
  Whatever a `pre_compact` hook or a remote compactor does, the loop **re-asserts
  the [spec-09 invariants](09-context-compaction.md)** on the resulting window:
  `estimate/count ≤ target` **or** the 2-message floor, the first non-system
  message is **never** `Role::Tool` (no orphan-tool tail), and the volatile recent
  tail is preserved. A hook cannot narrow these away. *(New — agent-seddon owns the
  invariant guarantee no peer states explicitly.)*
- **Error / panic isolation.** A `pre_compact` or `post_compact` hook that panics
  or returns `Err` **must not** corrupt the working set or crash the loop: the
  dispatcher catches it (`hook_errors_total`), and — matching pi
  "continue with default compaction if extension throws" — a failing *pre*-hook
  **fails open** (compaction proceeds locally) while a failing *post*-hook is inert
  (the already-compacted, already-valid window is untouched). *(Port pi.)*
- **`RemoteCompactor` delegate + local fallback (new).** `ContextStrategy` gains an
  optional `Arc<dyn RemoteCompactor>` (a `summarize(plan) -> Result<Compacted>`
  seam) selected by `remote_compactor = "grpc"`. `compact` tries the remote first;
  on **any** `Err`, a **deadline** breach (`remote_compact_deadline_ms`, capped —
  a hostile server can't stall the loop), or a **hostile response** (oversized /
  invalid summary that would violate an invariant), it **falls back to the local
  `SummarizingWindow`/`SlidingWindow` path** — reusing the exact fallback shape
  `SummarizingWindow` already has for a failing local summarizer. *(Port codex
  `compact_remote` → `should_retry_with_current_model` → fallback.)*
- **Metered outcome + span.** A `compaction_total{implementation=local|remote|fallback,
  outcome=ok|deferred|error}` counter (codex's `model_fallback{outcome}` analogue),
  the existing `context.compact` span gains `implementation` + `deferred` attributes,
  and each hook dispatch is metered like every other Hook point (spec 22). *(New.)*
- **Config, no code edits.** `[context] compact_hooks = true`,
  `remote_compactor = "grpc" | "off"`, `[context.remote] endpoint = …`,
  `remote_compact_deadline_ms = …` — registry-selected exactly like every other
  seam impl. *(New.)*

## Table-driven test plan

Two homes. Hook lifecycle cases extend the spec-22 loop test
(`crates/agent-runtime/tests/compact_hooks.rs`, modelled on `hooks.rs`); the
remote-fallback cases live next to the strategy in
[`crates/agent-context/src/summarizing.rs`](../../crates/agent-context/src/summarizing.rs).
Doubles (from [`agent-testkit`](../../crates/agent-testkit/src/lib.rs)): the
spec-22 **`RecordingHook`** gains `pre_compact`/`post_compact` recording +
`deny_on_compact()` / `err_on_compact()` / `panic_on_compact()`; a **new
`ScriptedRemoteCompactor`** (`Ok(summary)` / `Err` / `hang(past_deadline)` /
`hostile(oversized)`); the spec-09 `long(role, n)` / `msg(role, content)` helpers
and `FailingProvider` double are reused. A `TestClock` makes the deadline case
deterministic (no wall-clock `sleep`). Every case re-asserts the **spec-09
invariants** (`≤ target` or 2-floor; first non-system ≠ `Tool`; volatile tail
preserved). Prefixes: `positive_` succeeds, `negative_` reject/error/isolation,
`corner_` odd-but-valid, `boundary_` at a limit. `(port: <peer>)` marks a case
mined from a peer test; `(new: agent-seddon)` marks ours.

```rust
// crates/agent-runtime/tests/compact_hooks.rs — pre/post-compact hook lifecycle
// Reuses the spec-22 loop harness (ScriptedProvider, EchoTool, RecordingMemory,
// StaticContext/SummarizingWindow) + RecordingHook extended with compact points.
#[rstest]
// pre_compact fires BEFORE the window is mutated, post_compact AFTER; a plain
// observer sees both and the compacted window still satisfies every spec-09
// invariant (nothing the observer did changed the result).
#[case::positive_pre_and_post_compact_fire_in_order(
    HookScript::Observe,
    Want::Lifecycle(vec!["pre_compact", "compact", "post_compact"]))] // (port: codex PreCompact/PostCompact; pi before_compact/compact)
// pre_compact may OBSERVE (snapshot/persist the summarize-candidates) but the
// compaction invariants still hold: under target OR 2-floor, first non-system
// != Tool, volatile recent tail preserved verbatim.
#[case::positive_pre_compact_observes_but_invariants_hold(
    HookScript::SnapshotOnly,
    Want::InvariantsHold)] // (port: codex PreCompactRequest / pi custom compaction)
// pre_compact returns Deny -> this compaction cycle is DEFERRED: the window is
// left untouched (no drop/summarize), loop re-evaluates next iteration.
#[case::corner_pre_compact_deny_defers_compaction(
    HookScript::DenyCompact,
    Want::WindowUnchanged)] // (port: codex PreCompactHookOutcome::Stopped / pi cancel)
// a pre_compact hook that panics/Err is ISOLATED and FAILS OPEN: local
// compaction still runs to completion and the window is valid.
#[case::negative_pre_compact_failure_isolated_still_compacts(
    HookScript::PanicPre,
    Want::InvariantsHold)] // (port: pi "continue with default compaction if extension throws")
// a post_compact hook that panics/Err MUST NOT corrupt the already-compacted
// working set: it is inert, the window is byte-identical to the pre-hook result,
// the loop continues.
#[case::negative_post_compact_failure_does_not_corrupt_working_set(
    HookScript::PanicPost,
    Want::WorkingSetIntact)] // (port: pi throws; new: agent-seddon post-corruption guard)
#[tokio::test(flavor = "multi_thread")]
async fn compact_hook_cases(#[case] script: HookScript, #[case] want: Want<'_>) {
    // Build Agent(SummarizingWindow, RecordingHook(script), small TokenBudget that
    // forces one compaction); run("go"); assert on hook.events() / the working set.
    // EVERY case additionally re-asserts the spec-09 invariants on the final window.
}
```

```rust
// crates/agent-context/src/summarizing.rs — remote-compaction fallback
// Doubles: ScriptedRemoteCompactor + the spec-09 FailingProvider/long()/msg().
#[rstest]
// remote succeeds: window compacted from the remote summary; invariants hold,
// implementation recorded = "remote".
#[case::positive_remote_compaction_succeeds(Remote::Ok, Impl::Remote)]                 // (port: codex compact_remote happy path)
// THE marquee case: remote returns Err -> fall back to the LOCAL summarize/slide
// path; ends under target, head preserved, no orphan-tool tail, tail preserved,
// implementation = "fallback".
#[case::boundary_remote_failure_falls_back_to_local(Remote::Err, Impl::Fallback)]     // (port: codex should_retry_with_current_model -> fallback; hermes summarizer fallback)
// remote hangs past remote_compact_deadline_ms (capped) -> deadline breach ->
// local fallback. Determinism: TestClock advances past the cap, never wall-clock.
#[case::negative_remote_timeout_capped_falls_back(Remote::Hang, Impl::Fallback)]      // (new: agent-seddon; hostile-server cap, cf. security rules)
// remote returns a hostile/oversized summary that would violate an invariant ->
// rejected -> local fallback; the volatile tail is NEVER lost to a bad remote.
#[case::corner_remote_hostile_summary_rejected_falls_back(Remote::Hostile, Impl::Fallback)] // (new: agent-seddon)
// remote_compactor = "off": local path only, remote never dialed (no leak).
#[case::negative_remote_disabled_local_only(Remote::Disabled, Impl::Local)]           // (new: agent-seddon)
#[tokio::test]
async fn remote_compact_fallback_cases(#[case] remote: Remote, #[case] expect: Impl) {
    // Build WorkingSet (head + large turns) + small TokenBudget forcing compaction;
    // inject ScriptedRemoteCompactor(remote). Assert: compact() returns Ok in EVERY
    // case (never propagates a remote error), the recorded implementation == expect,
    // and the spec-09 invariants hold (estimate/count <= target OR len==2; first
    // non-system != Role::Tool; recent tail present unchanged).
}
```

gRPC roundtrip (extend
[`crates/agent-grpc/tests/roundtrip.rs`](../../crates/agent-grpc/tests/roundtrip.rs)):
serve the `RemoteCompactor` as a `= "grpc"` seam (TCP + UDS), drive one compaction
whose summary comes over the wire, then a second where the served compactor returns
`Status::unavailable` and assert the client **falls back to local** — the seam is
identical in-process vs. served, and the fallback survives the transport (the
pattern every other seam's roundtrip test uses).

Case-prefix key: `positive_` expected success, `negative_` expected
error/isolation/guard, `corner_` odd-but-valid, `boundary_` at a limit.
`(port: <peer>)` names the peer a case was mined from (codex is the anchor for both
halves; pi for error-isolation + cancel; hermes for the summarizer fallback);
`(new: agent-seddon)` marks the deadline-cap, hostile-summary, post-corruption, and
metered-implementation assertions that have no peer analogue.

## Harness obligations

The implementing PR (one feature, green under `nix flake check`) must ship:

- **Seam extension + registry.** Two new points on the `Hook` trait
  (`pre_compact` returning `HookOutcome`, `post_compact` taking the richer
  `CompactionInfo`) in [`agent-core`](../../crates/agent-core/src/lib.rs); the
  `on_compact` call site in [`agent.rs`](../../crates/agent-runtime/src/agent.rs)
  (~1266–1279) split into a `pre_compact` dispatch **before** `context.compact` and
  a `post_compact` dispatch after; a `RemoteCompactor` trait + a local-fallback
  wrapper in `ContextStrategy`; one factory line in
  [`register_builtins`](../../crates/agent-runtime/src/registry.rs) (incl.
  `remote_compactor = "grpc"` → remote client); config in `config/agent.toml`. Doc
  in [`docs/components/context.md`](../components/context.md) +
  [`docs/components/hooks.md`](../components/hooks.md).
- **Proto + gRPC.** A `Compact`/`Summarize` RPC on a `CompactorService` (mirroring
  the existing seam services), reflection, `--serve-compactor`; extend
  `roundtrip.rs`; bump the buf baseline via `nix run .#buf-image`; add the endpoint
  to `nix/constants.nix` → `nix run .#gen-constants`. (The pre/post-compact events
  ride the deferred `HookService.Subscribe` bus — not this PR.)
- **Metrics + OTel.** `compaction_total{implementation,outcome}` counter (the codex
  `model_fallback{outcome}` analogue) and `hook_errors_total` reuse from spec 22;
  the `context.compact` span gains `implementation` + `deferred` attributes; each
  compact-hook dispatch is metered via the existing `metered.rs` hook decorator.
- **Bench: SKIP (documented).** Both new surfaces are IO/callback-bound (a hook
  body or a network round-trip; the pure CPU cost sits in the already-benched
  `estimate_tokens`/tokenizer count and the spec-09 drop/summarize loop) — document
  the iai skip in `nix/checks/bench.nix` alongside the other IO-bound skips, per the
  per-spec contract.
- **Leak.** A dhat `tests/leak.rs` case (`dhat-heap` feature) over the
  **delegate→fallback** path — a remote error that falls back to local must free
  the discarded remote attempt (summary buffer, request) and stay under budget; the
  post-compact `CompactionInfo` clone (with `dropped_turns`) is the allocation-heavy
  part worth pinning.

## References

- **agent-seddon:**
  [`crates/agent-runtime/src/agent.rs`](../../crates/agent-runtime/src/agent.rs)
  (`run_loop` compaction ~1266–1279: `tokens_before` → `context.compact(...)` →
  `on_compact(CompactionInfo)` under the `context.compact` span — the split point),
  [`crates/agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs)
  (`Hook` trait ~2510, `HookOutcome`, `CompactionInfo`, `ContextStrategy`,
  `WorkingSet`, `TokenBudget`),
  [`crates/agent-context/src/summarizing.rs`](../../crates/agent-context/src/summarizing.rs)
  (head+tail retention + existing summarizer-error → local-truncation fallback — the
  shape the remote fallback mirrors),
  [`crates/agent-context/src/sliding_window.rs`](../../crates/agent-context/src/sliding_window.rs)
  (drop-oldest + the spec-09 invariants),
  [`crates/agent-context/src/mode_aware.rs`](../../crates/agent-context/src/mode_aware.rs)
  (the reshape variant),
  [`crates/agent-runtime/src/registry.rs`](../../crates/agent-runtime/src/registry.rs)
  (`register_builtins`),
  [`crates/agent-runtime/src/metered.rs`](../../crates/agent-runtime/src/metered.rs)
  (metered-hook decorator),
  [`crates/agent-grpc/tests/roundtrip.rs`](../../crates/agent-grpc/tests/roundtrip.rs),
  [`crates/agent-testkit/src/lib.rs`](../../crates/agent-testkit/src/lib.rs)
  (`RecordingHook`, `ScriptedProvider`, `FailingProvider`, add `ScriptedRemoteCompactor`);
  related specs [`09-context-compaction.md`](09-context-compaction.md) (compaction
  seam + invariants), [`22-hooks.md`](22-hooks.md) (the Hook seam extended here),
  [`23-tokenizer-cost.md`](23-tokenizer-cost.md) (the real counts the budget uses).
- **codex (anchor — both halves):**
  `codex-rs/core/src/hook_runtime.rs` (`run_pre_compact_hooks`/`run_post_compact_hooks`,
  `PreCompactHookOutcome`/`PostCompactHookOutcome::{Continue,Stopped}`),
  `codex-rs/hooks/src/events/compact.rs` (`PreCompactRequest`/`PostCompactRequest`:
  `transcript_path`/`model`/`trigger`),
  `codex-rs/core/src/compact_token_budget.rs` ("modeled as compaction so compact
  hooks … observe the same lifecycle as local or remote compaction"),
  `codex-rs/core/src/compact_remote.rs` + `compact_remote_v2.rs`
  (`run_remote_compact_task_inner_impl` → `fallback_step_context` on `Err`),
  `codex-rs/core/src/compact_model_fallback.rs`
  (`should_retry_with_current_model`, `record_model_fallback{reason,implementation,outcome}`),
  `codex-rs/core/src/state/auto_compact_window.rs` (`AutoCompactWindowIds`);
  tests `codex-rs/core/tests/suite/token_budget.rs` (`PreCompact`/`PostCompact`
  `hooks.json`, `HookEventName::PreCompact`), `codex-rs/core/tests/suite/compact_remote.rs`
  (`remote_pre_turn_compaction_failure`, `…context_window_exceeded`),
  `codex-rs/core/tests/suite/compact.rs`, `codex-rs/core/src/compact_tests.rs`.
- **opencode:** `packages/plugin/src/index.ts`
  (`"experimental.session.compacting"`, `"experimental.compaction.autocontinue"`),
  `packages/opencode/src/session/compaction.ts` (`plugin.trigger` at both points);
  tests `packages/opencode/test/session/compaction.test.ts`,
  `packages/core/test/session-compaction.test.ts`.
- **pi:** `packages/coding-agent/src/core/extensions/types.ts`
  (`SessionBeforeCompactEvent` — cancel/customize; `CompactOptions`/`CompactionResult`),
  `packages/coding-agent/src/core/compaction/compaction.ts` (`generateSummary`);
  tests `packages/coding-agent/test/compaction-extensions.test.ts`
  (`allow extensions to cancel compaction`, `provide custom compaction`,
  `continue with default compaction if extension throws error`),
  `test/trigger-compact-extension.test.ts`.
- **hermes:** — no compact hook (`hermes_cli/plugins.py` `VALID_HOOKS`);
  summarizer-fallback only in `trajectory_compressor.py` (retry + basic-summary
  fallback) and `agent/auxiliary_client.py` (`fallback_chain`); tests
  `tests/agent/test_compression_fallback_budget.py`,
  `tests/test_trajectory_compressor.py`,
  `tests/agent/test_context_compressor_summary_continuity.py`.
