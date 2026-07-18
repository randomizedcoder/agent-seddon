# Extending agent-seddon

Every swappable component of the harness — the **provider**, **tools**, **memory**,
**context strategy**, and **policy** — is an `async` trait in `agent-core`, wired at
runtime by a **plugin registry**. Adding a new implementation is the same three
steps for any seam:

1. **Implement the trait** from `agent-core`.
2. **Write a factory** — a `Fn(&Config) -> anyhow::Result<Arc<dyn Trait>>`.
3. **Register it** under a config-string name.

Then a user selects it purely by config (`provider = "my-provider"`), with no code
change to the loop. There are two ways to register — in-tree (contribute upstream)
and out-of-tree (your own binary, no fork).

## The seams

| Seam | Trait (`agent-core`) | Selected by | Registry method |
|------|----------------------|-------------|-----------------|
| Provider | `LlmProvider` | `[agent] provider` | `Registry::provider` |
| Context strategy | `ContextStrategy` | `[agent] context` | `Registry::context` |
| Policy | `Policy` | `[agent] policy` | `Registry::policy` |
| Memory backend | `MemoryStore` | `[memory] backend` | `Registry::memory` |
| Tool | `Tool` | `[tools] enabled` (by name) | `Registry::tool` |

Trait contracts live in `crates/agent-core/src/lib.rs`. All are object-safe and use
`#[async_trait]`; the shared message protocol (`Message`, `ToolCall`, `Observation`,
`ToolSchema`, `CompletionChunk`, …) is the only currency between seams — don't
invent a parallel one.

Context-strategy note: the context factory receives `(&Config, &Arc<dyn
LlmProvider>)` — the already-built provider — so a strategy like
`summarizing-window` can call the model during compaction. Strategies that don't
need it (e.g. `sliding-window`) ignore the second argument.

Provider note: implement `complete` (buffered); `stream` is optional and defaults
to adapting `complete` into a single terminal chunk, so a provider works with
streaming for free and only overrides `stream` to do incremental SSE.

Tool note: `Tool::parallel_safe` defaults to `true`; override to `false` if the
tool must not run concurrently with others in a turn.

## In-tree (contribute a built-in)

Example: a new provider `"my-llm"`.

1. Add `crates/agent-providers/src/my_llm.rs` implementing `LlmProvider`, and
   gate + re-export it in `crates/agent-providers/src/lib.rs`:

   ```rust
   #[cfg(feature = "provider-my-llm")]
   mod my_llm;
   #[cfg(feature = "provider-my-llm")]
   pub use my_llm::MyLlmProvider;
   ```

2. Declare the feature in `crates/agent-providers/Cargo.toml`:

   ```toml
   [features]
   provider-my-llm = []
   ```

3. Add a factory + registration in `agent-runtime`. Put the factory in
   `crates/agent-runtime/src/builder.rs` (feature-gated), then one line in
   `register_builtins` (`crates/agent-runtime/src/registry.rs`):

   ```rust
   #[cfg(feature = "provider-my-llm")]
   r.provider("my-llm", crate::builder::my_llm_provider);
   ```

4. Forward the feature in `crates/agent-runtime/Cargo.toml` (and add it to
   `default` if it should ship by default):

   ```toml
   provider-my-llm = ["agent-providers/provider-my-llm"]
   ```

**Invariant:** a cargo feature is declared *only alongside the code it gates*, so
every build — including the CI `clippy --all-features` — stays green. Don't add a
feature name before its module exists.

Tools follow the same shape in `crates/agent-tools` (`tool-*` features); memory in
`crates/agent-memory` (`memory-*`); context in `crates/agent-context` (`context-*`).
Policies live directly in `agent-runtime` and are always registered.

## Out-of-tree (your own crate, no fork)

Depend on `agent-core` + `agent-runtime`, register your factories on a `Registry`,
and build the agent from it with `build_agent_with`:

```rust
use std::sync::Arc;
use agent_runtime::{register_builtins, build_agent_with, Config, Metrics, Registry};

let mut registry = Registry::new();
register_builtins(&mut registry);              // keep the built-ins…
registry.provider("my-llm", |cfg| {            // …and add your own
    Ok(Arc::new(my_crate::MyLlmProvider::new(cfg.provider.model.clone())?))
});

let agent = build_agent_with(&registry, config, /* telemetry */ None, session_id, Metrics::new()).await?;
```

`config.agent.provider = "my-llm"` now selects it. See
`crates/agent-cli/examples/custom_provider.rs` for a runnable example.

## External tools via MCP

Beyond in-tree/out-of-tree Rust tools, the harness can pull tools from any
**Model Context Protocol** server at startup — no code required, just config. Each
configured server is connected (`agent-mcp` feature `mcp`, on by default), its
tools discovered via `tools/list`, and each registered as `mcp_<server>_<tool>`
into the same `ToolRegistry` as the built-ins.

```toml
[[mcp.servers]]                 # stdio: spawned as a subprocess
name    = "filesystem"
command = "npx"
args    = ["-y", "@modelcontextprotocol/server-filesystem", "."]

[[mcp.servers]]                 # http: streamable-HTTP endpoint
name    = "remote"
url     = "https://mcp.example.com/mcp"
# headers = { Authorization = "Bearer …" }
```

Connection is best-effort: a server that fails to start/handshake is logged and
skipped, never aborting the run. MCP tools are always added when their server is
configured — the `[tools] enabled` allowlist only filters the built-ins. The
client lives in `crates/agent-mcp` (stdio + streamable-HTTP transports behind a
`McpTransport` trait); it implements the client half of MCP (tool discovery +
calls).

## Subagents (`delegate`)

With `[agent] subagents = true`, a `delegate` tool is registered. The model calls
it with a sub-goal; it builds a **child agent** from the same components (provider,
worker tools, context, policy, memory), runs the child's own tool loop in an
isolated context, and returns only the child's final summary. Recursion is bounded
by `[agent] subagent_max_depth` (a child gets its own `delegate` only while depth
remains). See `crates/agent-runtime/src/subagent.rs`. Off by default — nested loops
multiply token cost.

## Verifying an extension

- `cargo build` (default features) and a minimal build to confirm gating:
  `cargo build -p agent-runtime --no-default-features --features provider-openai-compat,tool-core,context-sliding-window,memory-file`
- `cargo test -p <your-crate> --features <your-feature>`
- `nix flake check` runs `clippy --all-features -D warnings`, rustfmt, tests, and
  cargo-audit hermetically — the source of truth for CI.
