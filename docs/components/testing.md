# Testing — the `agent-testkit` crate

Every seam is a trait, so faking one is easy — but tests used to hand-roll the same
mock provider, recording memory, and echo tool over and over. `agent-testkit`
collects those doubles in one place so testing a new impl (or the loop against it)
means reaching for a ready-made fake.

- **Crate:** [`agent-testkit`](../../crates/agent-testkit) — a **dev-dependency
  only**; nothing here belongs in a release build.
- **Use it:** add `agent-testkit.workspace = true` under `[dev-dependencies]`.

## What's in the box

| Double | Seam | Purpose |
|--------|------|---------|
| `ScriptedProvider` | `LlmProvider` | Replay a fixed `Vec<CompletionResponse>` (one per call; last repeats). Builders: `tool_turn(..)`, `final_turn(..)`, `ScriptedProvider::tools_then_final(..)`. |
| `FnProvider` | `LlmProvider` | Compute the response from the request via a closure — assert on what the loop *sent*. |
| `RecordingMemory` | `MemoryStore` | Recalls nothing, records every appended event; `tool_order()` / `events()`. Cloneable (clones share the log). |
| `StaticContext` | `ContextStrategy` | Assemble system + user, never compact. |
| `EchoTool` | `Tool` | Returns its `val` arg after an optional `sleep_ms` delay (to make completion order differ from call order). |
| `mcp::ScriptedTransport` | `McpTransport` | Answer requests from a canned `method → result` map; pair with `McpClient::with_transport` to drive the client with no subprocess. |

## Example

```rust
use agent_testkit::{tool_turn, final_turn, ScriptedProvider, RecordingMemory, StaticContext, EchoTool};

let provider = ScriptedProvider::new(vec![
    tool_turn(vec![/* a ToolCall for "echo" */]),
    final_turn("done"),
]);
let memory = RecordingMemory::new();
let agent = Agent::new(Arc::new(provider), tools, Arc::new(memory.clone()),
                       Arc::new(StaticContext), Arc::new(AutoApprove), Metrics::new(), settings);
agent.run("go").await.unwrap();
assert_eq!(memory.tool_order(), vec!["t0", "t1", "t2"]);
```

## Dependency shape

`agent-testkit` depends only on `agent-core` + `agent-mcp` (the stable seam crates),
and consumers pull it in as a **dev-dependency**, so the graph stays acyclic and no
test double reaches a release build.
