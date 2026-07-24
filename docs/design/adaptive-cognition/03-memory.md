# 03 — Dimensional memory

Status: **design / pre-implementation.** Closes the loop from [`01`](01-mode.md) /
[`02`](02-compaction.md): a mode switch **flushes** the shed transcript into
per-dimension histories, and **dimension-weighted recall** feeds the "pull in fresh"
column of `02`'s before/after table.

## Motivation

The agent already summarizes durable facts — but as **one flat blob** at session end,
opt-in, and undirected. That is the wrong granularity for the mode loop: when we enter
Implement we want the *coding* and *testing* history; when we enter Review we want
*coding + git + project*; when we Explain we want what we know about the *user*. A
single blob can't be recalled by axis.

The idea: with near-free local tokens, spend a **cheap MI50 call per step** to review
what just happened and file it into **per-dimension histories** — and a step can
belong to several dimensions at once ("this step was both *coding* and *git*"). That
directly serves the compaction pivot and turns memory from a single end-of-run summary
into a queryable, mode-relevant store.

## What already exists (and its gaps)

- The layered memory seam: `MemoryStore` / `EpisodicStore` / `SemanticStore` +
  `LayeredMemory` — `agent-core/src/lib.rs:1901-1985`. `append` → episodic, `recall` →
  semantic, `distill` → read the recent episodic tail, promote.
- `FileSemantic::distill` — the **LLM-summarize precedent**: renders the episodic tail
  to a transcript, one completion under `DISTILL_SYSTEM_PROMPT` (temp 0, 1024 tok),
  screens the output with `scan_for_injection`, writes **one** curated markdown file.
  Opt-in via `[memory] distill` (`agent-memory/src/file.rs:171-217`).
- `MemoryEvent { kind, message, ts_ms, … }` — `lib.rs:1872-1895`. `kind` is a
  free-form routing tag (`goal`/`tool`/`review`/…), **not** a semantic axis. `usage`,
  `verification`, `review` are additive serde-defaulted **side-channels**.
- The recording path: `CompositeMemory` mirrors every event to ClickHouse routed by
  `kind` (`agent-telemetry/src/memory.rs`, dispatch `lib.rs:86-107`); `ReviewRow`/
  `record_review` is the precedent for a new typed row + table.
- `scan_for_injection` (`file.rs`), `safe_segment` (`agent-git`), the recency tail-cap
  (`file.rs:78-80`), and the structured-output seam.

**Gaps:** nothing categorises a step by dimension; distillation is one blob at session
end, not per-step per-dimension; recall can't filter by axis.

## Design

### The per-step dimension pass

On each step (or a small window of recent events — tuneable, default a handful of
turns), the runtime renders the recent episodic tail and makes **one cheap MI50
call** (medium tier, structured output) that returns a **set**:

```rust
// structured output — validated at the tool-call layer, model retries on mismatch
struct DimensionStep { summaries: Vec<DimensionSummary> }        // 0..=MAX_DIMS_PER_STEP
struct DimensionSummary { dimension: String, summary: String, is_new: bool }
```

Each `{dimension, summary}` is appended to that dimension's history file
`.agent/memory/dimensions/<dim>.md` (git-friendly markdown, like the semantic store).
A step that touches several axes yields several summaries — the "coding *and* git"
case — bounded by `MAX_DIMS_PER_STEP`.

### Hybrid taxonomy: seeded + emergent

- **Seed set** (closed, always valid): `coding · git · user · project · testing ·
  tooling · docs`. Covers the common axes deterministically.
- **Emergent**: the model may propose a *new* slug (`is_new = true`) when a step fits
  none — the user's "was there a new dimension about the user?" case. To keep this
  bounded and safe, a new dimension is **admitted only after it recurs `K` times**
  (tracked in a small pending-slugs ledger) **and** while the total dimension count is
  under `MAX_DIMENSIONS`. Until admitted, a pending slug's summaries file under a
  catch-all `misc` dimension. Every slug — seed or emergent — passes `safe_segment`
  before it becomes a path component.

### History upkeep (cheap-then-heavy)

A per-dimension file has a rolling size cap (recency, like `FileEpisodic::recent`).
When one grows past its budget, a **periodic GLM-5.2 pass** does a **cross-dimension
synthesis / re-summarize** — merge and dedup that dimension's history into a tighter
form (the distill-compaction pattern, but scoped to one axis and using the heavy model
because a good merge is worth it and it is infrequent).

### Dimension-weighted recall (the bridge to `02`)

`recall` gains an optional dimension filter. On a mode switch (`01`), the runtime
issues a dimension-weighted recall for the destination mode and hands the results to
`02` as the "pull in fresh" column:

