# 11 — Deep instrumentation

Status: **design / pre-implementation.**

The review flow is a parallel fan-out of services, and the whole point of the
project is that such a thing is **legible**: for every stage you can name the
metric it emitted, the span it opened, and the duration it took — and from those,
find the **critical path** and the **parallel-optimization opportunities**. This
doc consolidates the per-component instrumentation (each doc states its own) and
specifies the duration-accounting design that makes optimization a query, not a
guess.

No new framework: it plugs into the three signals agent-seddon already has —
Prometheus metrics via the `metered.rs` decorators + typed-event callbacks,
OpenTelemetry spans with W3C propagation across every gRPC hop, and ClickHouse
recording via the `MemoryEvent`/`CompositeMemory` mirror.

## The metric families (all new, namespaced)

`agent_pool_*` for the pool, `agent_review_*` for the flow. Durations are
**histograms** (so p50/p95 are available, not just means); outcomes are
**counters** with an `outcome`/`status` label; liveness is a **gauge**.

| Family | Type | From |
|---|---|---|
| `agent_pool_members_alive` | gauge | 01 |
| `agent_pool_probe_duration_seconds` / `agent_pool_probes_total` | hist / ctr | 01 |
| `agent_pool_dispatch_duration_seconds` / `agent_pool_member_calls_total` | hist / ctr | 01 |
| `agent_review_mode_decisions_total` / `agent_review_mode_duration_seconds` | ctr / hist | 02 |
| `agent_review_collect_duration_seconds` | hist | 03 (whole fan-out) |
| `agent_review_collector_duration_seconds` / `agent_review_collectors_total` | hist / ctr | 03 (per collector) |
| `agent_review_change_duration_seconds` / `agent_review_change_files` | hist | 04 |
| `agent_review_gitstate_total` | ctr | 04 |
| `agent_review_analyze_duration_seconds` / `agent_review_analyze_tool_duration_seconds` | hist | 05 |
| `agent_review_findings_total` / `agent_review_analyze_tool_runs_total` | ctr | 05 |
| `agent_review_ast_duration_seconds` / `agent_review_ast_nodes` / `_edges` | hist | 06 |
| `agent_review_style_duration_seconds` / `agent_review_style_diff_conformance_total` | hist / ctr | 07 |
| `agent_review_summarize_duration_seconds` / `agent_review_summarize_job_duration_seconds` | hist | 08 |
| `agent_review_summaries_total` | ctr | 08 |
| `agent_review_runs_total` / `agent_review_total_duration_seconds` | ctr / hist | 09 |
| `agent_review_parallelism_ratio` | hist | 09 |
| `agent_review_records_dropped_total` | ctr | 09 |

**Emission pattern (reused).** The impl crates (`agent-providers`,
`agent-analyzer`, …) must **not** depend on `agent-metrics` — they raise typed
events (the `RouteEvent`/`PoolEvent` precedent) that the runtime converts in
`metered.rs`. This keeps the dependency graph acyclic, exactly as the router does
today. Each served seam also exposes its own `/metrics` on its `metrics_port`, so a
remote analyzer's histograms are scrapeable where it runs.

## The span tree (what a single review looks like in a trace)

```
review.collect                       (root; total_ms)
├─ review.detect                     (via, mode, confidence)     [may precede collect]
│  └─ pool.dispatch (mode vote)      → pool.member × N
├─ review.change                     (source, n_files)           ← barrier: runs first
├─ review.analyze                    (language, tier)
│  ├─ analyze.tool golangci-lint     (duration_ms, finding_count)
│  ├─ analyze.tool gosec             (duration_ms, finding_count)
│  └─ analyze.tool go-vet            (duration_ms, finding_count)
├─ review.ast                        (nodes, edges)
├─ review.style                      (comment_density, …)
└─ review.summarize                  (produced/requested)
   └─ pool.dispatch → pool.member × M
```

