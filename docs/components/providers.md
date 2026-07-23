# Providers — the `LlmProvider` seam

Wraps a model behind a uniform request/response so the loop never touches HTTP or a
provider's on-the-wire format. Selected by `[agent] provider`.

- **Trait:** `agent_core::LlmProvider` ([`agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs))
- **Impl crate:** [`agent-providers`](../../crates/agent-providers)
- **Shipped:** `openai-compat` (OpenAI-compatible `/chat/completions` — covers GLM,
  vLLM, Ollama, OpenAI), `anthropic` (native Messages API)
- **Cargo features:** `provider-openai-compat` (default), `provider-anthropic`

## Configuring an endpoint

```toml
[provider]
base_url = "http://localhost:11434/v1"   # Ollama's OpenAI-compatible endpoint
model    = "llama3.1:latest"
api_key  = "ollama"
```

Three things bite people, in the order they hit them:

- **A key is always required.** Resolution is `api_key` (inline) > `api_key_env` >
  `api_key_file`, and if all three are empty the build fails with *"no API key"* —
  even for a local server that ignores it. Pass a placeholder.
- **The model must support tool calling.** The loop cannot act without it. The
  symptom of a model that lacks it is an agent that answers in prose and never
  touches a file. `llama3.1`, `llama3.3` and `mistral-nemo` work; many small models
  do not — and see the Ollama caveat below, because "supports tools" is necessary
  but not sufficient.
- **On Ollama, prefer a llama-family model.** Ollama's OpenAI-compatible endpoint
  (`/v1/chat/completions`, which this provider speaks) does **not** apply some
  models' tool-call parsers, even when the model emits a perfectly good tool call.
  Observed with `qwen3-coder` and `qwen2.5-coder`: the model returns
  `<function=…><parameter=…>…</function>`, but Ollama hands it back as plain
  `content` with `tool_calls: null`, so the agent sees prose and writes nothing.
  Ollama's *native* `/api/chat` endpoint parses it; the OpenAI-compat one does not.
  `llama3.x` and `llama3-groq-tool-use` use a format Ollama's OpenAI path *does*
  translate, so they work. Test any model on a task whose result you can check
  rather than trusting that it advertises `tools`.
- **`[agent] context_window` must match the model**, not the largest model you
  once used. It is the budget compaction works against, so overstating it means
  the request overflows the real window rather than being compacted.

`insecure_tls = true` skips TLS certificate verification. It is **off by default**
and should stay off: it disables the check that the endpoint is who it claims to
be, so it is only defensible for a self-signed development endpoint you control
and reach over a trusted network. Prefer trusting the CA.

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
