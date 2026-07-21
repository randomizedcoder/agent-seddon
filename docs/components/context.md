# Context strategies — the `ContextStrategy` seam

Decides what messages the model sees each turn, and how the working window is
compacted when it grows past the token budget. Selected by `[agent] context`.

- **Trait:** `agent_core::ContextStrategy` ([`agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs))
- **Impl crate:** [`agent-context`](../../crates/agent-context)
- **Shipped:** `sliding-window` (drop oldest turns — lossy but free),
  `summarizing-window` (keep head + recent tail, replace the middle with an
  LLM-generated summary)
- **Cargo features:** `context-sliding-window` (default), `context-summarizing`

## The trait

```rust
#[async_trait]
pub trait ContextStrategy: Send + Sync {
    async fn assemble(&self, input: ContextInput) -> Result<Vec<Message>>;
    async fn compact(&self, working: &mut WorkingSet, budget: &TokenBudget) -> Result<()>;
}
```

`assemble` builds the initial model-ready message list from the system prompt,
injected [context files](runtime.md), recalled [memory](memory.md), and the goal.
`compact` must be **non-destructive** with respect to episodic memory — it only
trims the live working set; the durable log is never mutated.

## Design note: the factory context

Every seam factory takes one `FactoryCtx`, which carries the config, the shared
`Metrics`, and — where already built — the provider and tokenizer. The context
strategy is built after both, so `summarizing-window` can call `ctx.provider()?`
to summarize the dropped middle and either strategy can take `ctx.tokenizer()` to
budget with real counts. Strategies that need neither simply ignore them. Both
share the `assemble_messages`/`estimate_tokens` helpers in
[`agent-context/src/lib.rs`](../../crates/agent-context/src/lib.rs).

## Adding your own

In-tree: implement `ContextStrategy` in `agent-context` (gate behind a `context-*`
feature), register a factory + one line in `register_builtins`. Out-of-tree:
```rust
registry.context("map-reduce", |ctx| Ok(Arc::new(MapReduce::new(ctx.provider()?.clone()))));
```
Then `[agent] context = "map-reduce"`. See the general
[extension model](../extending.md).

## Testing

`agent_testkit::StaticContext` is a trivial assemble-and-never-compact double for
loop tests — see [testing](testing.md).
