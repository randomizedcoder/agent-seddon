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

## As built (inc 09)

Folded into the opt-in `nix run .#portal-e2e` app (inc 07), not on the gate.

- **Table.** `agent.portal_gui_perf` (the design DDL, prefixed `agent.` to match every
  other table) added to [`nix/clickhouse/schema.sql`](../../../nix/clickhouse/schema.sql).
  The `agent doctor` ClickHouse drift check parses `schema.sql`, so it **auto-extends** —
  an un-migrated volume reports the table missing until `nix run .#clickhouse-migrate`.
- **Two ClickHouse servers, one key.** The perf rows land in the **agent** ClickHouse
  (host `:8123`, database `agent`); the gateway/Envoy **spans** live in ClickStack's
  **separate** bundled ClickHouse (`default.otel_traces`), whose native port is not
  host-published. Under rootless podman the two have no shared network, so `trace_id` is
  a **portable key link, not a cross-server JOIN** — the "trace of the slowest" query
  (Q3 in [`portal_gui_perf.sql`](../../../nix/clickhouse/portal_gui_perf.sql)) is a
  two-step lookup: get the `trace_id` from the agent CH, resolve it in ClickStack's CH.
- **What's measured, live.** Per driven action: `interaction_ms` (client-perceived, from
  the driver's own timing) and `grpc_server_ms` (**server truth** from the `:9700`
  `agent_grpc_server_rpc_seconds` histogram delta — note its `rpc` label is the **full**
  method path with **no** `outcome` label, unlike the `_total` counter the assertions
  use). `grpc_client_ms` / `grpc_upstream_ms` (the `x-envoy-upstream-service-time` header)
  are not captured from the headless browser and are left for a later pass.
- **trace_id capture.** Looked up in `otel_traces` by the gateway span's **short** op
  name (`SpanAttributes['rpc']`, e.g. `registry.put` / `registry.enable` /
  `prompt.set_active_personality` — the `info_span!("grpc.server", rpc, …)` in
  `crates/agent-grpc/src/server/*`), newest within the run window, so a captured id is
  guaranteed to resolve in `otel_traces`. Best-effort: no ClickStack / export lag /
  tracing-off yields `''` (like inc 07's span check).
- **Ingestion & safety.** One `INSERT … FORMAT JSONEachRow` POST to `:8123`, built with
  `jq` (data, never SQL). Untrusted inputs are fail-closed: histogram fields accept only a
  clean non-negative decimal, `value_ms` is regex-validated before the row is emitted, a
  server-supplied `trace_id` is accepted only if hex, and `pr_number` collapses to `0`
  unless all-digits. The insert is **best-effort** — a down CH or missing table is a warn,
  never a contract failure. `iteration=1` for the single live drive (N-iteration warm-up
  is future work; the p50/p95 SQL is ready for when it lands).
- **Grafana.** A `grafana-clickhouse-datasource` datasource
  ([`clickhouse.yml`](../../../nix/grafana/provisioning/datasources/clickhouse.yml)) +
  a `portal-gui-perf` dashboard
  ([`portal-gui-perf.json`](../../../nix/grafana/dashboards/portal-gui-perf.json));
  `grafana-up` installs the plugin via `GF_INSTALL_PLUGINS`, so a pre-existing grafana
  container must be recreated (`grafana-down && grafana-up`) to pick it up.
- **Deferred:** the `envoy_access_latency` materialized view (07) — the Envoy access-log
  latency columns are already flat in `otel_logs`, so it is a convenience view, not a
  blocker; left for a focused follow-up.
