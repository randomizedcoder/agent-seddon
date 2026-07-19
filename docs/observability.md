# Observability

Observability is a first-class feature of this harness: every seam is
instrumented, so you (and the agent itself) can see **what happened, how long it
took, and why**. There are three signals, each with a local stack you bring up
with one `nix run` command:

| Signal | What it answers | Emitted by | Stack | UI |
|--------|-----------------|------------|-------|----|
| **Metrics** | rates, counts, latencies (p50/p95), gauges | `agent-metrics` registry + `metered.rs` decorators | Prometheus → Grafana | Grafana :3000 |
| **Traces** | the causal span tree of one run, across process hops | `agent-telemetry` OTLP export | ClickStack (HyperDX) | HyperDX :8080 |
| **Logs / history** | structured turn history, usage, tool events | `agent-telemetry` ClickHouse sink | ClickHouse | `clickhouse-client` |

Full runbooks: **[metrics.md](metrics.md)** (Prometheus + Grafana) and
**[tracing.md](tracing.md)** (OTLP + HyperDX). This page is the map that ties them
together and shows how the **agent can inspect its own performance**.

## Quick start — the whole stack

```sh
# Metrics (enabled by default; served on 127.0.0.1:9600):
nix run .#prometheus-up      # Prometheus  UI :9090, scrapes :9600 + per-seam :9601–9606
nix run .#grafana-up         # Grafana     UI :3000, provisioned "agent-seddon" dashboard

# Traces (opt-in — set [telemetry] otlp_endpoint = "http://127.0.0.1:4317"):
nix run .#clickstack-up      # HyperDX UI :8080, OTLP :4317 (traces) + bundled ClickHouse

# Run the agent so signals flow:
nix run .#agent -- --config config/agent.toml
```

Tear down with the matching `*-down` apps (`prometheus-down`, `grafana-down`,
`clickstack-down`).

## Metrics → Grafana dashboard

The agent records into one shared Prometheus registry (cheap, always-on; only
*serving* `/metrics` is gated by `[metrics] enabled`, default true). Grafana
auto-provisions the **agent-seddon** dashboard, one row per component:

- **Overview** — runs/sec by outcome, run duration p50/p95, iterations, active runs.
- **Provider** — API calls, call latency, request/TTFT p95 by provider, tokens/sec, errors.
- **Tools** — calls/sec and exec p95 **by tool** (so `search` vs `bash` vs `edit`), errors.
- **Memory / Context / Policy** — op latencies, recall size, compactions, authorize decisions.
- **Search** — index freshness (`1 = up to date`) and file count, **reindex
  duration + rate** (see *when* indexing runs and *how long* it takes), and
  **query latency / rate / hits by `backend` + `mode`** (so tantivy vs. a second
  backend compare side by side).

The dashboard's `$job` selector switches between the in-process agent (`job="agent"`)
and any `agent --serve-<seam>` process, which each expose their own `/metrics`
(server-side latency) on a dedicated port — provider 9601 … **search 9606** (from
`nix/constants.nix`). See [metrics.md](metrics.md) for the full metric catalogue.

## Traces → HyperDX

With `[telemetry] otlp_endpoint` set, each run is a span tree exported over
OTLP/gRPC to ClickStack:

```
agent.turn
├─ memory.recall · context.assemble
├─ provider.stream ──▶ grpc.server        (folds in a remote `= "grpc"` seam)
├─ policy.authorize
├─ tool.execute            (e.g. the `search` tool ──▶ search.query when remote)
└─ context.compact
```

W3C trace context rides in gRPC metadata, so a request that crosses into a
`--serve-<seam>` process stays **one trace** spanning both services. Open HyperDX
at <http://localhost:8080>, or query the spans directly:

```sh
nix run .#clickstack-client -- -q "SELECT ServiceName, SpanName, count() n \
  FROM default.otel_traces GROUP BY ServiceName, SpanName ORDER BY 1,2 FORMAT PrettyCompact"
```

See [tracing.md](tracing.md) for setup (ingestion key, gotchas) and more queries.

## The agent observing itself

The agent can watch its own performance without any of the stack running — the
`metrics` tool reads the **in-process** registry (the exact series Prometheus
scrapes):

- **`metrics` tool** (built-in, enabled by default) — the model calls it like any
  tool: `{"filter": "search"}` returns the live search series (index freshness,
  reindex/query `_count` + `_sum` → average = sum/count), `{"filter": "tool"}` the
  per-tool exec times, and no filter dumps everything. Because it's advertised in
  the tool list, the agent *knows* it can self-inspect. Implementation:
  [`agent-tools/src/metrics.rs`](../crates/agent-tools/src/metrics.rs).

For deeper, time-windowed analysis the agent (via `bash`) or a human can reach the
same stack the dashboards use:

```sh
# raw exposition (counters/gauges + histogram buckets):
curl -s 127.0.0.1:9600/metrics | grep '^agent_search_'

# rates / quantiles over time via the Prometheus HTTP API:
curl -s 'http://127.0.0.1:9090/api/v1/query' \
  --data-urlencode 'query=histogram_quantile(0.95, sum(rate(agent_search_query_seconds_bucket[5m])) by (le,backend,mode))'

# recent traces (span durations) from ClickStack:
nix run .#clickstack-client -- -q "SELECT SpanName, count() n, round(avg(Duration)/1e6,1) avg_ms \
  FROM default.otel_traces WHERE SpanName LIKE 'search.%' GROUP BY SpanName FORMAT PrettyCompact"
```

The `/skill:observe` REPL skill ([`skills/observe/SKILL.md`](../skills/observe/SKILL.md))
bundles these into a runbook you can load into a session on demand.

## Where it lives in the code

- Metric registry + families: [`agent-metrics`](../crates/agent-metrics).
- Per-seam recording decorators: [`agent-runtime/src/metered.rs`](../crates/agent-runtime/src/metered.rs).
- `/metrics` HTTP server: [`agent-cli/src/metrics_server.rs`](../crates/agent-cli/src/metrics_server.rs).
- OTLP tracing + ClickHouse sink: [`agent-telemetry`](../crates/agent-telemetry).
- Ports (single source of truth): [`nix/constants.nix`](../nix/constants.nix).
- Monitoring stack apps: [`nix/prometheus`](../nix/prometheus), [`nix/grafana`](../nix/grafana),
  [`nix/clickstack`](../nix/clickstack), [`nix/clickhouse`](../nix/clickhouse).