W3C context propagates across each gRPC hop (the existing `server::span` parents on
the caller's context, `client::outbound` injects it), so a remote `--serve-analyzer`
still shows up as a child of `review.collect` in one trace — the two-process
distributed-trace property the project already demonstrates. The **sibling spans
under `review.collect` are the parallelism**: their start/stop overlap *is* the
concurrency, visible directly in ClickStack/Jaeger.

## Duration & parallel-optimization accounting (the headline)

This is what "account for durations and parallel optimization opportunities" means
concretely. Three numbers, captured every run (see the `agent_reviews` table in
[`09`](recording.md)):

- **`total_ms`** — wall-clock of the whole fan-out (the root span).
- **`sum_work_ms`** — Σ of every collector's `duration_ms` (the work if it ran
  serially).
- **`critical_path`** — the collector whose `duration_ms` is largest.

From them:

**Parallelism ratio** `= sum_work_ms / total_ms`. A ratio near the collector count
means the fan-out is paying off (work is overlapping); a ratio near **1** means one
collector dominates and the concurrency buys nothing — go optimize (or further
split) `critical_path`.

**Where more concurrency helps.** If `critical_path` is `review.analyze` and *its*
child `analyze.tool` spans are themselves serial, the win is intra-analyzer
parallelism (already designed: the linters run concurrently — this measures whether
they actually overlap). If `critical_path` is `review.summarize`, the win is a
wider pool `fanout` or a lighter tier. The per-collector table
(`agent_review_collectors`) is the drill-down.

### The queries (measurement, not guesswork)

```sql
-- Which collector is the critical path, how often, and how slow (p95)?
SELECT critical_path, count() AS runs,
       quantile(0.95)(total_ms) AS p95_total
FROM agent.agent_reviews
GROUP BY critical_path ORDER BY runs DESC;

-- Parallelism payoff over time — is the fan-out actually saving wall-clock?
SELECT toStartOfHour(ts) AS h,
       avg(sum_work_ms)  AS serial_ms,
       avg(total_ms)     AS wallclock_ms,
       avg(sum_work_ms / total_ms) AS parallelism_ratio
FROM agent.agent_reviews GROUP BY h ORDER BY h;

-- Per-collector p50/p95 — which stage to optimize, and its variance.
SELECT collector,
       quantile(0.50)(duration_ms) AS p50,
       quantile(0.95)(duration_ms) AS p95,
       countIf(status = 'failed')  AS failures
FROM agent.agent_review_collectors GROUP BY collector ORDER BY p95 DESC;
```

The same numbers are on the Prometheus side (`agent_review_total_duration_seconds`,
`agent_review_collector_duration_seconds`, `agent_review_parallelism_ratio`) for a
live Grafana panel — provisioned from the same `nix/constants.nix` source as the
existing dashboard, so it can't drift.

## Logging discipline

Every component logs **ids, hashes, counts, and durations only** — never tokens,
endpoint URLs, keys, raw source, findings' raw output, or model summaries. Levels:
`INFO` for stage completion and mode transitions, `WARN` for a failed/timed-out
collector or a degraded pool, `DEBUG` for per-item detail. This is the same
token/URL-hygiene rule the `Forge` and provider layers already enforce.

## Security

- Metrics labels are **bounded, closed sets** (collector names, tool names, tiers,
  outcomes) — never a model-supplied or repo-supplied string, so cardinality can't
  be blown up by hostile input.
- All counts/durations feeding an `inc_by`/observe are **clamped** (the
  hostile-number rule) — a `duration_ms` of `u32::MAX` or a NaN ratio must not
  panic the Prometheus path.
- Spans and logs carry hashes, not raw content; a remote service's trace context is
  validated as W3C, not trusted blindly.
- `adversarial_` cases (mandated per component): a collector reporting a hostile
  `duration_ms`, a finding count of `u32::MAX`, a label-injection attempt via a
  crafted tool name — all clamped/rejected before they reach a metric or a row.

## Deferred

- A **provisioned Grafana panel** for the review flow (the metrics are specified;
  the dashboard JSON is generated from constants at implementation time).
- **Outcome-correlated timing** (did a slower, richer collection produce better
  reviews?) — needs the review outcome proxies that `09` defers.
