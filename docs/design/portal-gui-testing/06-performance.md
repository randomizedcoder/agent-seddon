# 06 — Performance measurement & tracking (per-PR, in ClickHouse)

Every test doubles as a **timing sample**. Pass/fail catches breakage; timings catch the
*silent slowdown* — a page that got heavier, an RPC that regressed — which a boolean gate
never sees. Both layers record durations and **stream them into a dedicated ClickHouse
table** so any PR's performance is comparable to `main` (and to prior PRs) with plain
SQL, and any slow sample links to its full trace.

## What is measured

- **UI render / interaction time** (both layers): per page, time to first settle
  (`pump`→`pumpAndSettle`) = `page_load_ms`; per action, gesture→settled/expected-state
  = `interaction_ms`. In Layer A this catches UI-side regressions (an expensive build or
  layout) fairly deterministically; in Layer B it is the real perceived latency.
- **gRPC response time** (Layer B, live): `grpc_client_ms` (client-perceived
  round-trip); `grpc_upstream_ms` from the `x-envoy-upstream-service-time` header
  (backend-only ms — so `client − upstream` = proxy+network); and the **server-side
  truth** already emitted, the `agent_grpc_server_rpc_seconds{rpc}` histogram
  (`crates/agent-metrics/src/lib.rs:964-971`) + OTLP span durations. Layer A's fake is
  instant, so it records UI timings only.
- Each measured action runs **N iterations with warm-up excluded**; store per-iteration
  rows plus a p50/p95 summary — wall-clock is noisy and single samples lie.

## The table

A wide MergeTree, one row per (sample, metric), designed for cross-PR SQL. Covers the
requested columns (timestamp, test name, phase, step, PR number) plus the git / host /
layer / rpc dimensions that make the SQL useful:

```sql
CREATE TABLE portal_gui_perf (
  ts            DateTime64(3) DEFAULT now64(3),
  run_id        String,                   -- one suite invocation (shared with the report)
  pr_number     UInt32,                   -- 0 when run off a branch / locally
  commit_sha    String,
  branch        String,
  git_dirty     UInt8,
  host          LowCardinality(String),   -- never compare across hosts
  layer         LowCardinality(String),   -- 'unit'|'widget'|'golden'|'e2e'
  page          LowCardinality(String),
  element_id    LowCardinality(String),
  test_name     String,
  phase         LowCardinality(String),   -- 'load'|'action'|'settle'|'rpc'|'teardown'
  step          String,                   -- free-form sub-step label
  rpc_method    LowCardinality(String),   -- '' for pure-UI samples
  metric        LowCardinality(String),   -- 'page_load_ms'|'interaction_ms'|'grpc_client_ms'|'grpc_upstream_ms'|'grpc_server_ms'
  value_ms      Float64,
  iteration     UInt16,
  outcome       LowCardinality(String),   -- 'pass'|'fail'|'skip'
  trace_id      String                    -- Layer B: link to the ClickHouse trace
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(ts)
ORDER BY (page, test_name, metric, ts);
```

## Ingestion

Batch a run's rows and POST once to ClickHouse `INSERT … FORMAT JSONEachRow` over HTTP,
reusing the ClickHouse endpoint/creds the ClickStack sink already uses
([`docs/deployment-l2.md`](../../deployment-l2.md), `crates/agent-telemetry/src/otel.rs`). `pr_number` /
`commit_sha` / `branch` / `git_dirty` come from git + a `PORTAL_PR_NUMBER` env set in CI;
local runs land with `pr_number=0`. **The insert is best-effort and never fails the test
run** — perf recording is observability, not a correctness gate.

## Using it

Ship canned SQL and a Grafana panel (Grafana already runs on l2). Example — a PR's p95
per `(page, metric)` vs `main`'s rolling median:

```sql
SELECT page, metric,
       quantile(0.95)(value_ms)            AS p95_this_pr
FROM portal_gui_perf
WHERE run_id = {run_id:String} AND outcome = 'pass'
GROUP BY page, metric
ORDER BY page, metric;
-- join against a windowed median over branch='main' to flag deltas over a threshold
```

Example — the slowest gRPC methods this run, with a trace to open:

```sql
SELECT rpc_method, max(value_ms) AS worst_ms, argMax(trace_id, value_ms) AS trace
FROM portal_gui_perf
WHERE run_id = {run_id:String} AND metric = 'grpc_client_ms'
GROUP BY rpc_method ORDER BY worst_ms DESC LIMIT 20;
```

## Discipline: trend-tracking, not a hard gate

A regression surfaces as a **query/dashboard signal** — optionally a generously
thresholded, non-blocking PR annotation — **not** a red build. Wall-clock GUI latency is
too noisy to fail CI on. Deterministic gated micro-perf stays the
[iai-callgrind](../../components/benchmarking.md) Ir ceilings; this table is the
complementary, cross-PR *trend* record for the GUI path.

The proxy-vs-backend split (`grpc_client_ms` / `grpc_upstream_ms` / `grpc_server_ms`)
plus the Envoy access-log latencies ([`07`](07-envoy-otel.md)) let a SQL query localize
any slowdown to the UI, the proxy hop, or the backend.
