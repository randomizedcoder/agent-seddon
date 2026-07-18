# agent-seddon

<p align="center">
  <img src="agent-seddon.png" alt="agent-seddon logo" width="300">
</p>

Experimental, modular coding-agent harness in Rust. Every major component — the
LLM provider, the tools, the memory, the context assembly — sits behind a trait
and is wired by a **plugin registry**, so implementations can be swapped by config,
compiled in via cargo features, and contributed by third parties without forking.

See **[DESIGN.md](DESIGN.md)** for the architecture, the loop, and the layered
memory model; **[docs/extending.md](docs/extending.md)** for how to add a
provider/tool/memory/context/policy; and
**[docs/features-comparison.md](docs/features-comparison.md)** for how it stacks up
against other harnesses.

## Workspace

One crate per seam (DESIGN.md §7):

| Crate | Role |
|-------|------|
| `agent-core` | Seam traits + shared types (no impls) |
| `agent-providers` | `LlmProvider` impls: OpenAI-compatible (GLM/OpenAI/vLLM/Ollama) + Anthropic-native, both streaming |
| `agent-tools` | `Tool` impls: `bash`, `read_file`, `write_file`, `edit`, `grep`, `find`, `ls` |
| `agent-memory` | `MemoryStore`: JSONL episodic + markdown semantic |
| `agent-context` | `ContextStrategy`: sliding-window compaction |
| `agent-mcp` | MCP client (stdio + streamable-HTTP) — external tools as `mcp_<server>_<tool>` |
| `agent-runtime` | Config, the plugin registry, the loop (streaming + parallel tools), and subagents |
| `agent-cli` | The `agent` binary |

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

In the REPL, type a goal to run a turn (history persists across turns), or a slash
command: `/help`, `/new`, `/compact`, `/resume`, `/skills`, `/skill:<name>`,
`/model`, `/tools`, `/save`, `/quit`. Each turn is saved under `.agent/sessions/`;
resume with `--continue` (most recent) or `--resume <id>`, or `/resume` inside the
REPL. `/skills` lists reusable instruction snippets (see [`skills/`](skills/)) and
`/skill:<name>` loads one into the conversation on demand.

Set `RUST_LOG=debug` to see the model's `reasoning_content` length and compaction
decisions.

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

Prometheus metrics about a running agent. Enable in `config/agent.toml`
(`[metrics] enabled = true`) to serve `/metrics` on `listen` (default
`127.0.0.1:9600`); set `pushgateway` to also push on exit (useful for short runs).
Exposed metrics include `agent_api_calls_total`, `agent_api_call_duration_seconds`,
`agent_tokens_total`, `agent_context_tokens`, `agent_context_messages`,
`agent_tool_calls_total`, `agent_iterations_total`, `agent_runs_total`,
`agent_run_duration_seconds`, and `agent_active`.

```sh
# while a run is in progress:
curl -s 127.0.0.1:9600/metrics | grep '^agent_'
```

## Runtime state

Written under `.agent/` (git-ignored):

- `.agent/episodic.jsonl` — append-only event log ("what happened").
- `.agent/memory/*.md` — curated semantic facts ("what is true"), recalled by
  keyword match and injected into context.
- `.agent/sessions/*.jsonl` — REPL conversation transcripts, for `--continue` /
  `--resume` / `/resume`.
