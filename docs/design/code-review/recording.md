# 09 — Grounded context & recording

Status: **implemented** (increment 10) — destinations (a) and (c) ship; the durable
git-state memories (b) stay a deferred follow-up. See **Implementation** below.

## Implementation (increment 10)

The pipeline's end. Two of the three destinations ship, mirroring the verifier's
recording exactly:

- **(a) Grounded context + stated gaps.** `render_facts` already injects the
  compact, sectioned context (hard facts first, soft summaries last, labelled). This
  increment adds a **`Not established`** section: any collector that `Skipped`/
  `Failed` is listed with its reason, so the reviewer never mistakes an absent fact
  for a clean one.
- **(c) ClickHouse recording.** A flattened **`ReviewRecord`** (hashes/counts/
  durations only — never source, contents, URLs, or summaries) is derived from
  `ReviewFacts` via `ReviewRecord::from_facts(&facts, mode_via)` and recorded through
  the **exact verifier pipeline**: a `review: Option<ReviewRecord>` side-channel on
  `MemoryEvent` (telemetry-local, dropped at the gRPC boundary), `Agent::record_review`
  → `CompositeMemory` mirror → `TelemetryHandle` routes `kind == "review"` →
  `Msg::Review`/`Msg::ReviewCollector` → the batched writer → **`agent_reviews`** (one
  headline row) + **`agent_review_collectors`** (one row per collector, the
  parallelism drill-down). With telemetry off the record still lands in
  `.agent/episodic.jsonl` — how the gate + e2e assert it. Metrics:
  `agent_review_runs_total{project,mode_via,outcome}`,
  `agent_review_total_duration_seconds{project}`, `agent_review_parallelism_ratio`
  (Σ collector work ÷ total wall-clock — the parallelism payoff). Appended at both
  `collect()` sites: `agent --review` (`mode_via = explicit`) and the in-loop handoff
  (`auto`).
- **Verification:** telemetry-row unit tests (`ReviewRow`/`ReviewCollectorRow`
  from_event) + a render-gaps test + the hermetic `nix/checks/review-recording.nix`
  (runs the real binary and asserts the `ReviewRecord` in `episodic.jsonl`, with only
  hashes/counts, no URL). Live-proven end-to-end.

**Deferred:** (b) **durable git-state memories** via `SemanticStore` (needs the
semantic store threaded to the review call site + injection-scan — a clean
follow-up); per-collector `items` counts beyond the well-known collectors (not
carried on `CollectorStatus`); the `agent_review_records_dropped_total` backpressure
counter; and the outcome-proxy columns (`revised_after`-style) the table is shaped
to add later.

---

Where the pipeline ends: turn `ReviewFacts` into (a) the **grounded context** the
reviewer reasons over, (b) durable **memories** for the facts that persist across
reviews, and (c) a **ClickHouse row** so reviews are measurable offline. All three
reuse plumbing that already exists — no new observability framework.

## The three destinations

### (a) Injected grounded context

`ReviewFacts` is rendered into a compact, clearly-sectioned context block and
injected via the existing context assembly (the same path `context.d/` and
recalled memory use), labelled **"grounded review facts (tool-derived)"**. The
rendering rules encode the grounded-first principle:

- **Hard facts first, verbatim from tools**: the change set, git state,
  analyzer findings (foregrounding `in_diff`), the call-graph blast radius, the
  style fingerprint. These are stated as facts.
- **Soft summaries last, labelled**: the before/after function summaries, each
  tagged with its producing model, explicitly marked *derived*.
- **Gaps are stated**: any collector that `Skipped`/`Failed` appears as an explicit
  "not established: {reason}", so the reviewer never mistakes absence for
  cleanliness.

The reviewer — a human reading it, or the main loop continuing the turn — now has
a fact base it cannot hallucinate over.

### (b) Durable memories

The **stable** facts from `GitState` (remote host, fork-vs-clone, default branch,
project language) are the kind of thing worth remembering across sessions — they
match the agent's semantic-memory model ("what is true" about this repo). They're
written as memory facts through the existing `SemanticStore`, so a later review of
the same repo starts already knowing them. Volatile facts (a specific diff) are
**not** memorized — they belong to the run, not the repo.

### (c) ClickHouse recording (measurement)

Every review emits records to ClickHouse through the **exact pipeline the verifier
uses** — no new mechanism:

```
ReviewFacts ─▶ MemoryEvent { kind: "review", review: Some(ReviewRecord) }
            ─▶ CompositeMemory.append (mirror, non-blocking) ─▶ inner durable store
                         │
                         └▶ TelemetryHandle.record_event  (routes by kind)
                                     └▶ Msg::Review ─▶ batched writer ─▶ agent_reviews
```

Reusing:
- the `MemoryEvent` **side-channel** field pattern (`usage`, `verification` today;
  add `review: Option<ReviewRecord>`), telemetry-local, dropped at the gRPC memory
  boundary;
