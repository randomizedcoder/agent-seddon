# Parity spec 09 — context assembly + compaction

Per-feature parity spec for the `ContextStrategy` seam: how the model-ready
message list is *assembled* each turn, and how the live window is *compacted* when
it grows past the token budget (sliding-window vs summarizing-window). Tracks what
agent-seddon ships today, what the peers assert, and the concrete cases needed to
be the most complete of the four.

Unlike most parity docs, this one is more **exceed** than **port**: our own
compaction is already decently tested, and — as §3 shows — the peers test
*compaction* surprisingly lightly (opencode: 2 prompt-shape assertions; pi: rich
cut-point math but no summarizer-error path). The value here is hardening our own
boundaries, not chasing a peer feature we lack.

## Feature & why it matters

Every turn, the agent loop shows the model a bounded message window. Two things
decide what it sees:

1. **Assembly** — fold the fixed user context (`context.d/` blocks) and recalled
   [memory](../components/memory.md) into the system prompt, add the goal, and
   append any trailing context as a final system message. This is the same for
   both strategies.
2. **Compaction** — when the working set's estimated tokens exceed
   `max_context_tokens − reserve_output`, shrink it. `sliding-window` drops the
   oldest turns (lossy but free); `summarizing-window` keeps the leading system
   message(s) and a recent tail (~`keep_recent_tokens`) and replaces the middle
   with one LLM-generated summary, **falling back to truncation** if the
   summarizer errors.

This matters because it is the seam between "the loop can make progress" and "the
provider rejects the request for overflowing its context window." The failure
modes are all at the *boundaries*: an empty history, a working set sitting exactly
at budget, a summarizer that errors mid-run, and the invariant that a `tool`
result must never become the first non-system message (the provider API rejects a
tool result with no preceding assistant `tool_call`). Compaction that silently
corrupts the window here is worse than no compaction, so the retention rules
(head kept verbatim, recent tail preserved, orphan tool-results folded into the
summary) are exactly what must be pinned.

## agent-seddon today