| Entering mode | Recalled dimensions |
|---|---|
| Implement | coding, testing |
| Debug | coding, testing (+ the area's debugging history) |
| Review | coding, git, project |
| Design | project, docs |
| Explain | user |

This is what makes a shed *safe*: the content `02` drops from the working set was
already flushed here and is recalled when its mode comes around.

### Recording

Mirror a `kind = "dimension"` `MemoryEvent` (carrying a `DimensionalRecord`
side-channel) → a new `agent_dimension_summaries` ClickHouse table, so the dimension
distribution and emergent-slug churn can be analysed offline (the `record_review`
pattern). The side-channel is dropped at the gRPC memory boundary, like `review`.

## Failure semantic

**Fail-soft and opt-in.** No provider / dead pool → the pass is a no-op (like today's
`distill` with no provider); the loop is unaffected and the hard episodic log is
untouched. A malformed structured-output response → retried by the structured-output
layer, then skipped. A rejected slug or flagged summary → dropped with a count, never
persisted. Recall with no dimension files → an empty vec (as today).

## Protobuf

Additive — no baseline bump.

```proto
message DimensionSummary {
  string dimension = 1;   // safe_segment'd slug
  string summary   = 2;   // bounded
  bool   is_new    = 3;
}
message DimensionStep { repeated DimensionSummary summaries = 1; }   // capped server-side

message DimensionalRecord {          // MemoryEvent side-channel → agent_dimension_summaries
  repeated DimensionSummary summaries = 1;
  string session_id = 2;
  uint64 ts_ms      = 3;
}

// RecallQuery gains an additive optional dimension filter.
message RecallQuery {
  string text = 1;
  uint32 limit = 2;
  string dimension = 3;   // additive; empty ⇒ unfiltered (today's behaviour)
}

message SummarizeRequest { repeated agent.v1.MemoryEvent events = 1; }   // bounded server-side
```

`convert.rs` both directions; `DimensionalRecord` rides `MemoryEvent` as a
serde-defaulted side-channel, dropped at the gRPC boundary.

## gRPC interface

```proto
service DimensionService {
  rpc Summarize (SummarizeRequest) returns (DimensionStep);
}
```

`--serve-dimension`, endpoint from a **new `dimension` block** in `nix/constants.nix`
(regen `constants.rs`; `constants-sync` gate). Dimension-weighted **recall** rides the
existing `MemoryService` (the additive `RecallQuery.dimension`). Wire failure semantic:
`Summarize` returns an empty `DimensionStep` on a dead pool — never a gRPC error.

## Prometheus metrics

| Metric | Type | Labels |
|---|---|---|
| `agent_dimension_summaries_total` | counter | `dimension`, `is_new` |
| `agent_dimension_summarize_duration_seconds` | histogram | — |
| `agent_dimensions_active` | gauge | — (admitted dimension count) |
| `agent_dimension_recall_total` | counter | `dimension` |
| `agent_dimension_slug_rejected_total` | counter | `reason` = `unsafe`\|`capped`\|`scanned` |

Raised the `metered.rs` way (a `MeteredMemory`/typed `DimensionEvent`), keeping
`agent-memory` off `agent-metrics`.

## Tracing + logs

- Span `memory.dimension.summarize` with `dimensions`, `new_dims`, `duration_ms`
  (counts only — never the summaries).
- Span `memory.dimension.recall` with `dimension`, `hits`; a child of `mode.switch`
  when a switch triggered it, so recall shows up under the pivot next to `02`'s
  compaction.
- Span `memory.dimension.synthesize` for the periodic GLM-5.2 merge (`dimension`,
  `before_bytes`, `after_bytes`).
- Logs: `INFO` "dimension {dim}: +1 summary (new={bool})"; `WARN` on a rejected slug
  with the reason class. Never log summary bodies or the transcript.

## Testing (table-driven + adversarial)

`rstest` `#[case::name]`; `tempdir()` + a fake structured-output provider from
`agent-testkit`.

- `positive_` — a single-axis step files under the right seed dimension; a
  multi-dimension step ("coding + git") appends to **both** files; a dimension-weighted
  recall returns only that axis.
- `negative_` — no provider → no-op, no files written; recall for an empty dimension →
  empty.
- `corner_` — a step that fits nothing → `misc`; an emergent slug seen `K-1` times is
  still pending (not yet admitted).
- `boundary_` — the `K`-th recurrence admits the emergent dimension; the
  `MAX_DIMENSIONS`-th admission is the last accepted; a file exactly at its size cap
  triggers the synthesis pass.
- `adversarial_` (**mandatory**) — an emergent slug of `../../etc/passwd` or
  `..\..\` is rejected by `safe_segment` (no file escape); a summary containing an
  injection payload is bounded + `scan_for_injection`-screened before persisting (a
  dimension file is recalled verbatim into future contexts, so a poisoned one is as
  dangerous as a poisoned semantic file); a transcript trying to spawn 10 000 distinct
  dimensions is capped by `MAX_DIMENSIONS` + the `K`-recurrence gate + `MAX_DIMS_PER_
  STEP`; a 1 MB step is bounded before it reaches the model.

## Benchmark + leak

- **Bench** (`iai-callgrind`) — the classify-parse + per-dimension append path over a
  fixed transcript fixture, with the summarize call stubbed by a deterministic fake so
  the count is stable; absolute **Ir ceiling** in `nix/checks/bench.nix`.
- **Leak** (`dhat`, `dhat-heap`) — a per-step summarize frees the rendered transcript
  and the per-dimension write buffers; assert zero net leak and an allocation budget in
  `nix/checks/leak.nix`.

## Security

- Step content is **tool/model output — untrusted.** Every emergent slug goes through
  `safe_segment` (reject `..`, separators, leading `-`, ref/path-special chars) before
  it is a path component; every summary is **bounded** and `scan_for_injection`-screened
  before it is persisted, because a dimension file is recalled verbatim.
- Emergent-dimension admission is **bounded three ways** — `MAX_DIMS_PER_STEP`, a
  `K`-recurrence gate, and `MAX_DIMENSIONS` total — so a hostile transcript cannot
  spawn thousands of files or explode recall.
- The hard episodic log is never rewritten by this pass; dimensional histories are a
  *derived, curated* layer (like semantic memory), so a poisoned entry degrades recall
  but cannot corrupt the durable record — and the raw file stays on disk for a human to
  inspect.

## Deferred

- **Embedding-based dimension recall** — a vector `SemanticStore` per dimension; the
  keyword recall is the first cut (matches the shipped semantic layer).
- **Learned taxonomy** — mine the recorded `agent_dimension_summaries` to promote or
  retire dimensions from data rather than the seed list.
- **Dimension → mode-recall weights learned** from outcomes, rather than the fixed
  table above.
