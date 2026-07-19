# Extending agent-seddon

Every swappable component is an `async` trait in `agent-core`, wired at runtime by a
plugin [`Registry`](../crates/agent-runtime/src/registry.rs). Adding an
implementation is the **same three steps for any seam**:

1. **Implement the trait** from `agent-core`.
2. **Write a factory** — `Fn(&Config) -> anyhow::Result<Arc<dyn Trait>>` (a few
   seams also receive the built provider — see the per-component notes).
3. **Register it** under a config-string name.

Then a user selects it purely by config, with no change to the loop. This doc covers
the shared mechanics; each component doc has the specifics (trait shape, config key,
and its own "Adding your own"):

| Seam | Config key | Registry method | Component doc |
|------|-----------|-----------------|---------------|
| Provider | `[agent] provider` | `Registry::provider` | [providers](components/providers.md) |
| Context strategy | `[agent] context` | `Registry::context` | [context](components/context.md) |
| Policy | `[agent] policy` | `Registry::policy` | [policy](components/policy.md) |
| Tool | `[tools] enabled` | `Registry::tool` | [tools](components/tools.md) |
| Memory (whole store) | `[memory] backend` | `Registry::memory` | [memory](components/memory.md) |
| Memory — episodic | `[memory] backend` | `Registry::episodic` | [memory](components/memory.md) |
| Memory — semantic | `[memory] semantic` | `Registry::semantic` | [memory](components/memory.md) |
| MCP transport | `[[mcp.servers]] kind` | `Registry::transport` | [mcp](components/mcp.md) |
| Search | `[search] backends` | `Registry::search` | [search](components/search.md) |
| Git (repo) | `[git] backend` | `Registry::repo` | [git](components/git.md) |

The shared message protocol (`Message`, `ToolCall`, `Observation`, `ToolSchema`,
`CompletionChunk`, …) is the only currency between seams — don't invent a parallel
one. Trait contracts live in [`crates/agent-core/src/lib.rs`](../crates/agent-core/src/lib.rs);
all are object-safe and `#[async_trait]`.

## In-tree (contribute a built-in)

Example: a new provider `"my-llm"`.

1. Add `crates/agent-providers/src/my_llm.rs` implementing `LlmProvider`, and gate +
   re-export it in `lib.rs`:

   ```rust
   #[cfg(feature = "provider-my-llm")]
   mod my_llm;
   #[cfg(feature = "provider-my-llm")]
   pub use my_llm::MyLlmProvider;
   ```

2. Declare the feature in that crate's `Cargo.toml` (`provider-my-llm = []`).
3. Add a factory (in `agent-runtime/src/builder.rs` if it needs more than a
   one-liner) + one feature-gated line in `register_builtins`
   ([`registry.rs`](../crates/agent-runtime/src/registry.rs)):

   ```rust
   #[cfg(feature = "provider-my-llm")]
   r.provider("my-llm", crate::builder::my_llm_provider);
   ```

4. Forward the feature in `agent-runtime/Cargo.toml` (and add it to `default` if it
   should ship by default).

**Invariant:** a cargo feature is declared *only alongside the code it gates*, so
every build — including CI's `clippy --all-features` — stays green. Don't add a
feature name before its module exists.

Tools follow the same shape in `agent-tools` (`tool-*`); memory in `agent-memory`
(`memory-*`); context in `agent-context` (`context-*`). Policies live directly in
`agent-runtime` and are always registered.

## Out-of-tree (your own crate, no fork)

Depend on `agent-core` + `agent-runtime`, register your factories on a `Registry`,
and build from it with `build_agent_with`:

```rust
use agent_runtime::{register_builtins, build_agent_with, Config, Metrics, Registry};

let mut registry = Registry::new();
register_builtins(&mut registry);              // keep the built-ins…
registry.provider("my-llm", |cfg| {            // …and add your own
    Ok(Arc::new(my_crate::MyLlmProvider::new(cfg.provider.model.clone())?))
});

let agent = build_agent_with(&registry, config, None, session_id, Metrics::new()).await?;
```

`config.agent.provider = "my-llm"` now selects it. The same pattern registers custom
tools, context strategies, policies, memory layers, and
[MCP transports](components/mcp.md) (`registry.transport(...)`). See
[`examples/custom_provider.rs`](../crates/agent-cli/examples/custom_provider.rs).

## Skills, subagents, and other extension points

Some things extend the agent without touching a Rust trait — **skills** (`SKILL.md`
files) and **subagents** (`delegate`). Those live in the
[runtime doc](components/runtime.md).

## Verifying an extension

Test new impls with [`agent-testkit`](components/testing.md) (a dev-dependency)
instead of hand-rolling doubles. Then:

- `cargo build` (default features) and a minimal build to confirm gating:
  `cargo build -p agent-runtime --no-default-features --features provider-openai-compat,tool-core,context-sliding-window,memory-file`
- `cargo test -p <your-crate> --features <your-feature>`
- `nix flake check` runs `clippy --all-features -D warnings`, rustfmt, tests, and
  cargo-audit hermetically — the source of truth for CI.
