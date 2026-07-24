# Dimensional memory — the `DimensionStore` seam

A cheap per-step pass (adaptive-cognition 03) reviews the recent transcript and
files "what just happened" into **per-dimension** histories, so a later
[mode switch](mode.md) can pull the right axis back in as fresh context — the
"pull in fresh" column of [mode-aware compaction](context.md)'s before/after table.

- **Trait:** `agent_core::DimensionStore` (`summarize_step` + `recall_dimension`)
- **Impl crate:** [`agent-memory`](../../crates/agent-memory) (`FileDimensions`)
- **Cargo features:** `agent-memory/memory-dimensions`, runtime `dimensions`
- **Config:** `[dimensions] store` (`""`/`off` | `file` | `grpc`) — **off by default**
- **Service:** `agent --serve-dimension` (`agent.v1.DimensionService`)

## The pass

On each turn (after it produces an answer), the runtime renders the recent working
tail and makes **one cheap structured-output call** returning a *set* of
`{dimension, summary, is_new}`. A step may touch several axes ("coding *and* git")
— each files into its own `<semantic_dir>/dimensions/<dim>.md`. Off by default
because, unlike mode detection's free prefilter, this is a real (local-tier) LLM
call per turn — opt in like `[memory] distill`.

## Hybrid taxonomy: seeded + emergent

- **Seed set** (closed, always valid): `coding · git · user · project · testing ·
  tooling · docs`.
- **Emergent**: the model may propose a new lowercase slug (`is_new`). It is
  admitted only after it **recurs `K` times** *and* while the total dimension count
  is under `MAX_DIMENSIONS`; until then its summaries file under `misc`. Growth is
  bounded three ways — `MAX_DIMS_PER_STEP`, the `K`-recurrence gate, and
  `MAX_DIMENSIONS`.

When a dimension file grows past its size cap, a **synthesis pass** re-summarizes it
into a tighter form (fail-soft: on error it keeps the recent tail).

## Dimension-weighted recall (the bridge to compaction)

On a mode switch, the runtime recalls the destination mode's dimensions and injects
them as one system block — what [mode-aware compaction](context.md) sheds from the
working set was already flushed here, so recalling it when its mode comes around is
what makes the shed **safe**:

| Entering mode | Recalled dimensions |
|---|---|
| Implement / Debug | coding, testing |
| Review | coding, git, project |
| Design | project, docs |
| Explain | user |

## Security

Step content is **untrusted** tool/model output. Every emergent slug passes a
fail-closed `safe_segment` check before it is a path component (blocking
`../../etc/passwd`-style traversal); every summary is **bounded** and
`scan_for_injection`-screened before it is persisted, because a dimension file is
recalled verbatim into future context. The hard episodic log is never rewritten —
dimensional histories are a derived, curated layer, so a poisoned entry degrades
recall but cannot corrupt the durable record, and the raw file stays on disk for a
human to inspect.

## Observability

- Metrics: `agent_dimension_summaries_total{dimension,is_new}`,
  `agent_dimension_summarize_duration_seconds`, `agent_dimension_recall_total{dimension}`.
- Spans: `memory.dimension.summarize`, `memory.dimension.recall`,
  `memory.dimension.synthesize` (counts/lengths only — never summary bodies).
- ClickHouse: one `agent_dimension_summaries` row per accepted summary (dimension,
  `is_new`, `summary_len` — never the text), via a `kind = "dimension"` `MemoryEvent`.

Design: [adaptive-cognition/03-memory.md](../design/adaptive-cognition/03-memory.md).