- `CompositeMemory` mirroring append → telemetry;
- the `rows.rs` / `writer.rs` / `lib.rs` **route-by-`kind`** pattern (add a
  `ReviewRow` + a `Msg::Review` arm + a `kind == "review"` route);
- `fnv1a_hex` for hashing untrusted text, `clamped_confidence` for soft numbers.

With telemetry **off**, the record still lands in `.agent/episodic.jsonl` (how the
e2e tests will assert it, exactly as the verifier's do).

## Two tables

```sql
-- one row per review run (the headline: durations, sizes, mode)
CREATE TABLE IF NOT EXISTS agent.agent_reviews (
    session_id     String,
    ts             DateTime64(3, 'UTC'),
    repo_hash      String,            -- fnv1a, not the URL
    base_rev       String,
    head_rev       String,
    mode_via       String,            -- prefilter | vote | explicit
    project        String,            -- go | rust | mixed | unknown
    is_fork        UInt8,
    changed_files  UInt32,
    findings       UInt32,
    findings_in_diff UInt32,
    summaries      UInt32,
    total_ms       UInt32,            -- whole fan-out wall-clock
    sum_work_ms    UInt32,            -- Σ per-collector durations (parallelism ratio)
    critical_path  String            -- name of the slowest collector
) ENGINE = MergeTree ORDER BY (session_id, ts);

-- one row per collector per review (the parallelism / optimization detail)
CREATE TABLE IF NOT EXISTS agent.agent_review_collectors (
    session_id  String,
    ts          DateTime64(3, 'UTC'),
    collector   String,
    status      String,               -- ok | partial | skipped | failed
    duration_ms UInt32,
    items       UInt32                 -- findings / nodes / summaries produced
) ENGINE = MergeTree ORDER BY (session_id, ts, collector);
```

`total_ms` alongside `sum_work_ms` is deliberate: their ratio is the **parallelism
payoff** (how much the fan-out saved over running serially), and `critical_path`
names the collector to optimize next. The per-collector table is the drill-down.
This is what makes "account for durations and parallel-optimization opportunities"
answerable with a SQL query rather than a guess — see [`11`](observability.md) for
the queries.

## Failure semantic

**Best-effort telemetry** (like all recording here): a dropped row never affects
the review. The durable episodic log is the fallback. Memory writes are similarly
non-fatal.

## Protobuf

`ReviewRecord` is **telemetry-local** — it rides `MemoryEvent` and is dropped at
the gRPC memory boundary (the `verification` precedent), so it needs no service of
its own. Its shape mirrors the `agent_reviews` row; hashes not raw text.

```proto
message ReviewRecord {
  string repo_hash   = 1;
  string base_rev    = 2;
  string head_rev    = 3;
  string mode_via    = 4;
  RepoLanguage project = 5;
  bool   is_fork     = 6;
  uint32 changed_files = 7;
  uint32 findings    = 8;
  uint32 findings_in_diff = 9;
  uint32 summaries   = 10;
  uint32 total_ms    = 11;
  uint32 sum_work_ms = 12;
  string critical_path = 13;
  repeated CollectorStatus collectors = 14;   // reused from 03
}
```

## Prometheus metrics

| Metric | Type | Labels |
|---|---|---|
| `agent_review_runs_total` | counter | `project`, `mode_via`, `outcome` |
| `agent_review_total_duration_seconds` | histogram | `project` |
| `agent_review_parallelism_ratio` | histogram | — (`sum_work_ms` / `total_ms`) |
| `agent_review_records_dropped_total` | counter | — (telemetry backpressure) |

## Tracing + logs

- The record is emitted at the close of the root `review.collect` span; the span
  and the row share `total_ms`/`critical_path` so a trace and a ClickHouse row
  reconcile.
- Logs: `INFO` a final one-liner ("review recorded: {changed_files} files,
  {findings} findings, {total_ms}ms, critical={critical_path}"). Hashes/counts only.

## Security

- Only hashes, revs, counts, and durations are recorded — **never** raw source,
  file contents, remote URLs, or the model summaries. The analytics table cannot
  leak repo content.
- Memory writes go through the normal `SemanticStore` (which is itself
  injection-scanned); the git-state facts written are typed values, not free text.
- Counts/durations are clamped before any `inc_by` / total (hostile-number rule).
- `adversarial_` cases: a `ReviewRecord` with `total_ms = u32::MAX` or negative-
  turned-huge counts must clamp, not panic the Prometheus path.

## Deferred

- **Findings → posted review** (the `Forge` write path) — out of scope by decision.
- **Outcome proxies** for reviews (did the grounded context measurably improve the
  review?), mirroring the verifier's `revised_after`/`task_succeeded` columns —
  the table is shaped to add them later.
- **Cross-repo trust weighting** of summary models from `agent_review_collectors` —
  the same offline-weighting idea as the verifier design, enabled by this data.
