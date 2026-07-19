# agent-seddon

<p align="center">
  <img src="agent-seddon.png" alt="agent-seddon logo" width="300">
</p>

Experimental, modular coding-agent harness in Rust. Every major component — the
LLM provider, the tools, the memory, the context assembly — sits behind a trait
and is wired by a **plugin registry**, so implementations can be swapped by config,
compiled in via cargo features, and contributed by third parties without forking.

See **[DESIGN.md](DESIGN.md)** for the design rationale, the loop, and the layered
memory model; **[docs/architecture.md](docs/architecture.md)** for the boundary map
and per-component docs; **[docs/extending.md](docs/extending.md)** for how to add a
provider/tool/memory/context/policy/transport;
**[docs/grpc.md](docs/grpc.md)** for running the components as distributed gRPC
services; **[docs/observability.md](docs/observability.md)** for the metrics +
tracing + logs overview (Grafana dashboards, HyperDX, and how the agent inspects
its own performance); and **[docs/features-comparison.md](docs/features-comparison.md)**
for how it stacks up against other harnesses.

## Workspace

One crate per seam (DESIGN.md §7):

| Crate | Role |
|-------|------|
| `agent-core` | Seam traits + shared types (no impls) |
| `agent-providers` | `LlmProvider` impls: OpenAI-compatible (GLM/OpenAI/vLLM/Ollama) + Anthropic-native, both streaming |
| `agent-tools` | `Tool` impls: `bash`, `read_file`, `write_file`, `edit`, `grep`, `find`, `ls`, `search`, `metrics` (self-inspection) |
| `agent-search` | `SearchBackend`: high-performance code search (tantivy full-text index) ([docs/components/search.md](docs/components/search.md)) |
| `agent-memory` | `MemoryStore`: JSONL episodic + markdown semantic |
| `agent-context` | `ContextStrategy`: sliding-window or summarizing-window compaction |
| `agent-mcp` | MCP client (stdio + streamable-HTTP) — external tools as `mcp_<server>_<tool>` |
| `agent-proto` | protobuf/gRPC wire contracts for the seams + core↔proto conversions & OTel trace propagation ([docs/grpc.md](docs/grpc.md)) |
| `agent-grpc` | per-seam gRPC servers + clients over TCP or unix domain sockets (`--serve-<seam>`, `= "grpc"`) |
| `agent-telemetry` | Telemetry sink: streams transaction history, logs & usage to ClickHouse; OTLP trace export to the ClickStack collector |
| `agent-metrics` | Shared Prometheus registry + per-seam metric families ([docs/metrics.md](docs/metrics.md)) |
| `agent-runtime` | Config, the plugin registry, the loop (streaming + parallel tools), sessions, subagents |
| `agent-cli` | The `agent` binary (CLI + REPL + `--serve-mcp` + `--serve-<seam>`) |

## Plugins & features

Each seam implementation is a **registered plugin** selected by a config string
(`provider = "anthropic"`, `[tools] enabled = ["edit", "grep", …]`) and gated by a
**cargo feature**, so a build links only what it needs:

```sh
cargo build                    # default: both providers, all tools, sliding-window, file memory
cargo build -p agent-runtime --no-default-features \
  --features provider-openai-compat,tool-core,context-sliding-window,memory-file   # minimal
```

Adding a module is: implement the `agent-core` trait → register a factory → select
by config. Do it in-tree (a `#[cfg(feature)]` line in `register_builtins`) or
out-of-tree from your own binary via the public `Registry` + `build_agent_with`
API — see **[docs/extending.md](docs/extending.md)** and the runnable
`cargo run -p agent-cli --example custom_provider`.

Two more ways to get tools without writing Rust:

