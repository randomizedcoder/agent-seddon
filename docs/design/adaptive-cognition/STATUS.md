# Adaptive Cognition — implementation status

The living tracker for the [Adaptive Cognition](README.md) design. One gated PR per
increment, in dependency order (02 & 03 consume 01's `ModeSwitch`/`current_mode`).

## Increments

| # | Increment | Seam | Wire | Metrics | Tests | Bench+Leak | Status |
|---|---|:--:|:--:|:--:|:--:|:--:|:--:|
| 01 | Mode detection & switching | ✅ | ✅ | ✅ | ✅ | ✅ | **merged** |
| 02 | Mode-aware compaction | ✅ | ✅ | ✅ | ✅ | ✅ | **merged** |
| 03 | Dimensional memory | ✅ | ✅ | ✅ | ✅ | ✅ | **in review** |

## 01 — what shipped

- **`agent-mode` crate** (`HybridClassifier`), relocated out of `agent-review` — mode
  detection is general, always-on, not a review feature. Generalized to the full
  `TaskMode` taxonomy (was Review/Other).
- **Runtime:** session `current_mode` + `switch_history`; per-turn, history-aware
  classification in `send_inner`; a hysteresis switch decision; `ModeSwitch` recorded
  (metric + `mode.switch` span + episodic `mode_switch` event → `agent_events`).
- **Review is one consumer:** its in-loop hand-off now *reads* `current_mode` instead
  of classifying on its own (`review` feature implies `mode`).
- **Contract:** `mode.proto` (`ModeService.Classify`, additive — no buf baseline
  bump); `--serve-mode` gRPC server + `grpc` client; `mode` port block in
  `nix/constants.nix`; `agent_mode_*` metrics; `agent --detect-mode` offline CLI;
  table-driven + `adversarial_` tests; `classify` bench (Ir ceiling) + dhat leak;
  hermetic `nix/checks/mode-detect.nix`. Docs: `docs/components/mode.md`.
- **Default ON:** with no `[pool]` configured only the free prefilter runs per turn.

## 02 — what shipped

- **`ModeAwareWindow`** (`context-mode-aware` feature) wraps `SummarizingWindow`:
  ordinary budget compaction between switches, and on a switch a **reshape
  regardless of budget** that re-summarizes the demoted middle through the
  **destination mode's lens** (`lens_instruction(to)`). Head + recent tail are kept
  verbatim (the two table invariants); the episodic log is untouched.
- **Seam threading:** two additive **default methods** on `ContextStrategy` —
  `on_mode_switch(from, to)` (arms the next `compact`) and `last_compact_action()`
  (telemetry). A downcast probe can't cross the `MeteredContext`/`GrpcContext`
  decorator chain, so the capability rides the trait; decorators forward it.
- **Runtime:** `record_mode_switch` (01) now also calls `context.on_mode_switch`, so
  the reshape nests under `mode.switch`. Registered as `mode-aware-window` in the
  registry; default-on `context-mode-aware` feature.
- **Fail-soft chain:** switch-lens summary → generic summary → drop the span. The
  summary is **`scan_for_injection`-screened** before it re-enters context as a
  system message; a flagged summary is dropped, not obeyed.
- **Contract:** additive `from_mode`/`to_mode` on `CompactRequest` + `CompactStats`
  on `CompactResponse` (context service, no buf baseline bump); gRPC client arms via
  the wire fields and reads back the action; `agent_context_switch_compactions_total`,
  `agent_context_tokens_shed{trigger}`, `agent_context_summary_fallback_total{kind}`
  metrics; enriched `context.compact`; table-driven + `adversarial_` tests (flagged
  summary dropped, huge message bounded); `mode_aware` partition bench (Ir ceiling) +
  dhat leak. Docs: `docs/components/context.md`.

## 03 — what shipped

- **`FileDimensions`** (`agent-memory`, `memory-dimensions`) + the **`DimensionStore`**
  seam (`summarize_step` + `recall_dimension`). A cheap per-turn structured-output
  pass files `{dimension, summary, is_new}` sets into `<semantic_dir>/dimensions/
  <dim>.md`; a step may touch several axes at once.
- **Hybrid taxonomy:** the closed seed set (`coding · git · user · project · testing
  · tooling · docs`) plus emergent slugs admitted only after `K`-recurrence and under
  `MAX_DIMENSIONS`; pending slugs file under `misc`. Growth bounded three ways
  (`MAX_DIMS_PER_STEP`, `K`, `MAX_DIMENSIONS`). A size-cap **synthesis** pass
  re-summarizes an over-large file (fail-soft to truncation).
- **The bridge to 02:** on a mode switch the runtime recalls the destination mode's
  dimensions (`recall_dims_for`) and injects them as fresh context — the before/after
  table's "pull in fresh" column, which makes 02's shed safe.
- **Fail-soft + opt-in:** off by default (`[dimensions] store`), since the per-step
  pass is a real local-tier LLM call (unlike mode detection's free prefilter). No
  provider / dead pool / malformed output ⇒ a no-op.
- **Security:** emergent slugs pass `safe_segment` (no traversal); summaries are
  bounded + `scan_for_injection`-screened before persisting (a dimension file is
  recalled verbatim). The hard episodic log is never rewritten.
- **Contract:** `dimension.proto` (`DimensionService.Summarize` + `Recall`, additive
  — no buf baseline bump); `--serve-dimension` server+client; `dimension` port block;
  `agent_dimension_*` metrics; `memory.dimension.*` spans; `agent_dimension_summaries`
  ClickHouse table (counts/lengths only); table-driven + `adversarial_` tests;
  `dimensions` parse bench (Ir ceiling) + dhat leak. Docs: `docs/components/dimensions.md`.

## Deferred (from 03)

- **Dimension-weighted recall via `RecallQuery.dimension`** on the existing
  `MemoryService` — 03 recalls via the `DimensionStore` seam instead (self-contained,
  no churn to every `RecallQuery` call site).
- **Slug-rejected / active-dimension-count metrics** — need store→metrics plumbing;
  the three core `agent_dimension_*` families ship.
- **Heavy-tier synthesis routing** — the synthesis pass uses the main provider; a
  dedicated GLM tier rides the pool when configured.
- The design's own deferrals: embedding-based dimension recall, learned taxonomy,
  learned mode→dimension weights.

## Deferred (from 02)

- **Learned sheds** — record `CompactStats` + downstream outcome and learn which
  sheds hurt. Recording is specified; the learning is not built.
- **Per-mode budgets** — a review tail may warrant more room than an explain one.
- **Pull-in-fresh from `03`** — the table's last column lands with dimensional memory.

## Deferred (from 01)

- Mid-turn (per loop-iteration) re-classification — 01 classifies per user turn.
- The deterministic `RepoSignals` prefilter inputs (branch/git-status/PR-ref) beyond
  prompt cues — designed, not yet wired.
- A dedicated `agent_mode_switches` ClickHouse table — switches land in `agent_events`
  for now.
