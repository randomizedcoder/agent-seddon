# Chunked map-reduce review (design-of-record)

**Status:** ⬜ designed, not built. Companion to [STATUS.md](STATUS.md); this is the design for **lever 3**.

## Why

OTLP tracing of the l2 review-fleet (see [STATUS.md](STATUS.md)) established that **~90% of a review's
wall-time is remote-Kimi inference wait**, and that the dominant **single-PR** latency tail is a
**big-PR iteration runaway** — PR `runpod/host#2828` (40 files / 3298 churn) issued 41 sequential model
turns and hit `max_iterations` (~361s). Two levers already landed narrow the problem but do not solve
this case:

- **`stream=true`** removed the 157s monolithic-completion tail (confirmed win).
- **The non-convergence guard** ([lever 4](STATUS.md), `[agent] max_unproductive_iters`) caps a loop
  that *repeats* already-seen tool calls — but it deliberately does **not** fire on a review that
  explores with *distinct* calls because the diff genuinely doesn't fit the model's working budget.

That remaining case — a large PR the model must explore serially — is what chunking targets. **Within a
single review the ReAct loop is strictly serial** (turn N needs turn N-1's tool result), so one big PR
uses ~1 slot of the ≥3× B300 Kimi cluster no matter how wide the cluster is. Chunking turns one serial
review into a **map-reduce** so a single PR also fills the cluster: split the diff into K coherent
chunks, review them **in parallel**, then merge.

Measured cross-review concurrency (already working) is 1.67× @2 PRs / 2.3× @3 — so **throughput** for a
fleet grinding many PRs is largely solved. Chunking's unique payoff is **single-PR latency** on the big
sprawling PRs, and secondarily reducing per-chunk runaway (a bounded chunk is far less likely to spiral
than the whole 40-file diff).

## Non-goals / risk acknowledgement

This is the **highest-risk** lever and may not pay off. It ships **only if a `fleet-measure` before/after
shows a real latency win with no material recall loss** (see the go/no-go gate below). If the recall
check fails or the win is marginal, the increments are marked ❌ in STATUS.md and the code is not merged.

## Architecture — map-reduce over `facts.change.files`

```
                ┌─ split (deterministic, Rust) — partition ChangeSet.files into K bins ─┐
   PR diff ─────┤                                                                        │
   + facts      ├─ map: K parallel chunk-reviews (each a run_review → narrative+findings) ┤
                │        └ Kimi cluster runs the K concurrently                           │
                └─ reduce: merge findings (existing dedup) + merge narratives (MI50) ─────┘
                                                                                          │
                                                              one draft (unchanged shape) ┘
```

### Split — deterministic, in Rust (recommended over an LLM step)

Partition [`ChangeSet.files`](../../../crates/agent-core/src/lib.rs) (`Vec<ChangedFile>`, each already
carrying its own `patch`; built by `build_change_set`, `agent-review/src/repo_facts.rs`) into K bins:

- **Group by co-change component.** [`CoChangeCollector::compute`](../../../crates/agent-review/src/cochange.rs)
  already emits per-file coupling groups (file→file, with confidence). Keep coupled files in the **same**
  bin so a caller/callee pair (the classic cross-file bug) is reviewed together.
- **Pack under the per-chunk byte budget.** Each bin's rendered facts must fit `context_budget_bytes`
  (the same budget `render_facts_with` already enforces, dropping overflow hunks). Bin-packing by
  `additions+deletions` keeps chunks balanced.
- **Deterministic ⇒ testable, free, no extra round-trip.** An MI50 semantic-grouping pass is an optional
  future refinement, not the baseline. (Determinism also makes the recall check reproducible.)

### Map — K parallel chunk-reviews

For each chunk: render a scoped brief with
[`render_facts_with`](../../../crates/agent-review/src/lib.rs) over that chunk's files → fold into a goal
with `grounded_goal(brief_chunk)` → run one
[`run_review`](../../../crates/agent-runtime/src/agent/session_manager.rs). Fan the K out concurrently
(`buffer_unordered` / `join_all`, mirroring the collector fan-out in `ReviewOrchestrator::collect`). The
Kimi cluster serves the K in parallel. Each chunk yields a narrative string + its `AnalysisFinding`s.

### Reduce — merge K partials

- **Findings — reuse existing dedup.** `dedupe_findings` (`analyzer.rs`, keyed `(file,line,rule)`) and
  `digest::compute` (union-dedup by `(tool,rule,file,line)` + risk-rank + cap) already merge
  cross-source findings; a chunked review merges the K findings sets the same way. Overlapping chunks
  (if any) dedupe cleanly.
- **Narratives — the one genuinely new piece.** `render_draft` only redacts + size-caps; there is **no
  existing narrative-level merge**. Propose a cheap MI50 reduce step via `RouteRole::Review` (the same
  routing the Inc-4 `digest_summary` uses, `digest.rs`) to concatenate + dedupe the K narratives into
  one coherent review, **fail-soft to plain concatenation** if no healthy MI50 member.

## The sweet-spot gate (the crux)

Chunking is only invoked when it can help, and K is bounded, because each chunk **re-pays the fixed
context-ingest cost** (the ~65s "turn-1 full-context" call) — K chunks cost `K×ingest + reasoning`, not
`total/K`:

- **Gate:** chunk only when `files ≥ FILE_THRESHOLD` **and** `total_diff_bytes > context_budget` (i.e.
  the whole PR already overflows one review's budget — exactly the runaway case). Small PRs review whole.
- **K sizing:** `K = min(cluster_width, #coherent_groups)`. Never more parallel calls than the cluster
  runs at once (extra chunks re-serialize *and* re-pay ingest), and never split a coherent group.
- **Floor:** below ~1 chunk per 4–6 files the ingest tax eats the parallelism win.

## Risks

- **Recall:** a bug spanning two files in different chunks is missed. Mitigated by co-change grouping;
  optionally include the signatures/callgraph of referenced-but-external files as thin cross-chunk
  context.
- **Per-chunk ingest tax** (above) — bounded by the gate + K sizing.
- **Narrative merge quality** — the MI50 reduce could drop or duplicate points; fail-soft concatenation
  is the floor, and findings (the mechanized half) are merged deterministically regardless.

## Measurement / go-no-go

`fleet-measure` before/after on a set of **big** PRs (a #2828-class 40-file PR is the canonical case):

1. **Latency:** single-PR wall-clock, and per-chunk iteration counts (each chunk should be bounded — no
   runaway).
2. **Recall (the gate):** findings from the chunked review vs the whole-PR review on the *same* PR.
   **Ship only if chunked drops no material findings.**
3. **Cluster fill:** effective parallelism during one review should rise from ~1× toward K.

## Increments (each a future gated PR)

1. **Deterministic co-change chunker** — pure Rust, partition `ChangeSet.files` by co-change component
   under the byte budget. Table-driven tests.
2. **Parallel map + findings-reduce** in the orchestrator — K parallel `run_review`s, merge findings via
   the existing dedup.
3. **MI50 narrative merge** — the reduce step over the K narratives, fail-soft to concatenation.
4. **The gate + K sizing + live validation** — the `FILE_THRESHOLD`/budget gate, `K` sizing, and the
   `fleet-measure` recall/latency go-no-go on l2.

Each increment's tests follow the repo convention: **table-driven `rstest`** with description +
expected-outcome columns, covering `positive_`/`negative_`/`boundary_`/`corner_` **plus mandatory
`adversarial_`** for the untrusted diff/patch input (traversal paths, huge hunks, injection in file
names).
