# Context strategies — the `ContextStrategy` seam

Decides what messages the model sees each turn, and how the working window is
compacted when it grows past the token budget. Selected by `[agent] context`.

- **Trait:** `agent_core::ContextStrategy` ([`agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs))
- **Impl crate:** [`agent-context`](../../crates/agent-context)
- **Shipped:** `sliding-window` (drop oldest turns — lossy but free),
  `summarizing-window` (keep head + recent tail, replace the middle with an
  LLM-generated summary), `mode-aware-window` (summarizing-window + a reshape on a
  task-mode switch — see below)
- **Cargo features:** `context-sliding-window` (default), `context-summarizing`,
  `context-mode-aware`

## The trait

```rust
#[async_trait]
pub trait ContextStrategy: Send + Sync {
    async fn assemble(&self, input: ContextInput) -> Result<Vec<Message>>;
    async fn compact(&self, working: &mut WorkingSet, budget: &TokenBudget) -> Result<()>;

    // Optional capability (adaptive-cognition 02); default no-op / Budget.
    fn on_mode_switch(&self, from: TaskMode, to: TaskMode) {}
    fn last_compact_action(&self) -> CompactAction { CompactAction::Budget }
}
```

`assemble` builds the initial model-ready message list from the system prompt,
injected [context files](runtime.md), recalled [memory](memory.md), and the goal.
`compact` must be **non-destructive** with respect to episodic memory — it only
trims the live working set; the durable log is never mutated.

## Mode-aware compaction (`mode-aware-window`)

Context useful in one [task mode](mode.md) is noise in the next — the exploration
transcript that found the files is dead weight once you are *implementing* them.
`ModeAwareWindow` wraps `summarizing-window` and, when [mode detection](mode.md)
decides a switch, **reshapes regardless of budget**: it re-summarizes the demoted
middle **through the destination mode's lens** (a fixed, code-owned per-mode
prompt) while keeping the system head and the recent tail verbatim.

The switch reaches the strategy through the seam's two optional methods, not a new
`compact` signature: the runtime calls `on_mode_switch(from, to)` (right after it
records the switch), which *arms* the next `compact`; `last_compact_action()` lets
the [metered decorator](metrics.md) label the result. Both are default methods, so
`sliding-window`/`summarizing-window` are unaffected, and the `MeteredContext` /
`GrpcContext` decorators forward them (a downcast couldn't reach the inner
strategy). Over gRPC the switch rides the additive `from_mode`/`to_mode` fields on
`CompactRequest`, and `CompactStats` reports what happened.

**Fail-soft:** switch-lens summary → generic summary → drop the span, so the loop
always makes progress. The summary is **untrusted model text** about to become a
*system* message, so it is `scan_for_injection`-screened before insertion; a
flagged summary is dropped, never obeyed. Metrics:
`agent_context_switch_compactions_total{from,to}`,
`agent_context_tokens_shed{trigger}`,
`agent_context_summary_fallback_total{kind}`. Design:
[adaptive-cognition/02-compaction.md](../design/adaptive-cognition/02-compaction.md).

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
