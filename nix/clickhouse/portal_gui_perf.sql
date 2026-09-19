-- nix/clickhouse/portal_gui_perf.sql
--
-- Canned cross-PR queries for the portal GUI perf table (portal-gui-testing 06 /
-- inc 09). Run against the AGENT ClickHouse (the one holding `agent.portal_gui_perf`):
--
--   nix run .#clickhouse-client -- -q "$(sed -n '/^-- Q1/,/;/p' nix/clickhouse/portal_gui_perf.sql)"
--
-- or paste into http://localhost:8123/play. Substitute a real run id for
-- {run_id:String} (the value printed as `run_id=…` by `nix run .#portal-e2e`), e.g.
--   nix run .#clickhouse-client -- --param_run_id 12345 -q "<Q1>"
--
-- These are a TREND record, never a gate: wall-clock GUI latency is too noisy to fail
-- CI on — deterministic micro-perf stays the iai-callgrind Ir ceilings. `host` is
-- advisory; never compare value_ms across hosts.

-- Q1 — this run's p50/p95 per (page, metric), passing samples only.
SELECT page,
       metric,
       count()                          AS samples,
       round(quantile(0.50)(value_ms), 2) AS p50_ms,
       round(quantile(0.95)(value_ms), 2) AS p95_ms
FROM agent.portal_gui_perf
WHERE run_id = {run_id:String}
  AND outcome = 'pass'
GROUP BY page, metric
ORDER BY page, metric;

-- Q2 — this run's slowest gRPC methods (client-perceived), with a trace to open.
-- Swap metric to 'grpc_server_ms' for the server-side truth. The returned `trace`
-- is the key into ClickStack's default.otel_traces (see Q3).
SELECT rpc_method,
       round(max(value_ms), 2)          AS worst_ms,
       argMax(trace_id, value_ms)       AS trace
FROM agent.portal_gui_perf
WHERE run_id = {run_id:String}
  AND metric = 'grpc_server_ms'
  AND rpc_method != ''
GROUP BY rpc_method
ORDER BY worst_ms DESC
LIMIT 20;

-- Q3 — the full cross-hop trace for a slow sample. The perf rows live in the AGENT
-- ClickHouse; the spans live in ClickStack's SEPARATE, bundled ClickHouse (rootless
-- podman gives them no shared network, so this is a two-step lookup keyed by
-- trace_id, not a single-server JOIN). Take the `trace` from Q2 and run, against the
-- CLICKSTACK container:
--   podman exec agent-seddon-clickstack clickhouse-client -q "
--     SELECT Timestamp, ServiceName, SpanName, SpanAttributes['rpc'] AS rpc,
--            round(Duration/1e6, 3) AS ms
--     FROM default.otel_traces
--     WHERE TraceId = '<trace-from-Q2>'
--     ORDER BY Timestamp"
-- The envoy-portal-bridge and agent-gateway spans share the TraceId (inc 10), so this
-- shows the browser -> envoy -> gateway hop for that one slow call.

-- Q4 — cross-PR regression view: this branch's p95 per (page, metric) vs main's
-- rolling median over the last 30 days. A large positive delta_ms is the signal.
SELECT cur.page                               AS page,
       cur.metric                             AS metric,
       cur.p95_ms                             AS p95_this,
       base.med_ms                            AS median_main,
       round(cur.p95_ms - base.med_ms, 2)     AS delta_ms
FROM
(
    SELECT page, metric, round(quantile(0.95)(value_ms), 2) AS p95_ms
    FROM agent.portal_gui_perf
    WHERE run_id = {run_id:String} AND outcome = 'pass'
    GROUP BY page, metric
) AS cur
LEFT JOIN
(
    SELECT page, metric, round(median(value_ms), 2) AS med_ms
    FROM agent.portal_gui_perf
    WHERE branch = 'main' AND outcome = 'pass'
      AND ts > now() - INTERVAL 30 DAY
    GROUP BY page, metric
) AS base
USING (page, metric)
ORDER BY delta_ms DESC;
