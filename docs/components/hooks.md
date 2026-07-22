# Hooks

Typed lifecycle hooks fired from the agent loop. Parity spec
[22](../parity/22-hooks.md).

The loop is a fixed sequence — assemble → complete → authorize → dispatch →
record → compact. Anything cross-cutting that wants to *observe* or *intervene*
at those points otherwise has to be baked into the loop or a decorator, and that
does not scale: tracing sinks, custom guards, notifiers, and argument rewrites all
want the same attachment points without each patching `run_loop`.

## The seam

```rust
#[async_trait]
pub trait Hook: Send + Sync {
    fn name(&self) -> &str;
    async fn pre_turn(&self, working: &WorkingSet) {}
    async fn pre_tool(&self, call: &ToolCall) -> HookOutcome { HookOutcome::Continue }
    async fn post_tool(&self, call: &ToolCall, obs: &Observation) {}
    async fn post_turn(&self, message: &Message) {}
    async fn on_compact(&self, info: &CompactionInfo) {}
}
```

Every callback is **typed** — a hook is compile-checked against what it actually
receives, unlike an untyped event payload — and every one defaults to a no-op, so
a hook implements only the points it cares about.

Observation callbacks return nothing on purpose: **a hook must not be able to fail
the turn.** `pre_tool` is the single interventional point.

## The five attachment points

| Point | Fires |
|---|---|
| `pre_turn` | start of each iteration, before the model call |
| `pre_tool` | before an authorized call is dispatched — **can veto** |
| `post_tool` | after a tool produced an observation |
| `post_turn` | after the assistant message is recorded |
| `on_compact` | after the context strategy actually compacted |

`on_compact` only fires when compaction *changed* the working set, so a no-op
budget check doesn't generate noise.

## The veto, and what it cannot do

`pre_tool` runs **after** the `Policy` decision, so a hook can only ever *narrow*
permission — never widen it. A call the policy denied never reaches a hook, which
is asserted by a test. This ordering is the whole safety property: hooks are an
extension point, not an authorization bypass.

Within `pre_tool`, **first denial wins** and later hooks do not run — their side
effects would otherwise assume a call that never happens. A vetoed call produces
no observation, so `post_tool` does not fire for it.

## Ordering

Dispatch follows **config order**, so a guard can be placed ahead of an observer
and results are reproducible.

## Configuration

```toml
[hooks]
enabled = ["tracing"]   # empty (default) ⇒ no hooks, no per-turn cost
```

An unknown name **fails the build** rather than being ignored — silently
disabling observability an operator believes is on is worse than refusing to
start.

### Built-in: `tracing`

Emits a structured log line and a metric at each point: turn shape, tool
activity, and compaction deltas. This is the "observability sink" use case the
peers ship as a plugin (hermes' Langfuse hook, pi's event bus).

It deliberately **never vetoes** — a hook that both traces and blocks would make
the trace itself load-bearing, which is the wrong coupling.

## Observability

| Metric | Labels |
|---|---|
| `agent_hook_dispatches_total` | `hook`, `point` |

## Cost when unused

Every dispatch site short-circuits on an empty registry, so an agent with no
hooks configured pays nothing per turn.

## Deferred

**`hook.proto` / `HookService.Subscribe`** — the server-streaming event bus that
would let an external process (a dashboard, a recorder, a policy sidecar)
subscribe to the same lifecycle events without being in-process. The seam is
shaped for it; the transport is deferred consistently with specs 11–25.
