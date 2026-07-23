# Operating agent-seddon

Running it, configuring it, and where it puts things. For what the components *do*
see [`README.md`](README.md); for the observability stack see
[`observability.md`](observability.md).

## Running

The default configs target a local [Ollama](https://ollama.com), so a fresh clone
runs with no account and no API key:

```sh
ollama pull llama3.1:latest      # a model that really CALLS TOOLS — required
ollama serve                     # if not already running

cargo build
# one-shot:
cargo run -p agent-cli -- --config config/local-ollama.toml "list the files in this repo"
# interactive REPL (no goal): multi-turn, streaming, Ctrl-D to exit
cargo run -p agent-cli -- --config config/local-ollama.toml
```

Two configs ship, and they differ in purpose rather than backend:

- [`../config/local-ollama.toml`](../config/local-ollama.toml) — minimal and
  runnable, a small tool set, no indexing on first run. Start here.
- [`../config/agent.toml`](../config/agent.toml) — the annotated reference: every
  seam, every knob, with the rationale inline. Copy from it as you need things.

For a hosted model, set `[provider] base_url` and `model` and supply a key. Note
that **a key is required even when the endpoint ignores it** (Ollama's placeholder
is `api_key = "ollama"`), and that `[agent] context_window` must match the model
you actually configured — it is the budget compaction works against. See
[`components/providers.md`](components/providers.md#configuring-an-endpoint).

The REPL has arrow-key line editing and command history (via `rustyline`, stored in
`.agent/sessions/.repl_history`); piped input (`printf … | agent`) falls back to
plain line reading. Type a goal to run a turn, or a slash command:

`/help` · `/new` · `/compact` · `/resume` · `/skills` · `/skill:<name>` · `/model`
· `/tools` · `/save` · `/quit`

Each turn is saved under `.agent/sessions/`; resume with `--continue` (most recent)
or `--resume <id>`, or `/resume` inside the REPL. `/skills` lists reusable
instruction snippets (see [`../skills/`](../skills/)) and `/skill:<name>` loads one
into the conversation on demand.

`RUST_LOG=debug` shows the model's `reasoning_content` length and compaction
decisions.

## Configuration

Wiring lives in [`../config/agent.toml`](../config/agent.toml). The string fields
choose which seam implementation runs — `[agent] provider`, `context`, `policy`,
`[memory] backend`, `[tools] enabled`, and the per-seam `backend` keys. Swapping
them is the experimentation lever; no code edit required.

Two loop-level switches worth knowing:

- `[agent] stream` toggles incremental SSE streaming with a live stderr echo. Set
  it `false` for the buffered path — the escape hatch for servers that misbehave on
  SSE, and the only path that reports token usage for openai-compat providers.
- `[agent] parallel_tools` runs a turn's parallel-safe tool calls concurrently.

> **Unknown keys are warned about, not rejected.** A misplaced or misspelled key is
> reported at startup with its full dotted path (`unknown config key … key=agent.memory`)
> rather than silently doing nothing. It is a warning so a stale key cannot fail a
> startup, but it will not be silent.

### API key

The key is never stored in this repo. The provider config reads it, in order of
precedence, from:

1. `provider.api_key` (inline — avoid),
2. `provider.api_key_env` (an environment variable), or
3. `provider.api_key_file` (a path outside the repo).

### User context files (`context.d/`)

Markdown files in `context.d/` are added to the model context on **every** run —
always injected, unlike relevance-recalled semantic memory:

- `context.d/prepend/NNNN_*.md` — folded into the system prompt, before the goal.
- `context.d/append/NNNN_*.md` — a trailing message, after the goal.

The leading `NNNN` orders files ascending. The directory is set by
`[context_files] dir` (default `context.d`); a missing directory means no
injection. See [`../context.d/README.md`](../context.d/README.md).

## Runtime state

Written under `.agent/` and `.agent-seddon/` (both git-ignored):

- `.agent/episodic.jsonl` — append-only event log ("what happened"). This is the
  durable record even when telemetry is enabled.
- `.agent/memory/*.md` — curated semantic facts ("what is true"), recalled and
  injected into context.
- `.agent/sessions/*.jsonl` — REPL transcripts, for `--continue` / `--resume`.
- `.agent-seddon/index/<backend>/` — the code-search index.
- `.agent-seddon/session/` — content-addressed session checkpoints.

## As an MCP server

`agent --serve-mcp` runs agent-seddon as a [Model Context
Protocol](https://modelcontextprotocol.io) server over stdio, exposing a single
`run` tool: any MCP client (Claude Desktop, another agent, …) hands it a goal and
gets the final answer back. stdout carries only JSON-RPC; logs and the streaming
echo go to stderr.

```jsonc
// e.g. in an MCP client's server config:
{ "command": "agent", "args": ["--serve-mcp", "--config", "/path/to/agent.toml"] }
```

Use a non-interactive policy (`auto-approve`) — stdin is the JSON-RPC channel, so
an interactive approval prompt cannot read it. This is the server counterpart to
the `agent-mcp` client, which *consumes* other MCP servers; see
[`components/mcp.md`](components/mcp.md).

## Nix

A modular flake (thin `flake.nix` + a `./nix/` aggregator, Rust toolchain via
`rust-overlay`, builds via `crane`) provides the dev shell, the checks, and the
container apps. `nix/versions.nix` is the single source of truth for tool versions,
and `nix/constants.nix` for every port and socket path.

```sh
nix develop                 # dev shell: toolchain + tools + an `agent-help` menu
nix flake check             # the gate — see below
nix build .#agent           # build the binary -> ./result/bin/agent
nix run   .#agent -- --config config/agent.toml "list files in this repo"
nix fmt                     # format all .nix files
```

`nix flake check` runs nine checks: clippy (`-D warnings`), rustfmt, tests,
`cargo-audit`, nix-fmt, generated-constant drift, `buf lint`, `buf breaking`, and
the bench + leak suites ([`benchmarking.md`](benchmarking.md)).

### Does it actually work? (`nix run .#e2e-live`)

The gate proves the binary, the wire format and the tool path against a
*scripted* server ([`crates/agent-cli/tests/cli_e2e.rs`](../crates/agent-cli/tests/cli_e2e.rs)).
It cannot prove a real model can drive them, because the check sandbox has no
network. That is what this app is for:

```sh
ollama serve && ollama pull llama3.1:latest
nix run .#e2e-live
```

It runs the real agent on a real model, asks for a C hello world, then compiles
and runs the result. Point it at any OpenAI-compatible endpoint:

| Variable | Default | Notes |
|---|---|---|
| `AGENT_E2E_BASE_URL` | `http://localhost:11434/v1` | |
| `AGENT_E2E_MODEL` | `llama3.1:latest` | |
| `AGENT_E2E_API_KEY` | `ollama` | Required even when ignored |
| `AGENT_E2E_MAX_TOKENS` | `2048` | **Raise for a reasoning model** — see below |
| `AGENT_E2E_CONTEXT_WINDOW` | `8192` | Must not exceed what the server serves |
| `AGENT_E2E_INSECURE_TLS` | `0` | `1` skips certificate verification (self-signed dev endpoints only) |

```sh
AGENT_E2E_BASE_URL=https://my-endpoint:8000/v1 AGENT_E2E_MODEL=/model \
AGENT_E2E_API_KEY="$MY_KEY" AGENT_E2E_INSECURE_TLS=1 \
AGENT_E2E_MAX_TOKENS=8192 AGENT_E2E_CONTEXT_WINDOW=131072 \
  nix run .#e2e-live
```

> **Reasoning models need a much larger `max_tokens`.** They spend output tokens
> on `reasoning_content` *before* writing any answer, so a budget sized for a
> plain model is consumed entirely by the reasoning and comes back with
> `finish_reason: length` and empty `content` — which looks like the model
> ignoring you rather than running out of room. 8192 is a reasonable floor.

The exit codes are split on purpose, because the two failures have different
owners and merging them makes the result useless:

| Exit | Meaning |
|:--:|---|
| `0` | The agent ran the task **and** the program compiles and prints a greeting. |
| `1` | **Harness failure** — the agent errored, called no tool, or wrote no file. Our bug. It dumps the agent's stderr and `.agent/episodic.jsonl`, which is the record of what it actually did. |
| `2` | **Model-quality failure** — the agent worked, but the generated C does not compile. Not our bug, but not hidden either. |

If no model server is reachable it **fails** rather than skipping: a skip that
exits 0 reads as a pass, and the entire point of this tier is that it really
talked to a model.

> Expect exit 2 from small models sometimes. `llama3.1:latest` at
> `temperature = 0` passed three runs out of three; at the default temperature it
> emitted `<stdio.h>` and literal `\n` escapes and failed to compile.
> Sampling noise in an 8B model shows up directly as broken C.

> The toolchain is supplied reproducibly by the flake lock, but is **not pinned to
> a specific Rust version** — `nix/versions.nix` uses `rust-bin.stable.latest`.
> Pin it there if you need a frozen toolchain.

### ClickHouse (run history)

A pinned ClickHouse container holds the transaction history, logs and token usage.
Schema: [`../nix/clickhouse/schema.sql`](../nix/clickhouse/schema.sql) —
`agent_events`, `agent_logs`, `agent_usage`, and `agent_verifications` (tool-call
verifier verdicts + outcome proxies, see [`components/verifier.md`](components/verifier.md)).
Requires a running Docker daemon.

```sh
nix run .#clickhouse-up                                  # start + apply schema
nix run .#clickhouse-client -- -q 'SHOW TABLES FROM agent'
nix run .#clickhouse-down                                # stop + remove (data discarded)
```

Enable `[telemetry] enabled = true` and run a goal to populate them. Each run gets
a `session_id`, printed at the end. Writes use ClickHouse's **native protocol**
(port 9000) through a background batched writer, which also disables ClickHouse's
own `system.query_log` for its connection so high-frequency inserts do not bloat
the server's internal logs.

It is best-effort: if ClickHouse is unreachable the loop is unaffected and
`.agent/episodic.jsonl` still holds the full record.

```sh
nix run .#clickhouse-client -- -q \
  "SELECT kind, role, substring(content,1,60) FROM agent.agent_events ORDER BY ts, seq FORMAT PrettyCompact"
```

### Prometheus, Grafana and ClickStack

```sh
nix run .#prometheus-up          # scraper, UI :9090 — scrape targets generated from nix/constants.nix
nix run .#grafana-up             # dashboards, UI :3000 (Dashboards → agent-seddon)
nix run .#clickstack-up          # OTLP receiver + trace UI :8080, OTLP :4317
```

The main agent serves `/metrics` on `127.0.0.1:9600`; each `--serve-<seam>` process
serves its own, and the `--serve-all` gateway has one too. The exact ports are
generated — read `nix/constants.nix` rather than memorising them.

Runbooks: [`metrics.md`](metrics.md), [`tracing.md`](tracing.md).
