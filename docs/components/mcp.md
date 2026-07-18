# MCP — external tools + the `McpTransport` seam

The harness is both an MCP **client** (pull tools from external Model Context
Protocol servers) and an MCP **server** (expose itself over stdio). The transport a
client uses is its own seam, wired like every other.

- **Crate:** [`agent-mcp`](../../crates/agent-mcp) (client half of MCP: `initialize`,
  `tools/list`, `tools/call`)
- **Transport trait:** `agent_mcp::McpTransport` (JSON-RPC `request` + `notify`)
- **Shipped transports:** `stdio` (spawn a subprocess), `http` (streamable HTTP)
- **Cargo feature:** `mcp` (default, in `agent-runtime`)

## Client: external tools with no code

Each configured server is connected at startup, its tools discovered via
`tools/list`, and each wrapped as an `agent_core::Tool` named `mcp_<server>_<tool>`
so it drops into the same [`ToolRegistry`](tools.md) as the built-ins. Connection is
best-effort: a server that fails to start/handshake is logged and skipped.

```toml
[[mcp.servers]]                 # stdio: spawned as a subprocess
name    = "filesystem"
command = "npx"
args    = ["-y", "@modelcontextprotocol/server-filesystem", "."]

[[mcp.servers]]                 # http: streamable-HTTP endpoint
name = "remote"
url  = "https://mcp.example.com/mcp"
```

## The transport seam

Transports live behind `agent_mcp::TransportRegistry`, which the runtime
[`Registry`](runtime.md) owns — so a custom transport registers exactly like a
provider or tool. Built-ins come from `TransportRegistry::with_builtins`; the kind
is inferred from config (`command` → stdio, `url` → http) or named explicitly.

```rust
#[async_trait]
pub trait TransportFactory: Send + Sync {
    async fn build(&self, cfg: &ServerConfig) -> Result<Box<dyn McpTransport>>;
}
```

### Adding a transport (e.g. websocket)

```rust
registry.transport("websocket", MyWsFactory);      // on the runtime Registry
```
```toml
[[mcp.servers]]
name = "remote"
kind = "websocket"     # your registered kind; empty ⇒ inferred (command→stdio, url→http)
url  = "wss://mcp.example.com"
```

`kind` maps internally to `Transport::Other { kind, params }`; `params` carries the
whole server entry (`command`/`args`/`env`/`url`/`headers`) for the factory to
interpret. To drive the client directly (no runtime), call
`agent_mcp::connect_tools(&transports, &server)` with your own `TransportRegistry`.
See the general [extension model](../extending.md).

## Server: expose the agent over MCP

`agent --serve-mcp` runs the agent *as* an MCP server over stdio, advertising a
single `run` tool (takes a goal, returns the final answer). Implemented in
[`agent-cli/src/mcp_server.rs`](../../crates/agent-cli/src/mcp_server.rs); it knows
only the public `Agent` API.

## Testing

`agent_testkit::mcp::ScriptedTransport` answers requests from a canned map, and
`agent_mcp::McpClient::with_transport` drives the client with it — no subprocess or
socket. See [testing](testing.md).
