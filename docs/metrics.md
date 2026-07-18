# Metrics (Prometheus + Grafana)

Per-component Prometheus metrics for a running agent, scraped by Prometheus and
visualised in a provisioned Grafana dashboard. This is the **metrics** leg of
observability; it complements the OTLP **tracing** ([docs/tracing.md](tracing.md))
and the native ClickHouse transaction sink. Everything ships behind the same Nix
docker-app pattern as ClickHouse/ClickStack.

## What's instrumented

Instrumentation is unconditional and cheap — a shared `agent_metrics::Metrics`
registry is threaded into every seam and the loop; only *serving* `/metrics` is
gated by `[metrics] enabled`.

- **Loop** (`agent-runtime`): `agent_runs_total`, `agent_run_duration_seconds`,
  `agent_active`, `agent_iterations_total`, `agent_api_calls_total`,
  `agent_api_call_duration_seconds`, `agent_tokens_total`,
  `agent_context_tokens`, `agent_context_messages`, `agent_tool_calls_total`.
- **Per component** (recorded at each seam via a metrics wrapper — see
  `crates/agent-runtime/src/metered.rs`):
  - Provider — `agent_provider_request_seconds{provider,stream}`,
    `agent_provider_ttft_seconds`, `agent_provider_stream_chunks_total`,
    `agent_provider_errors_total`.
  - Tools — `agent_tool_exec_seconds{tool}`, `agent_tool_errors_total`
    (covers built-in, MCP `mcp_*`, and remote gRPC tools).
  - Memory — `agent_memory_op_seconds{op}`, `agent_memory_recall_items`,
    `agent_memory_errors_total`.
  - Context — `agent_context_op_seconds{op}`,
    `agent_context_compactions_total`, `agent_context_compact_tokens{when}`.
  - Policy — `agent_policy_authorize_total{policy,decision}`,
    `agent_policy_authorize_seconds`.

Because the wrapper sits at the seam boundary, a remote `= "grpc"` seam is timed
on the loop side (label `provider="grpc"`, etc.), and the same wrapper on a
`--serve-<seam>` process captures that seam's **server-side** latency.

## Quick start (single process)

Metrics are enabled by default in [`config/agent.toml`](../config/agent.toml)
(`[metrics] enabled = true`), served on `127.0.0.1:9600`.

```sh
nix run .#prometheus-up          # Prometheus  UI :9090, scrapes :9600–:9605
nix run .#grafana-up             # Grafana     UI :3000, provisioned dashboard

# start a session so metrics accumulate (the REPL stays up for scraping):
nix run .#agent -- --config config/agent.toml

# in another terminal, confirm the endpoint:
curl -s 127.0.0.1:9600/metrics | grep -E '^agent_(provider|tool|memory|context|policy)_'
```

Open Grafana at <http://localhost:3000> (anonymous Admin) → **Dashboards →
agent-seddon**. Prometheus target health is at
<http://localhost:9090/targets>.

```sh
nix run .#prometheus-down
nix run .#grafana-down
```

> One-shot runs (`agent … "a goal"`) may start and exit between Prometheus'
> 5s scrapes, so short runs can be missed — use the REPL or a `--serve-<seam>`
> process for live scraping, or set `[metrics] pushgateway` for short runs.

## Distributed topology

Each seam server exposes its own `/metrics` on a dedicated port (from
[`nix/constants.nix`](../nix/constants.nix)), scraped as a separate Prometheus
job so co-located servers don't collide on `:9600`:

| Seam server | Prometheus job | `/metrics` |
|-------------|----------------|-----------|
| `--serve-provider` | `provider` | `127.0.0.1:9601` |
| `--serve-memory`   | `memory`   | `127.0.0.1:9602` |
| `--serve-tools`    | `tools`    | `127.0.0.1:9603` |
| `--serve-context`  | `context`  | `127.0.0.1:9604` |
| `--serve-policy`   | `policy`   | `127.0.0.1:9605` |

Run the `config/otel-demo` two-process demo (a gateway + a `provider = "grpc"`
loop) and both the loop (`agent`, `:9600`) and the gateway (`provider`, `:9601`)
appear UP in Prometheus; the dashboard's `$job` selector filters between them.

## Networking note (Linux)

The Prometheus and Grafana containers run with docker `--network host` so
Prometheus can scrape the agent's loopback `127.0.0.1` ports and Grafana can
reach Prometheus at `127.0.0.1:9090`. On macOS/Windows docker there is no host
networking — swap the targets to `host.docker.internal` and drop `--network
host` (see the header comments in `nix/prometheus/default.nix`).

## Where it lives

- `crates/agent-metrics` — the shared registry + metric families.
- `crates/agent-runtime/src/metered.rs` — the per-seam wrappers (applied in
  `builder.rs`).
- `nix/prometheus/`, `nix/grafana/` — the docker-app modules + provisioning +
  dashboard JSON. Scrape targets are generated from `nix/constants.nix`.
