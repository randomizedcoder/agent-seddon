# Task-mode detection — the `TaskClassifier` seam

Detects which **task mode** the current work is — every turn — so the loop can
switch to a more appropriate mode. It is a general, always-on capability (it began
inside the review flow as a review trigger; it now lives on its own). The mode drives
the in-loop [review](../design/code-review/README.md) hand-off today, and
mode-aware compaction + dimensional memory in later increments
([`docs/design/adaptive-cognition/`](../design/adaptive-cognition/README.md)).

- **Trait:** `agent_core::TaskClassifier` ([`agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs))
- **Impl crate:** [`agent-mode`](../../crates/agent-mode) (`HybridClassifier`)
- **Shipped:** `hybrid` (free deterministic prefilter → cheap light-tier pool vote),
  `grpc` (dial a remote `ModeService`)
- **Cargo feature:** `mode` (default on; `review` implies it)
- **Selected by:** `[mode] classifier` (`hybrid` | `grpc` | `""` off)

## The trait

```rust
pub enum TaskMode { Review, Implement, Design, Debug, Explain, Other }

#[async_trait]
pub trait TaskClassifier: Send + Sync {
    fn name(&self) -> &str;
    async fn classify(&self, ctx: &ClassifyCtx<'_>) -> ModeVerdict;   // fails safe → Other
}
```

`ClassifyCtx { prompt, history }` is borrowed so the runtime builds it cheaply each
turn. `ModeVerdict { mode, confidence, reason }` — `confidence` is untrusted (a pool
vote's self-report) and clamped before use.

## How detection works

Two stages, cheapest first:

1. **Deterministic prefilter (free, always).** High-precision keyword cues settle the
   clear cases across the whole taxonomy (a review phrase → `Review`, an implement verb
   → `Implement`, a failure cue → `Debug`, …). Everything else is *ambiguous*.
2. **Pool vote (only when ambiguous, only if a pool is wired).** Fan out to the
   **light** tier ([LLM pool](../design/code-review/llm-pool.md)) and ask for a
   one-word mode label; take the plurality, clamp the confidence. A dead pool or no
   pool falls through to a fail-safe `Other`.

With no `[pool]` configured (the default), only the free prefilter runs — so per-turn
mode detection is near-zero cost.

## Switching (runtime)

The classifier only *labels* a turn; the **switch decision** lives in the runtime
(`Session` in [`agent-runtime/src/agent.rs`](../../crates/agent-runtime/src/agent.rs)).
Each turn it classifies (history-aware), then with **hysteresis** decides whether to
move the session mode: a candidate must reach `[mode] confidence_floor`, and either a
*decisive* deterministic hit (which switches immediately) or the same mode winning
`[mode] hysteresis` consecutive turns (which guards against thrash). A decided switch
emits a `ModeSwitch`, updates `current_mode`, and is recorded (a metric, a
`mode.switch` span, and an episodic `mode_switch` event → ClickHouse `agent_events`).
**Fail-safe:** any uncertainty leaves the mode unchanged — a switch shapes context, it
never widens tool or policy scope.

## Config

```toml
[mode]
classifier       = "hybrid"   # "hybrid" | "grpc" | "" (off)
confidence_floor = 0.6        # a candidate must reach this to be considered
hysteresis       = 2          # consecutive turns a non-decisive candidate must win
```

## CLI

`agent --detect-mode "<prompt>"` classifies a single prompt and prints the verdict
(`mode=… confidence=… reason=…`) — an offline debug surface (the deterministic
prefilter needs no model).

## gRPC

`agent --serve-mode` hosts the classifier as `ModeService.Classify` on the `mode`
endpoint (`nix/constants.nix`); a `[mode] classifier = "grpc"` client dials it. Wire
failure is fail-safe: a transport error resolves to `Other`, never a spurious switch.

## Metrics

`agent_mode_classifications_total{mode,via}`, `agent_mode_switches_total{from,to}`,
`agent_mode_switch_confidence` (histogram).

## Adding your own

Implement `TaskClassifier` in `agent-mode` (or a new crate), register it in
`register_builtins` / the builder, and select it with `[mode] classifier`. See
[`extending.md`](../extending.md).
