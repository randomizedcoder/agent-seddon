# Increment 03 — Instant context compaction

Goal: compaction stops summarizing the whole history synchronously
(`summarizing.rs:147`, up to 1024 output tokens on the critical path each time
the budget crosses) and instead **assembles** from the digest ledger increment 02
has been filling: one small objective call + a selection pass + concatenation.
"Parallel Context Compaction" (arXiv 2605.23296) measures synchronous compaction
at up to 62% of agent wall time — this removes it from the critical path
entirely.

## Shape

New `ContextStrategy`: `crates/agent-context/src/instant.rs` —
`InstantWindow { digests: Arc<dyn DigestStore>, objective_provider:
Arc<dyn LlmProvider>, inner: SummarizingWindow /* fallback */, tokenizer, cfg }`,
registered as `[context] strategy = "instant-window"` (composing factory:
builds the fallback exactly as `summarizing-window` does today).

`compact(working, budget, switch)`:

1. **Trigger** unchanged: over-budget, or an armed mode switch (delegating the
   switch-lens behavior to the `mode-aware` path is config: `instant` wraps
   either window).
2. **Current objective** (the phase-shift problem: a coding session's focus
   moves, so old summaries may be off-goal): one bounded completion
   (`objective_max_tokens = 128`) over the protected head + recent tail — "state
   the current objective in ≤3 sentences." Stored as a `kind = objective` digest
   (session history of objective drift for free). Fail-soft: on error, fall back
   to the session goal (first user message).
3. **Candidate fetch**: `DigestStore::query(kind = Summary)` for the span being
   compacted (summaries whose `seq` falls in the to-be-dropped window), ordered.
   **Gap check**: if digest coverage of the span is below `min_coverage`
   (default 0.6 — distiller lag, dropped jobs, or a pre-feature session), the
   ledger can't represent the span faithfully → fall back to the inner
   summarizing window. Never assemble from a half-empty ledger.
4. **Relevance selection** (objective-conditioned — the novel step):
   - Cheap prefilter first: keyword overlap with the objective (pushed down to
     the store — `hasAny(keywords, [...])` on the ClickHouse backend via
     `DigestQuery.keywords_any` — plus BM25 via tantivy when the search feature
     is on) drops clearly-off-goal summaries.
   - Then **one** bounded LLM pass over the surviving candidates *as a batch*
     ("mark each summary keep/drop for this objective"), `role = classify`
     routing (cheap upstream). Config `relevance = "llm" | "keyword" | "all"` —
     `keyword` makes compaction **zero-LLM** except the objective call.
   - Dropped ≠ deleted: rows stay in the store; only assembly is filtered.
5. **Assemble**: protected head + one system message —

   ```
   ## Current objective
   {objective}
   ## Summary of earlier conversation (from the session ledger)
   {relevant summaries, seq order, merged}
   ## Key facts
   {facts rows for the session, seq order}
   ## Open alternatives (with reconsideration triggers)
   {alternatives rows, seq order: option — summary — reconsider when: …}
   ```

   — + protected recent tail. Facts are tiny by construction (increment 02 caps)
   and are appended wholesale; a `facts_max_chars` guard truncates oldest-first
   if a pathological session blows the cap. **Alternatives survive compaction
   by design** — this is the moment they would otherwise be forgotten: the
   `kind = alternatives` rows (recorded by the gate, increment 01/02) are
   appended with their `reconsider_when` triggers so a phase shift or new
   information later in the task can still resurrect the road not taken. They
   pass through the same relevance selection as summaries (an alternative about
   an abandoned sub-goal drops out of *assembly* — never out of the store) and
   share an `alternatives_max_chars` cap, newest-first.
6. **Screening + invariants**: every retrieved text is
   `scan_for_injection`-screened at read (defense in depth — it was screened at
   write too); flagged content is dropped and counted. After assembly, re-check
   the spec-42 invariants (≤ target or the 2-message floor; first non-system
   message never a tool message; recent tail intact) — violated ⇒ fall back.
7. **Fallback chain**, every step fail-soft:
   `instant → inner summarizing window → drop-oldest`, mirrored in the metric
   `compaction_total{implementation = instant|summarize|drop, outcome}` (the
   spec-42 axis gains the `instant` value).

## What it costs vs. today

| | today (`summarizing-window`) | instant |
|---|---|---|
| LLM input | entire compacted span | head+tail (objective) + candidate summaries (relevance) |
| LLM output | ≤1024 tokens, critical path | ≤128 (objective) + keep/drop marks; 0 extra with `relevance="keyword"` |
| The heavy summarization | at compaction time | already paid, per-turn, in the background |

## Config

```toml
[context]
strategy = "instant-window"

[context.instant]
relevance = "llm"           # "llm" | "keyword" | "all"
objective_max_tokens = 128
min_coverage = 0.6
facts_max_chars = 4096
alternatives_max_chars = 2048
```

## Tests / verification

Fixtures come from `agent_digest::testdata` (README §Harness obligations): the
corpus's phase drift (explore → implement → debug → document) is exactly the
relevance-selection fixture — an `explore`-phase summary must drop from
assembly when the current objective says `debug`; `populated_sqlite` gives an
ephemeral ledger per test, and the same scenarios re-run live against
ClickHouse via the ignored round-trip pattern.

- `positive_`: over-budget compaction assembles objective + relevant summaries +
  facts + tail; relevance drops the off-objective summary (scripted keep/drop,
  and a corpus-driven case keyed on phase keywords).
- `negative_`: digest store erroring → inner fallback (and metric); objective
  call error → session-goal fallback, assembly proceeds.
- `corner_`: empty facts; summaries present but all dropped by relevance (still
  valid — objective + facts + tail); armed mode switch path; alternatives
  re-injected across two consecutive compactions (not lost at the second);
  off-objective alternative dropped from assembly but still in the store.
- `boundary_`: coverage exactly at `min_coverage`; facts at `facts_max_chars`;
  budget exactly at target (no-op).
- `adversarial_`: injection-flagged summary/fact dropped and counted, never
  assembled; hostile keywords in the objective; digest rows with absurd seq/token
  values clamped; assembled result re-checked against invariants.
- iai bench: assembly path (fetch mocked) under an Ir ceiling — the point of the
  feature is that this path is cheap; the bench enforces it stays so.
- Live: long Kimi session past the budget → observe `implementation="instant"`,
  wall-time delta vs `summarizing-window`, and coherent post-compaction behavior.
