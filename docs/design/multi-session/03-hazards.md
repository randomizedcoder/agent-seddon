# 03 — The two single-loop hazards

Two pieces of state live at `Agent` scope today but are *really* per-session. Under
one loop they are invisible; under N concurrent sessions they race or cross-wire.
Both must be fixed for multi-session correctness — this is not optional polish.

## Hazard A — `ModeAwareWindow` shared mutable state

[`ModeAwareWindow`](../../../crates/agent-context/src/mode_aware.rs) holds:

```rust
pending: Mutex<Option<(TaskMode, TaskMode)>>,   // the armed switch reshape
last_action: Mutex<CompactAction>,              // for the metered decorator's label
```

The `ContextStrategy` lives behind **one shared `Arc` on the backend**, so these two
`Mutex`es are **global across all sessions**. Session A arming a mode-switch reshape
(via `on_mode_switch`) and session B's `compact` reading `pending` would clobber each
other's armed switch and last-action telemetry. They are per-session directives
masquerading as strategy state.

### Fix (recommended): make the strategy stateless

Thread the directive *through* `compact` and delete the shuttle methods. Change the
[`ContextStrategy`](../../../crates/agent-core/src/lib.rs) trait:

```rust
async fn compact(
    &self,
    working: &mut WorkingSet,
    budget: &TokenBudget,
    switch: Option<ModeSwitch>,      // the armed (from,to), or None — was on_mode_switch
) -> Result<CompactAction>;          // was Result<()> — was last_compact_action
// DELETE on_mode_switch(...) and last_compact_action(...)
```

`Session` owns `pending_switch` ([`02-runtime-split.md`](02-runtime-split.md)), set by
`decide_switch` and consumed by the next `compact` call, and reads the returned
`CompactAction` directly for the metric. `ModeAwareWindow` becomes fully stateless: no
`Mutex`, no `pending`, no `last_action`. This **eliminates** the race (the strategy is
now an honestly-shared pure transform; the state belongs to the caller), and — unlike
the alternative below — it is also correct for a `context = "grpc"` deployment, where a
shared *remote* strategy would otherwise race identically.

**Ripple** (bounded, but real — the `ContextStrategy` seam boundary):
- `SummarizingWindow::compact` / `SlidingWindow::compact` — ignore `switch`, return
  `CompactAction::Budget`.
- `MeteredContext` decorator ([`metered.rs`](../../../crates/agent-runtime/src/metered.rs))
  — read the returned action instead of calling `last_compact_action()`.
- The gRPC **`ContextService`** — its `Compact` RPC/message already carries
  `from_mode`/`to_mode` (see [`context.proto`](../../../crates/agent-proto/proto/agent/v1/context.proto));
  fold those into the `switch` arg and add the returned `CompactAction` to the
  response. **Additive → no buf baseline bump.**

### Alternative (smaller blast radius): `fn fork()`

Add one defaulted trait method `fn fork(&self) -> Arc<dyn ContextStrategy>` that
returns `self` shared (correct for the stateless strategies), overridden by
`ModeAwareWindow` to hand each `Session` its own instance (fresh `Mutex`es,
`Arc`-shared heavy deps). `compact`/`on_mode_switch` signatures untouched → no
`ContextService` change. **Downside:** keeps the mutable-state design (just
per-session-instanced), and forking a `context = "grpc"` client strategy has no clean
remote semantics. Use only if the proto change is judged too costly this cycle.

**Decision to settle here:** stateless `compact(switch)` (recommended) vs. `fork()`.

## Hazard B — `SessionEvents` single broadcast

[`SessionEvents`](../../../crates/agent-runtime/src/session_events.rs) is one
`broadcast::Sender` + one `Mutex<StatusSnapshot>` — it assumes **one running loop**.
`AgentSessionService.Subscribe/Snapshot` carry no session selector, so with two
sessions a subscriber would see both interleaved and the snapshot would be whichever
loop wrote last.

### Fix: keyed registry + a selector on the RPC

Replace the single sink on the backend with

```rust
pub struct SessionEventsRegistry {
    map: RwLock<HashMap<SessionId, Arc<SessionEvents>>>,
}
// get_or_create(id, context_window) · get(id) · remove(id) · keys()
```

`SessionEvents` itself is **unchanged** — still one broadcast + one snapshot, but now
**one per live session**, minted by the manager at session creation and dropped at
`remove` ([`02-runtime-split.md`](02-runtime-split.md)). `Session` holds its own
`Arc<SessionEvents>` directly, so the hot token-delta path does no per-publish map
lookup.

The service ([`agent-grpc/src/server/agent_session.rs`](../../../crates/agent-grpc/src/server/agent_session.rs))
holds an `Arc<SessionEventsRegistry>` (behind a `SessionRegistry`-style trait to keep
the crate boundary, as today's `Arc<dyn SessionSource>`). `Subscribe`/`Snapshot` gain
`string session_id = 1;`:

- `registry.get(&session_id)` → the per-session source; `NOT_FOUND` if absent.
- **Back-compat:** empty `session_id` with exactly one live session returns that one
  (keeps the single-session observer/REPL path selector-free); empty with many →
  `INVALID_ARGUMENT`.
- Optional `ListSessions` RPC over `registry.keys()` for the portal's session picker.

Proto change is **additive** (a new request field + optional new RPC) → no baseline
bump.

## What lands where

| File | Change |
|---|---|
| `crates/agent-core/src/lib.rs` | `ContextStrategy::compact(switch) -> CompactAction`; delete `on_mode_switch`/`last_compact_action`; `ModeSwitch` type. |
| `crates/agent-context/src/{mode_aware.rs,summarizing.rs,sliding.rs}` | stateless `compact`; drop the `Mutex`es. |
| `crates/agent-runtime/src/metered.rs` | `MeteredContext` reads the returned action. |
| `crates/agent-runtime/src/session_events.rs` | `SessionEventsRegistry`; per-session mint/retire. |
| `crates/agent-proto/proto/agent/v1/{context,agent_session}.proto` | `switch`/action on Compact; `session_id` selector (+ optional `ListSessions`). |
| `crates/agent-grpc/src/{server,client}/{context,agent_session}.rs` | wire the new fields/selector. |

## Tests

- `adversarial_`/`corner_` — two sessions compact concurrently; each sees only its own
  armed switch (the race is gone); a subscriber for session A never receives session
  B's events.
- `positive_` — single-session `Subscribe`/`Snapshot` with empty selector still works.
- `boundary_` — empty selector with >1 live session → `INVALID_ARGUMENT`; `Subscribe`
  for an unknown session → `NOT_FOUND`.
