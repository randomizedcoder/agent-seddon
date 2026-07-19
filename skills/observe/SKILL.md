---
name: observe
description: Inspect this agent's own performance — metrics, search index state, and traces
---
# Observing your own performance

You are an instrumented agent. Use these to see what happened, how long it took,
and whether the search index is healthy. See docs/observability.md for the full map.

## 1. Live metrics (always available, in-process)

Call the `metrics` tool — no external services needed:

- `metrics {"filter": "search"}` — index freshness (`agent_search_index_fresh`, 1 = up
  to date), file count, and reindex/query timings. For a histogram like
  `agent_search_query_seconds`, read `_count` and `_sum`: **average seconds = sum / count**.
- `metrics {"filter": "tool"}` — per-tool call counts and exec latency.
- `metrics {"filter": "provider"}` — model call latency, TTFT, tokens.
- `metrics {}` — everything; `metrics {"raw": true}` adds histogram buckets.

## 2. Rates & p95 over time (needs Prometheus up: `nix run .#prometheus-up`)

Via `bash` + the Prometheus HTTP API (:9090):

```sh
curl -s 'http://127.0.0.1:9090/api/v1/query' \
  --data-urlencode 'query=histogram_quantile(0.95, sum(rate(agent_search_query_seconds_bucket[5m])) by (le,backend,mode))'
```

Or open the Grafana dashboard (`nix run .#grafana-up`, UI :3000 → **agent-seddon**);
the **Search** row shows index freshness/files, reindex duration+rate, and query
latency/rate/hits by backend+mode.

## 3. Traces / span durations (needs ClickStack up: `nix run .#clickstack-up`)

```sh
nix run .#clickstack-client -- -q "SELECT SpanName, count() n, round(avg(Duration)/1e6,1) avg_ms \
  FROM default.otel_traces GROUP BY SpanName ORDER BY n DESC FORMAT PrettyCompact"
```

## Reading the search signals

- `agent_search_index_fresh{backend} == 0` → the index is stale/missing; a
  background reindex should be running (check `agent_search_reindex_total` and
  `agent_search_index_seconds` for how long it took).
- Slow queries → compare `agent_search_query_seconds` average across `mode`
  (literal/phrase/fuzzy/regex) and `backend`.
