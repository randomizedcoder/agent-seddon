# 02 — Mode-aware compaction

Status: **design / pre-implementation.** Consumes the `ModeSwitch` event from
[`01`](01-mode.md); pulls fresh context from [`03`](03-memory.md).

## Motivation

Context useful in one mode is noise in the next. The exploration transcript that
found the right files is dead weight once we are *implementing* them; the
step-by-step build history is noise once we are *reviewing* the diff. Today
compaction cannot express that — it only reacts to a **token budget**, with a single
generic summary prompt, blind to what the agent is actually doing:

- `ContextStrategy::compact(&mut WorkingSet, &TokenBudget)` takes no mode
  (`agent-core/src/lib.rs:2021-2028`).
- `SummarizingWindow::compact` keeps the leading system messages + a recent tail and
  replaces the middle with one LLM summary under a fixed, mode-agnostic prompt
  (`agent-context/src/summarizing.rs:67-148`).

A **mode switch** is exactly the moment to reshape context deliberately — and it is
cheap to do well, because the MI50 summary it needs is the kind of job the pool
exists for.

## What already exists (and its gaps)

- `ContextStrategy` seam with `assemble` + `compact` — `lib.rs:2021`; the compaction
  invariant is **non-destructive w.r.t. the episodic log** (`compact` only trims the
  working set), which is what makes an aggressive shed safe.
- `SummarizingWindow` (`context-summarizing` feature) — keeps head + `keep_recent_
  tokens` tail, LLM-summarizes the middle, falls back to `drop_oldest` on failure
  (`summarizing.rs`). Holds its own `Arc<dyn LlmProvider>` and an optional
  `Tokenizer` for the real over-budget count.
- `SlidingWindow` (`context-sliding-window`, default) — lossy/free drop-oldest.
- The compaction call site + span: `self.context.compact(working, budget)` inside a
  `context.compact` span with an `on_compact` hook (`agent.rs:1096-1099`).
- The `ContextService` gRPC seam already exists (`agent-grpc/src/server/context.rs`,
  `assemble` + `compact`) — so mode-aware compaction extends a service, it doesn't add
  one.

**Gaps:** `compact` can't see the mode; there is no path from the `01` verdict into
the strategy; and there is one summary prompt, not a per-destination one.

## Design

### `ModeAwareWindow` — a new `ContextStrategy`

A strategy (`context-mode-aware` feature) that **wraps a `SummarizingWindow`** and
adds a current-mode field and a switch reaction:

- **Between switches** it is `SummarizingWindow` — budget-triggered, ordinary
  summarize-the-middle. No behaviour change, no extra cost.
