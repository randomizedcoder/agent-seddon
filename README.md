# agent-seddon

<p align="center">
  <img src="agent-seddon.png" alt="agent-seddon logo" width="300">
</p>

Experimental, modular coding-agent harness in Rust. Every major component — the
LLM provider, the tools, the memory, the context assembly — sits behind a trait
so implementations can be swapped by config and compared cheaply.

See **[DESIGN.md](DESIGN.md)** for the architecture, the loop, the layered memory
model, and a comparison to Hermes / OpenCode / Roo Code.

## Workspace

One crate per seam (DESIGN.md §7):

| Crate | Role |
|-------|------|
| `agent-core` | Seam traits + shared types (no impls) |
| `agent-providers` | `LlmProvider` impls (OpenAI-compatible / GLM) |
| `agent-tools` | `Tool` impls: `bash`, `read_file`, `write_file` |
| `agent-memory` | `MemoryStore`: JSONL episodic + markdown semantic |
| `agent-context` | `ContextStrategy`: sliding-window compaction |
| `agent-runtime` | Config, the factory/registry, and the loop |
| `agent-cli` | The `agent` binary |

## Build & run

```sh
cargo build
cargo run -p agent-cli -- --config config/agent.toml "list the files in this repo"
```

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

> The Rust client integration that populates these tables (a composite memory sink +
> a tracing layer, keyed by a per-run `session_id`) is a follow-up; the schema and
> container are in place now.

## Configuration

Wiring lives in [`config/agent.toml`](config/agent.toml). The string fields under
`[agent]` and `[memory]` choose which seam implementation runs — swapping them is
the experimentation lever, no code edit required.

### API key (kept out of the repo)

The key is never stored in this repo. The provider config reads it, in order of
precedence, from:

1. `provider.api_key` (inline — avoid),
2. `provider.api_key_env` (an environment variable), or
3. `provider.api_key_file` (a path, e.g. `~/Downloads/runpod/glm/glm-api-key`).

The example config points `api_key_file` at the GLM key file outside the repo.

## Runtime state

Written under `.agent/` (git-ignored):

- `.agent/episodic.jsonl` — append-only event log ("what happened").
- `.agent/memory/*.md` — curated semantic facts ("what is true"), recalled by
  keyword match and injected into context.