- **MCP** — list external [Model Context Protocol](https://modelcontextprotocol.io)
  servers under `[[mcp.servers]]` (stdio or streamable-HTTP); their tools are
  discovered at startup and registered as `mcp_<server>_<tool>`.
- **Subagents** — set `[agent] subagents = true` to expose a `delegate` tool the
  model can call to run a well-scoped sub-task in a child agent with isolated
  context (returns only a summary; depth-bounded).

## Build & run

```sh
cargo build
# one-shot:
cargo run -p agent-cli -- --config config/agent.toml "list the files in this repo"
# interactive REPL (no goal): multi-turn, streaming, slash commands, Ctrl-D to exit
cargo run -p agent-cli -- --config config/agent.toml
```

The REPL has arrow-key line editing + command history (via `rustyline`, stored in
`.agent/sessions/.repl_history`); piped input (`printf … | agent`) falls back to
plain line reading. Type a goal to run a turn (history persists across turns), or a
slash command: `/help`, `/new`, `/compact`, `/resume`, `/skills`, `/skill:<name>`,
`/model`, `/tools`, `/save`, `/quit`. Each turn is saved under `.agent/sessions/`;
resume with `--continue` (most recent) or `--resume <id>`, or `/resume` inside the
REPL. `/skills` lists reusable instruction snippets (see [`skills/`](skills/)) and
`/skill:<name>` loads one into the conversation on demand.

Set `RUST_LOG=debug` to see the model's `reasoning_content` length and compaction
decisions.

## As an MCP server

`agent --serve-mcp` runs agent-seddon as a [Model Context
Protocol](https://modelcontextprotocol.io) server over stdio, exposing a single
`run` tool: any MCP client (Claude Desktop, another agent, …) can hand it a goal
and get the final answer back. stdout carries only JSON-RPC; logs and the
streaming echo go to stderr.

```jsonc
// e.g. in an MCP client's server config:
{ "command": "agent", "args": ["--serve-mcp", "--config", "/path/to/agent.toml"] }
```

Use a non-interactive policy (`auto-approve`) — stdin is the JSON-RPC channel, so
an interactive approval prompt can't read it. (This is the server counterpart to
the `agent-mcp` client, which *consumes* other MCP servers.)

## Distributing components (gRPC)

Because every seam is a trait selected by config, a component can run **in a
different process or on a different machine** with no change to the loop — a remote
seam is just another impl, chosen with `= "grpc"`. `agent-proto` defines the
protobuf wire contract for every seam (all binary — arbitrary JSON args ride as a
lossless `JsonValue`, not text); `agent-grpc` provides the per-seam servers and
clients over **TCP or unix domain sockets** (UDS is the same-host fast path,
bound `0700`/`0600`).

Host a seam with `agent --serve-<seam>` and point another agent at it:

```sh
# terminal A — a model gateway serving the real provider over gRPC (default :50051)
agent --serve-provider --config gateway.toml         # gateway.toml: provider = "anthropic"

# terminal B — a loop that dials the gateway instead of calling the model directly
#   loop.toml:  [agent] provider = "grpc"
#               [grpc.provider] endpoint = "http://127.0.0.1:50051"  (or unix:/path)
agent --config loop.toml "list the files in this repo"
```

`--serve-provider|--serve-memory|--serve-tools|--serve-context|--serve-policy|--serve-search` each
host one seam; ports/socket paths are generated from `nix/constants.nix` into
`agent-grpc`'s `constants.rs` (a `nix flake check` guard keeps them in sync). This
lifts the design's "distributed components" goal to a k8s-style topology — a
gateway, a shared memory service, sandboxed tool workers. Full contract, error
mapping, and deployment sketch in **[docs/grpc.md](docs/grpc.md)**.

## Nix

A modular flake (thin `flake.nix` + `./nix/` aggregator, pinned Rust toolchain via
`rust-overlay`, builds via `crane`) provides the dev shell, checks, and a ClickHouse
container. See `nix/versions.nix` for the single source of truth on tool versions.

```sh
nix develop                 # dev shell: toolchain + tools + `agent-help` menu
nix flake check             # clippy (-D warnings) + rustfmt + tests + cargo-audit + nix-fmt
nix build .#agent           # build the `agent` binary -> ./result/bin/agent
nix run   .#agent -- --config config/agent.toml "list files in this repo"
nix fmt                     # format all .nix files
```

### ClickHouse (telemetry)

A pinned ClickHouse container (Docker) holds the agent's full transaction history,
logs, and token usage. Schema: [`nix/clickhouse/schema.sql`](nix/clickhouse/schema.sql)
(`agent_events`, `agent_logs`, `agent_usage`). Requires a running Docker daemon.

```sh
nix run .#clickhouse-up                                  # start + apply schema
nix run .#clickhouse-client -- -q 'SHOW TABLES FROM agent'
nix run .#clickhouse-down                                # stop + remove (data discarded)
```

For **distributed OpenTelemetry tracing** (spans that follow a request across
gRPC components into a **ClickStack / HyperDX** UI), a separate all-in-one container
is provided (`nix run .#clickstack-up`, UI on `:8080`, OTLP on `:4317`). See the
runbook in [`docs/tracing.md`](docs/tracing.md).

To actually populate the tables, enable telemetry in `config/agent.toml`
(`[telemetry] enabled = true`) and run a goal. Each run gets a `session_id`
(printed at the end); the composite memory sink mirrors every event into
`agent_events`, a tracing layer streams logs into `agent_logs`, and per-turn
token counts land in `agent_usage`. It's best-effort — if ClickHouse is
unreachable the loop is unaffected and `.agent/episodic.jsonl` still holds the
full record.

Writes use ClickHouse's **native protocol** (`klickhouse`, port 9000) via a
background batched writer, and the writer disables ClickHouse's own
`system.query_log` for its connection so the high-frequency telemetry inserts
don't bloat the server's internal logs.

```sh
# with telemetry enabled and the container up:
nix run .#agent -- --config config/agent.toml "list the files in this repo"
nix run .#clickhouse-client -- -q \
  "SELECT kind, role, substring(content,1,60) FROM agent.agent_events ORDER BY ts, seq FORMAT PrettyCompact"
```

## Configuration

Wiring lives in [`config/agent.toml`](config/agent.toml). The string fields under
`[agent]` and `[memory]` choose which seam implementation runs (`provider`,
`context`, `policy`, `[memory] backend`, `[tools] enabled`) — swapping them is the
experimentation lever, no code edit required. `[agent] stream` toggles incremental
SSE streaming with a live stderr echo (set `false` for the buffered path — the
escape hatch for servers that misbehave on SSE, and the only path that reports
token usage for openai-compat); `[agent] parallel_tools` runs a turn's
parallel-safe tool calls concurrently.

### API key (kept out of the repo)

The key is never stored in this repo. The provider config reads it, in order of
precedence, from:

1. `provider.api_key` (inline — avoid),
2. `provider.api_key_env` (an environment variable), or
3. `provider.api_key_file` (a path, e.g. `~/Downloads/runpod/glm/glm-api-key`).

The example config points `api_key_file` at the GLM key file outside the repo.

### User context files (`context.d/`)

Drop markdown files into `context.d/` to add fixed entries to the model context on
every run — always injected (unlike relevance-recalled semantic memory):

- `context.d/prepend/NNNN_*.md` — folded into the system prompt (before the goal).
- `context.d/append/NNNN_*.md` — a trailing message (after the goal).

The leading `NNNN` orders files ascending. The directory is set by
`[context_files] dir` (default `context.d`); a missing directory means no injection.
See [`context.d/README.md`](context.d/README.md).

## Metrics

Prometheus metrics about a running agent — enabled in `config/agent.toml`
(`[metrics] enabled = true`) to serve `/metrics` on `listen` (default
`127.0.0.1:9600`). Alongside the loop-level counters (`agent_api_calls_total`,
`agent_api_call_duration_seconds`, `agent_tokens_total`, `agent_context_tokens`,
`agent_context_messages`, `agent_tool_calls_total`, `agent_iterations_total`,
`agent_runs_total`, `agent_run_duration_seconds`, `agent_active`), **each seam is
instrumented independently** — a metrics wrapper (`agent-runtime/src/metered.rs`)
records provider request/TTFT (`agent_provider_*`), per-tool latency
(`agent_tool_exec_seconds`), memory ops (`agent_memory_*`), context
assemble/compact (`agent_context_*`), policy authorize (`agent_policy_*`), and
search index/query timings labelled by backend (`agent_search_*`). A remote
`= "grpc"` seam is timed on the client side, and each `--serve-<seam>` process
serves its own `/metrics` (ports 9601–9606) for server-side latency.

The agent can inspect **its own** performance in-process via the built-in
`metrics` tool (no stack required) — see **[docs/observability.md](docs/observability.md)**.

A **Prometheus + Grafana** stack ships as Nix docker-apps (same pattern as
ClickHouse/ClickStack), with a provisioned per-component dashboard:

```sh
nix run .#prometheus-up          # scraper — UI :9090, scrapes :9600–:9606
nix run .#grafana-up             # dashboards — UI :3000 (Dashboards → agent-seddon)
# run a REPL session (or the gRPC demo), then watch the dashboard fill.
nix run .#grafana-down && nix run .#prometheus-down

# or just curl the endpoint while a session is live:
curl -s 127.0.0.1:9600/metrics | grep '^agent_'
```

Full runbook (single-process + distributed topology + networking notes):
**[docs/metrics.md](docs/metrics.md)**; the three-signal overview (metrics +
tracing + logs, and agent self-inspection) is **[docs/observability.md](docs/observability.md)**.

## Tracing (OpenTelemetry)

Distributed traces show the agent's loop as a **span tree** and follow a request
across process boundaries. Turn it on with a single knob:

```toml
[telemetry]
otlp_endpoint = "http://127.0.0.1:4317"   # OTLP/gRPC receiver (empty = off)
# otlp_headers = "authorization=<key>"    # if the collector authenticates ingestion
```

How it works:

- **Spans.** The loop (`agent-runtime`) wraps each seam interaction in a span, so a
  run is `agent.turn → memory.recall · context.assemble · provider.complete/stream ·
  policy.authorize · tool.execute · context.compact` (+ `agent.delegate` for
  subagents). `tracing-opentelemetry` also auto-tags each span with its
  `code.filepath`/`lineno`, so a span links back to source.
- **Export.** A `tracing` layer (`agent-telemetry`) batches the spans and exports
  them over **OTLP/gRPC** to any collector. It's *additive* to the native ClickHouse
  sink and **independent of `RUST_LOG`** (its own `INFO` filter — so quieting the
  console doesn't silently disable tracing).
- **Propagation.** The gRPC transports carry the **W3C trace context** in request
  metadata (`agent-proto`), so when a seam is remote (`= "grpc"`), the server's
  handler span is a child of the caller's span — **one trace across both
  processes**.

A ready-to-run demo ships as a **ClickStack / HyperDX** all-in-one container
(collector + ClickHouse + UI):

```sh
nix run .#clickstack-up          # UI :8080, OTLP :4317
# run the two-process demo in config/otel-demo/ (a gateway + a provider = "grpc" loop),
# then open http://localhost:8080 → Traces to see the distributed waterfall.
nix run .#clickstack-down
```

Full runbook (including the two-process distributed trace and the setup gotchas):
**[docs/tracing.md](docs/tracing.md)**.

## Runtime state

Written under `.agent/` (git-ignored):

- `.agent/episodic.jsonl` — append-only event log ("what happened").
- `.agent/memory/*.md` — curated semantic facts ("what is true"), recalled by
  keyword match and injected into context.
- `.agent/sessions/*.jsonl` — REPL conversation transcripts, for `--continue` /
  `--resume` / `/resume`.