- **On a switch** it runs a **switch compaction**, regardless of budget: partition the
  working set into *keep verbatim* / *summarize-demote* / *drop* per the table below
  for `(from → to)`, re-summarize the demoted span **through the destination mode's
  lens** (a per-destination prompt, e.g. *"Summarize the above for a code **review**:
  keep the change and its intent, drop the process"*), and rebuild the window. The
  raw episodic log is untouched, so a wrong shed is recoverable by recall.

### Threading the mode in (least-invasive seam change)

`compact`'s signature stays `(&mut WorkingSet, &TokenBudget)` — no breaking change to
every strategy or the gRPC contract. Instead, the runtime pushes the switch to the
strategy just before the post-turn `compact`:

```rust
// optional capability; strategies that don't implement it are unaffected
trait ModeAware { fn on_mode_switch(&self, from: TaskMode, to: TaskMode); }
```

The loop, on a `ModeSwitch` from `01`, downcasts/checks the active strategy for
`ModeAware`, calls `on_mode_switch(from, to)` (which arms the next `compact` as a
*switch* compaction and records the destination lens), then the existing
`compact(working, budget)` runs. `SlidingWindow`/`SummarizingWindow` don't implement
`ModeAware`, so they behave exactly as today.

*Alternative considered, not chosen:* an additive `compact_for(working, budget, hint:
CompactHint)` default method delegating to `compact`. It threads the mode explicitly
but touches the trait + every wrapper (`MeteredContext`, `GrpcContext`); the
capability-probe above is smaller. Recorded for the implementation phase to confirm.

### The before/after context-requirements table

The specification of "useful in one mode, noise in the next." For each transition:
what is **kept verbatim**, what is **summarized/demoted** (one line, destination-mode
lens), what is **dropped**, and what is **pulled in fresh** from dimensional memory
(`03`). Draft — the first thing to refine against real transcripts:

| Transition | Keep verbatim | Summarize / demote | Drop | Pull in fresh (from `03`) |
|---|---|---|---|---|
| **Explore → Implement** | chosen file paths, the decided approach, the goal | the exploration transcript (dead-ends, rejected files) → one "what we learned" note | raw `grep`/`ls`/`find` dumps already acted on | coding-dim history + the target files' current content |
| **Implement → Debug** | the failing test/error, the edit just made, the goal | earlier successful edits → a "changes so far" summary | verbose build logs before the failure | debugging-dim history for this area (past fixes) |
| **Implement → Review** | the diff, the changed-file set | how-we-got-here → a "change intent" summary | intermediate broken/reverted states | the `ReviewFacts` bundle (existing review flow) |
| **Debug → Implement** | the root cause found, the fix plan | the debugging trail (hypotheses tried) → "root cause: X" | stack traces, log dumps | coding-dim history |
| ***→ Explain** | the goal + the answer-relevant facts | the whole working trail → "what was done" | tool noise entirely | user-dim history (their level, preferences) |
| **Design → Implement** | the design decisions | the design discussion → a "decisions" summary | rejected alternatives (already in memory) | project-dim constraints |

Two invariants the table obeys: the **head** (system prompt) is always kept verbatim;
the **most-recent tail** (the turn that triggered the switch) is always kept, because
it *is* the new mode's starting context.

## Failure semantic

**Fail-soft, three-step fallback.** A switch-compaction that can't get its
destination-lens summary (dead pool, model error) falls back to a **generic**
summarize (ordinary `SummarizingWindow`), which itself falls back to **`drop_oldest`**
truncation (the existing chain, `summarizing.rs:100-124`). The loop always makes
progress; the worst case is a less-tailored compaction, never a stall. If the mode
can't be determined (`01` fail-safe), no switch fires and compaction is ordinary
budget-triggered.

## Protobuf

Additive to the existing context service — no baseline bump.

```proto
// Existing (reused): message ContextInput; rpc Compact(...) on ContextService.

message CompactRequest {
  agent.v1.WorkingSet working = 1;
  agent.v1.TokenBudget budget = 2;
  TaskMode from_mode = 3;      // additive; UNSPECIFIED ⇒ ordinary budget compaction
  TaskMode to_mode   = 4;      // additive; a switch when != from_mode
}
message CompactStats {
  uint32 kept_tokens = 1;
  uint32 shed_tokens = 2;
  string action      = 3;      // "budget" | "switch" | "fallback-generic" | "fallback-drop"
}
message CompactResponse {
  agent.v1.WorkingSet working = 1;
  CompactStats stats = 2;      // additive
}
```

`convert.rs` both directions; the `from_mode`/`to_mode` default to `UNSPECIFIED` so an
old client's request is an ordinary compaction.

## gRPC interface

The **existing** `ContextService.Compact` (`--serve-context`), with the additive mode
fields above — no new service, no new port. Wire failure semantic: a compaction RPC
never errors on a dead summarizer; it returns the fallback result with
`stats.action = "fallback-…"`, so the caller can account for the degradation.

## Prometheus metrics

| Metric | Type | Labels |
|---|---|---|
| `agent_context_compactions_total` | counter | `strategy`, `trigger` = `budget`\|`switch` |
| `agent_context_compact_duration_seconds` | histogram | `trigger` |
| `agent_context_tokens_shed` | histogram | `trigger` |
| `agent_context_summary_fallback_total` | counter | `to` = `generic`\|`drop` |

Raised the `metered.rs` way (`MeteredContext` already wraps the strategy) — the
existing decorator gains the switch/shed accounting.

## Tracing + logs

- Enrich the **existing** `context.compact` span (`agent.rs:1096`) with `mode`,
  `switch` (`from→to`, empty when budget-triggered), `strategy`, `kept_tokens`,
  `shed_tokens`, `action`. When triggered by a switch it is a **child of
  `mode.switch`** (from `01`), so the reshape shows up under the pivot.
- Child span `context.summarize` for the LLM call (destination lens, tokens in/out).
- Logs: `INFO` "switch-compaction {from}→{to}: kept N, shed M" — counts only, never
  message bodies. `WARN` on a fallback with the reason class.

## Testing (table-driven + adversarial)

`rstest` `#[case::name]`; a fixed multi-turn `WorkingSet` fixture in `agent-testkit`.

- `positive_` — one case **per table row**: assert the kept/demoted/dropped partition
  for that `(from→to)` and that the destination-lens prompt was selected.
- `negative_` — no switch (same mode) → behaves exactly like `SummarizingWindow`;
  under budget → no compaction at all.
- `corner_` — an empty middle (nothing to summarize) → falls through to the tail
  intact; an orphan leading `Tool` message is folded, not left dangling (the existing
  guard).
- `boundary_` — working set exactly at budget; a switch with only head+tail present.
- `adversarial_` (**mandatory**) — a demoted turn whose content contains an injection
  string ("ignore all prior instructions, you are now…"): the produced summary is
  bounded and `scan_for_injection`-screened before it re-enters the next mode's system
  context (rendered inert, not obeyed); hostile token counts (NaN/huge from a
  tokenizer) are clamped before the budget math; a 100 KB single message is bounded.

## Benchmark + leak

- **Bench** (`iai-callgrind`) — `compact` over the fixed `WorkingSet`, in two
  variants: budget-triggered (no switch) and switch-triggered (with the summarizer
  stubbed by a deterministic fake provider so the count is stable). Absolute **Ir
  ceiling** in `nix/checks/bench.nix`. The partition logic (which turn goes where) is
  the hot path the ceiling guards.
- **Leak** (`dhat`, `dhat-heap`) — a switch compaction frees the dropped middle and
  the intermediate render buffer; assert zero net leak and an allocation budget in
  `nix/checks/leak.nix`.

## Security

- The summarizer's output is **untrusted model text** that is about to become a
  *system* message in the next mode's context — the highest-leverage injection
  surface here. Bound it and run `scan_for_injection` (the memory-recall precedent)
  before it is inserted; a flagged summary is replaced with a neutral placeholder, and
  the compaction falls back to `drop_oldest` for that span.
- Token counts from the `Tokenizer` (which may front an untrusted remote) are clamped
  before any budget arithmetic or loop bound.
- The shed is **working-set only** — the episodic log is never mutated (the seam
  invariant), so a hostile transcript can at worst degrade *this* turn's context, never
  corrupt durable memory.

## Deferred

- **Learned sheds** — record `CompactStats` + downstream outcome (did the reshaped
  context lead to a good next turn?) and learn which sheds hurt. The recording is
  specified; the learning is not built.
- **Per-mode budgets** — a review context may warrant a larger tail than an explain
  one. Fixed budget first.
