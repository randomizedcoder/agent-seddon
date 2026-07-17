# agent-seddon

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
