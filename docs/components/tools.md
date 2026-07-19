# Tools — the `Tool` seam

A tool is a capability the model can invoke by name. Tools are registered into a
`ToolRegistry`; the loop reads the registry and dispatches calls. Enabled by name
via `[tools] enabled` (empty ⇒ every registered tool).

- **Trait:** `agent_core::Tool` (+ `ToolRegistry`) ([`agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs))
- **Impl crate:** [`agent-tools`](../../crates/agent-tools)
- **Shipped:** `bash`, `read_file`, `write_file` (`tool-core`); `edit` (`tool-edit`);
  `grep`, `find`, `ls` (`tool-search`); `search` (`tool-search-index`, index-backed —
  see the [search seam](search.md)); `metrics` (`tool-metrics`, the agent inspects
  its own live performance registry — see [observability](../observability.md))
- **Cargo features:** `tool-core` (default), `tool-edit`, `tool-search`,
  `tool-search-index`, `tool-metrics`
- **Also register as tools:** [MCP](mcp.md) server tools (`mcp_<server>_<tool>`) and,
  with `[agent] subagents`, the `delegate` tool (see [runtime](runtime.md)).

## The trait

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn schema(&self) -> ToolSchema;                 // what the model is shown
    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<Observation>;
    fn parallel_safe(&self) -> bool { true }        // opt out of concurrent dispatch
}
```

A tool returns an `Observation` (`ok`/`error` + text) rather than erroring for
"expected" failures like a missing file, so the model sees the outcome and can
react. `parallel_safe` defaults to `true`; override to `false` for a tool that must
not interleave with others in the same turn (the loop then runs that turn's tools
sequentially).

## Design notes

Shared helpers in [`agent-tools/src/lib.rs`](../../crates/agent-tools/src/lib.rs)
keep the built-ins consistent: `resolve_within` (reject `..`/absolute escapes),
`truncate` (cap output at ~12KB), and `arg_str`/`arg_bool` extractors. The search
tools are gitignore-aware (via the `ignore` crate) and run their blocking walk on
`spawn_blocking`.

## Adding your own

In-tree: implement `Tool` in `agent-tools` (gate behind a `tool-*` feature),
register a factory + one line in `register_builtins`. Out-of-tree:
```rust
registry.tool("my_tool", |_cfg| Ok(Arc::new(MyTool)));
```
Then add `"my_tool"` to `[tools] enabled` (or leave the list empty for all). See the
general [extension model](../extending.md).

## Testing

`agent_testkit::EchoTool` is a ready-made tool double; drive tool dispatch through
the loop with a `ScriptedProvider` that requests it — see [testing](testing.md).