- **Trait:** [`ContextStrategy`](../../crates/agent-core/src/lib.rs) — `assemble(ContextInput) -> Vec<Message>` and `compact(&mut WorkingSet, &TokenBudget)`. `compact` is documented as **non-destructive** w.r.t. episodic memory: it only trims the live window; the durable log is never mutated.
- **Shared logic:** [`crates/agent-context/src/lib.rs`](../../crates/agent-context/src/lib.rs) — `assemble_messages` (system/user/(append) composition) and `estimate_tokens` (~4 chars/token + 8-char/message overhead). Both strategies share these; only compaction differs.
- **`SlidingWindow`:** [`crates/agent-context/src/sliding_window.rs`](../../crates/agent-context/src/sliding_window.rs) — drop the oldest non-system message until under target (never below 2 messages), then drop any leading orphan `tool` message.
- **`SummarizingWindow`:** [`crates/agent-context/src/summarizing.rs`](../../crates/agent-context/src/summarizing.rs) — keep `leading_system_count` head + a `keep_recent_tokens` tail; LLM-summarize the middle into one system message; on summarizer error, `tracing::warn!` and fall back to `drop_oldest` (the sliding-window truncation). Holds its own `Arc<dyn LlmProvider>` (the factory receives the agent's provider — see [docs/components/context.md](../components/context.md)).

Current coverage (already table-driven `#[rstest]`, ~18 cases across the crate):

- **`lib.rs` (~10 cases):** `estimate_tokens_cases` — empty (0), overhead-only, single text, two messages, unicode-uses-byte-len; plus `estimate_tokens_counts_tool_calls`. `assemble_message_count_cases` — `(n_prepend, n_recalled, n_append) → message count` (minimal→2, append→3, all→3); plus `assemble_folds_prepend_and_recalled_into_system` and `assemble_append_is_trailing_system_message`.
- **`summarizing.rs` (~8 cases):** `leading_system_count_cases` (empty/all-system/leading-then-other/none), `render_role_label_cases` (system/user/assistant/tool), `render_appends_tool_calls`, `summarizes_middle_keeps_head_and_tail` (a `FixedSummarizer` double), `no_op_when_under_budget`.
- **`sliding_window.rs` (2 cases):** `assemble_places_prepend_and_append`, `assemble_without_append_has_two_messages` — assembly only; **no compaction test at all**.

Honest gaps (all *hardening*, not missing features):

- **`SlidingWindow::compact` is untested.** The drop-oldest loop, the `len() > 2`
  floor, the exactly-at-budget no-op, and the leading-orphan-`tool` drop have zero
  direct coverage.
- **Summarizer-error fallback is untested.** `summarizing.rs` has a `FixedSummarizer`
  (always succeeds) but no *failing* provider, so the `Err(e) => drop_oldest` arm
  never runs in tests. `ScriptedProvider` in `agent-testkit` can't yet script an
  error response — a `FailingProvider` double is needed.
- **Head+tail retention correctness is asserted loosely.** The existing summarizing
  test checks `messages[1].content.contains("SUMMARY")` and `len() < 5`, but not
  that the head is preserved *verbatim*, nor exactly which tail messages survive.
- **Orphan-tool-at-tail folding is untested.** The `while cut < len && msgs[cut].role == Role::Tool` guard (don't let the tail begin with a tool result) has no case.
- **`cut <= head` → truncation-fallback path is untested** (compaction requested but nothing meaningful sits between head and tail).
- **Boundary inputs:** empty history, all-system history, a single-message window — none are exercised through `compact`.

## Peer implementations & their tests

| Peer     | Impl path | Test path | Framework |
| -------- | --------- | --------- | --------- |
| opencode | `opencode/packages/core/src/session/compaction.ts`, `.../session/projector.ts` | `opencode/packages/core/test/session-compaction.test.ts` (projector: `.../test/session-projector.test.ts`) | bun:test + Effect |
| pi       | `pi/packages/coding-agent/src/core/` (compaction/session) — see `pi/packages/coding-agent/docs/compaction.md` | `pi/packages/coding-agent/test/compaction.test.ts` (+ `agent-session-compaction.test.ts`, `agent-session-branching.test.ts`) | vitest |
| hermes   | — (no separate compaction seam surfaced) | — | — |

**Peer compaction test coverage is thin — this is the key finding.** opencode
*summarizes* (LLM-generated Markdown summary anchored to a fixed template, keeping
a `DEFAULT_KEEP_TOKENS` recent tail), which is architecturally close to our
`SummarizingWindow`; but its **test file has only 2 assertions** and neither
touches head/tail selection or budget math:

- `compaction prompt preserves detailed work state and relevant files` — asserts `buildPrompt` emits the `## Work State` / `### Completed` / `### Active` / `### Blocked` / `## Relevant Files` sections (prompt *shape* only).
- `compaction describes tool media without embedding base64` — `serializeToolContent` renders an attached PNG as `[Attached image/png: pixel.png]` and does **not** inline the base64 (a media-serialization guard, not a compaction-logic guard).

  The interesting logic — `select(entries, tokens)` walking back from the end to
  split head/recent, `compactIfNeeded` gating on `estimate(...) <= context −
  max(output, buffer)`, and the `catchTag("LLM.Error", () => false)` fallback — is
  **not directly unit-tested** in `session-compaction.test.ts`. `session-projector.test.ts`
  exists but is about DB projection of session events, not compaction.

**pi** does LLM-summary compaction plus branch-summarization for `/tree`
(per its README: `/compact [prompt]` manual + auto compaction; `/tree` navigates
the session tree with "all history preserved in a single file"; docs at
`pi/packages/coding-agent/docs/compaction.md`). Its `compaction.test.ts` is by far
the richest of the peers (**26 cases**), but focused on **token-anchor + cut-point
math**, not the summarizer round-trip or its failure:

- Context-token accounting from usage: total from usage, zero values, last non-aborted assistant usage as the anchor, skip aborted / all-zero usage, undefined when no assistant messages.
- Threshold gating: true when context exceeds threshold, false when disabled.
- Cut-point selection: find cut based on actual token differences, `startIndex` when no valid cut in range, keep everything if all fit, indicate a "split turn" when cutting at an assistant message, budget context-visible custom entries.
- Load/apply: single compaction, multiple compactions (only latest matters), keep-all when `firstKeptEntryId` is first, skip repeated compactions when kept messages still fit, **re-summarize previously kept messages when the recent window moves past them**, large-session end-to-end (produce a valid session after compaction).
- `agent-session-branching.test.ts`: fork from a single message, in-memory forking in `--no-session` mode, fork from the middle — the `/tree` branch primitive, adjacent to but not the same as compaction.

Notably, **no peer exercises a summarizer-error → deterministic-fallback path**:
opencode's `catchTag` fallback and pi's summary generation are not tested against a
failing model. That is precisely the case we should own.

## Completeness gaps

Behaviour agent-seddon should *guarantee via tests* to be the most complete (this
feature is spec-hardening — the impls above already exist; do **not** add new
behaviour here beyond a test-only `FailingProvider` double):

- **Sliding-window compaction is covered.** Drop-oldest until under target;
  never trim below 2 messages; exactly-at-budget is a no-op; a leading orphan
  `tool` result is dropped so the window never *starts* with a tool message.
- **Summarizing head+tail retention is pinned.** The leading system message(s)
  survive **verbatim**; a single `## Summary of earlier conversation …` system
  message is inserted immediately after the head; the `keep_recent_tokens` tail
  is preserved as-is; message count strictly decreases.
- **Orphan-tool-at-tail folding.** When the recent tail would begin with a `tool`
  message, `cut` advances past it so the tool result is folded into the summary
  (the kept tail never starts with an orphan `tool`).
- **Summarizer-error → truncation fallback (the marquee case).** When the
  summarizer's `complete` returns `Err`, `compact` must not fail: it falls back to
  `drop_oldest`, ending under target with the head preserved and no orphan `tool`
  leading the window. Requires a test-only `FailingProvider` double.
- **`cut <= head` → truncation fallback.** Compaction requested but nothing
  meaningful sits between head and tail ⇒ truncate rather than summarize an empty
  slice (the summarizer must not even be called).
- **Boundary inputs through `compact`:** empty history (no-op, no panic),
  all-system history (nothing to trim), single-message window (untouched).

## Table-driven test plan

Three tables. Compaction cases live next to their impls: add a `compact_cases`
table to
[`crates/agent-context/src/sliding_window.rs`](../../crates/agent-context/src/sliding_window.rs)
and to
[`crates/agent-context/src/summarizing.rs`](../../crates/agent-context/src/summarizing.rs).
Keep the existing `estimate_tokens` / `assemble` tables in `lib.rs` as-is.

Doubles: reuse the in-module `long(role, n)` / `msg(role, content)` helpers already
in `summarizing.rs` and the existing `FixedSummarizer`. Add **one** new test-only
double — a `FailingProvider` whose `complete` returns `Err(Error::Provider(...))`
— to drive the summarizer-error path (mirrors `FixedSummarizer`; belongs in the
`summarizing.rs` test module, since `agent-testkit`'s `ScriptedProvider` can't
script an error). No `tempdir`/filesystem needed — context strategies are pure.

Case-prefix key: `positive_` compacts successfully, `negative_`/`boundary_` an
edge that must be a no-op or a guarded fallback, `corner_` odd-but-valid input.
`(new)` marks a case with no peer origin (the norm here — peers barely test this);
`(port: …)` marks the rare case with a peer analogue.

```rust
// crates/agent-context/src/sliding_window.rs  — target file
// Doubles: none (pure). Helpers: local `long(role, n)` / `msg(role, content)`.
//
// Signature idea: (messages, budget) -> expected assertion on the compacted set.
// `Expect` distinguishes "unchanged", "fits under target", "starts with a
// non-tool message", etc. — pick whatever the harness finds cleanest; the cases
// below name the invariant each pins.
#[rstest]
#[case::boundary_empty_history(vec![], (100, 10))]                       // no-op, no panic            (new)
#[case::boundary_under_budget_noop(
    vec![(Role::System, 4), (Role::User, 4)], (100_000, 1_000))]        // untouched                  (new)
#[case::boundary_exactly_at_budget_noop(
    vec![(Role::System, 40), (Role::User, 40)], /*target == est*/ ())]  // == target ⇒ no drop        (new)
#[case::positive_drops_oldest_until_fit(
    vec![(Role::System, 20), (Role::User, 400), (Role::Assistant, 400),
         (Role::User, 400), (Role::Assistant, 20)], (500, 100))]        // fits under target          (port: pi)
#[case::boundary_never_below_two_messages(
    vec![(Role::System, 800), (Role::User, 800)], (10, 5))]             // len stays 2 even if huge   (new)
#[case::negative_no_leading_orphan_tool(
    vec![(Role::System, 20), (Role::Tool, 400), (Role::User, 20)], (60, 10))]
                                                                        // first non-system ≠ Tool    (new)
#[tokio::test]
async fn sliding_compact_cases(#[case] spec: Vec<(Role, usize)>, #[case] budget: (u32, u32)) {
    // build WorkingSet from spec via long(role, n); run SlidingWindow.compact;
    // assert: estimate_tokens(&msgs) <= target OR len == 2 (can't trim further),
    // and the first non-system message is never Role::Tool.
}
```

```rust
// crates/agent-context/src/summarizing.rs  — target file
// Doubles: existing `FixedSummarizer` (always "SUMMARY"); NEW `FailingProvider`.
//
// A summary succeeds ⇒ head kept verbatim, one summary system message after it,
// recent tail preserved, count decreases. A summary fails ⇒ truncation fallback.
#[rstest]
#[case::positive_summarize_keeps_head_and_tail(Summarizer::Fixed)]      // head verbatim + "SUMMARY" + tail  (new)
#[case::corner_orphan_tool_tail_folded(Summarizer::Fixed)]             // cut advances past leading Tool     (new)
#[case::negative_cut_le_head_truncates(Summarizer::Fixed)]            // nothing to summarize ⇒ drop_oldest  (new)
#[case::negative_summarizer_error_falls_back(Summarizer::Failing)]   // Err ⇒ drop_oldest, still under target (new)
#[tokio::test]
async fn summarizing_compact_cases(#[case] summarizer: Summarizer) {
    // Build a `WorkingSet` (system head + several large turns via long()) and a
    // small `TokenBudget` that forces compaction. Pick the summarizer per case.
    //
    // Fixed + summarize path asserts:
    //   - messages[0] is byte-identical to the original head (verbatim),
    //   - messages[1] is System and contains "Summary of earlier conversation",
    //   - the original recent-tail message(s) are present and unchanged,
    //   - working.messages.len() < original len,
    //   - first non-system message is never Role::Tool.
    // Failing path asserts:
    //   - compact() returns Ok (never propagates the summarizer error),
    //   - estimate_tokens(&msgs) <= target (truncation fallback ran),
    //   - head preserved, no leading orphan Tool.
    // cut_le_head asserts the summarizer was NOT called (truncation instead).
}

// NEW test-only double — the marquee gap. Mirrors FixedSummarizer.
struct FailingProvider;
#[async_trait]
impl LlmProvider for FailingProvider {
    fn capabilities(&self) -> ModelCapabilities {
        ModelCapabilities { supports_tools: false, context_window: 1000 }
    }
    async fn complete(&self, _req: CompletionRequest) -> Result<CompletionResponse> {
        Err(agent_core::Error::Provider("summarizer unavailable".into()))
    }
}
```

```rust
// crates/agent-context/src/lib.rs  — KEEP existing tables (no change needed),
// listed here so the plan is complete. estimate_tokens_cases (~5) +
// estimate_tokens_counts_tool_calls; assemble_message_count_cases (~5) +
// assemble_folds_prepend_and_recalled_into_system +
// assemble_append_is_trailing_system_message. These already pin assembly and the
// token estimate that both compaction strategies depend on.
```

## References

- **agent-seddon:** [`crates/agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs) (`ContextStrategy`, `ContextInput`, `WorkingSet`, `TokenBudget`), [`crates/agent-context/src/lib.rs`](../../crates/agent-context/src/lib.rs) (`assemble_messages`, `estimate_tokens`), [`crates/agent-context/src/sliding_window.rs`](../../crates/agent-context/src/sliding_window.rs), [`crates/agent-context/src/summarizing.rs`](../../crates/agent-context/src/summarizing.rs), [`crates/agent-testkit/src/lib.rs`](../../crates/agent-testkit/src/lib.rs) (`StaticContext`, `ScriptedProvider`), [`docs/components/context.md`](../components/context.md).
- **opencode:** `opencode/packages/core/src/session/compaction.ts`, `opencode/packages/core/src/session/projector.ts`; tests `opencode/packages/core/test/session-compaction.test.ts` (2 cases — prompt shape + media serialization), `opencode/packages/core/test/session-projector.test.ts` (DB projection, not compaction).
- **pi:** `pi/packages/coding-agent/docs/compaction.md`, README `#compaction` / `/tree`; tests `pi/packages/coding-agent/test/compaction.test.ts` (26 cases — token-anchor + cut-point math), `pi/packages/coding-agent/test/agent-session-compaction.test.ts`, `pi/packages/coding-agent/test/agent-session-branching.test.ts` (fork/`/tree`).
- **hermes:** no separate compaction seam surfaced — omitted from the peer table.
