# Providers — the `LlmProvider` seam

Wraps a model behind a uniform request/response so the loop never touches HTTP or a
provider's on-the-wire format. Selected by `[agent] provider`.

- **Trait:** `agent_core::LlmProvider` ([`agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs))
- **Impl crate:** [`agent-providers`](../../crates/agent-providers)
- **Shipped:** `openai-compat` (OpenAI-compatible `/chat/completions` — covers GLM,
  vLLM, Ollama, OpenAI), `anthropic` (native Messages API)
- **Cargo features:** `provider-openai-compat` (default), `provider-anthropic`

## The trait

```rust
#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn capabilities(&self) -> ModelCapabilities;
    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse>;
    async fn stream(&self, req: CompletionRequest) -> Result<ChunkStream> { /* default */ }
}
```

Each impl owns its own conversion between the shared message types
(`Message`, `ToolCall`, `ToolSchema`) and the provider's wire format, including
parsing tool calls out of the response.

## Streaming

`complete` (buffered) is the only required method. `stream` defaults to running
`complete` and emitting the text, each tool call, and a terminal chunk — so a new
provider gets streaming "for free" and only overrides `stream` to do real
server-sent-events incremental output (both shipped providers do).

## Adding your own

In-tree:
1. `crates/agent-providers/src/<name>.rs` implementing `LlmProvider`; gate + re-export in `lib.rs`.
2. Declare the `provider-<name>` feature in `agent-providers/Cargo.toml`.
3. Factory in `agent-runtime/src/builder.rs` + one line in `register_builtins`.
4. Forward the feature in `agent-runtime/Cargo.toml`.

Out-of-tree (no fork):
```rust
registry.provider("my-llm", |ctx| Ok(Arc::new(MyProvider::new(ctx.cfg.provider.model.clone())?)));
```
Then `[agent] provider = "my-llm"`. See the general
[extension model](../extending.md) and
[`examples/custom_provider.rs`](../../crates/agent-cli/examples/custom_provider.rs).

## Testing

`agent_testkit::ScriptedProvider` (replay a fixed response sequence) and
`FnProvider` (compute the response from the request) fake this seam without a
network — see [testing](testing.md).
