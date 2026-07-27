# 02 — Runtime split: Backend / Session / SessionManager

One agent process must run **N concurrent sessions over one shared backend**. Today
[`struct Agent`](../../../crates/agent-runtime/src/agent.rs) fuses two things: a
*shared backend bundle* (all `Arc<dyn>` seams + `Metrics` + config-derived
immutables) and *one process-wide identity* (`settings.session_id`, `settings.cwd`,
the single `event_sink`). This increment pulls the identity out of the bundle and
lets each session carry its own.

The loop body barely changes: `Agent::run_loop`
([`agent.rs`](../../../crates/agent-runtime/src/agent.rs)) already takes the mutable
per-conversation state (`working`, `budget`, `tool_ctx`, `tool_schemas`) by
reference and reads only immutable/shared things off `&self`. The work is ownership
plumbing, not loop logic.

## The split

### `struct Backend` (new — today's `Agent` minus identity)

All `Arc<dyn>` seams, `tools: ToolRegistry`, `metrics: Metrics`, `hooks`,
`system_fragments`, plus an immutable `RuntimeConfig` carved out of `Settings`
(model, temperature, max_tokens, max_iterations, context_window, reserve_output,
stream, mode knobs, review knobs, budgets). Holds **no** `session_id`, **no** single
`cwd`, **no** single `event_sink`. Gains a `SessionEventsRegistry`
([`03-hazards.md`](03-hazards.md)). All the `--serve-<seam>` accessor methods
(`provider()`, `memory()`, `context()`, `session_source()`, …) move here.

`Backend` is `Send + Sync` and shared as `Arc<Backend>` — every seam is
`Arc<dyn> + Send + Sync`, and `Metrics`/`ToolRegistry` are cheap `Clone`, so nothing
on it is exclusively borrowed. Distinct sessions touch it concurrently and freely.

### `struct Session` (rewrite — drop the `<'a>` borrow)

```rust
pub struct Session {
    backend: Arc<Backend>,          // was &'a Agent
    id: SessionKey,                 // new — (user, session)
    events: Arc<SessionEvents>,     // new — this session's sink (from the registry)
    metrics: SessionMetrics,        // new — per-session labeled recorder (06)
    working: WorkingSet,            // the transcript
    budget: TokenBudget,
    tool_ctx: ToolContext,          // per-session cwd (already here — good)
    tool_schemas: Vec<ToolSchema>,
    started: bool,
    pending_context: Vec<String>,
    current_mode: TaskMode,
    switch_history: VecDeque<TaskMode>,
    pending_switch: Option<(TaskMode, TaskMode)>,  // moved OUT of ModeAwareWindow (03)
    situational_present: bool,
}
```

Mechanical rewrites inside `send`/`send_inner`/`run_loop`:
`self.agent.X` → `self.backend.X`; the sites reading `self.agent.settings.session_id`
(the checkpoint call and the `record`/`record_usage`/`record_verification` event
stamps) → `self.id.session`; `self.agent.event_sink` → `self.events`;
`self.agent.metrics.<loop-level>` → `self.metrics.<...>` (the labeled view).

### `struct SessionManager` (new)

```rust
pub struct SessionManager {
    backend: Arc<Backend>,
    sessions: std::sync::Mutex<HashMap<SessionKey, Arc<tokio::sync::Mutex<Session>>>>,
}
```

- `get_or_create(spec: SessionSpec) -> Arc<tokio::Mutex<Session>>` — locks the outer
  `std::sync::Mutex` only to look up/insert the `Arc`, never across `.await`. A caller
  does `session.lock().await.send(goal).await`.
- The **per-session `tokio::Mutex` is held for the whole turn**, serializing turns
  *within* a session (correct — a conversation is single-threaded) while **different
  sessions run concurrently** on the shared `Backend`. A second goal for a busy key
  awaits that session's lock — natural per-session backpressure.
- `remove(key)` on session end → drops the `Session`, unregisters its `SessionEvents`
  ([`03-hazards.md`](03-hazards.md)), and retires its metric series
  ([`06-observability.md`](06-observability.md)).

`SessionSpec { user_id, session_id, workspace_root: Option<PathBuf> }`. When
`workspace_root` is absent the manager computes
`root = runtime_config.working_dir` and sets
`tool_ctx.cwd = SessionKey::path_under(root)?` — i.e.
`<working_dir>/<user_seg>/<session_seg>` — so per-session cwd buys per-session
confinement for free (the existing `confine()` already anchors `edit`/`read`/`write`/
`search` traversal to `tool_ctx.cwd`).

## Both entry paths go through the manager

`Agent` is **kept as a thin facade** so `grpc_server.rs`/`main.rs` call sites barely
change:

```rust
pub struct Agent { manager: SessionManager }        // wraps SessionManager + Arc<Backend>
impl Agent {
    pub async fn run(&self, goal: &str) -> Result<String> {
        self.manager.get_or_create(self.default_spec()).lock().await.send(goal).await
    }
    // serve-seam accessors delegate to self.manager.backend()
}
```

- **CLI / REPL (single session):** mints exactly one
  `SessionKey { user: "local", session: <process uuid> }` at startup and reuses it
  every `send`. One map entry, one events sink, one metric label set — **zero
  contention, zero measurable overhead** vs. today.
- **Portal `Send`-a-goal (many sessions):** the gRPC handler extracts
  `(user_id, session_id)` from metadata ([`01-identity.md`](01-identity.md)), calls
  `manager.get_or_create(spec)`, drives `send`. Concurrent goals for distinct keys
  run in parallel.

## Concurrency model — decision to settle here

| Option | Shape | Trade |
|---|---|---|
| **`Arc<Mutex<Session>>` map (recommended)** | manager owns the map; caller locks per turn | Minimal machinery, fits "low hundreds"; the default. |
| Actor-per-session | `mpsc` command channel + one spawned task per session | Owned isolation, per-session mailbox, clean cancellation for a long-running portal goal — the scaling path if needed later. |

Recommend the map for v1; note the actor as the documented upgrade path.

## What lands where

| File | Change |
|---|---|
| `crates/agent-runtime/src/agent.rs` | `Backend`/`Session`(owned)/`SessionManager`; `Agent` facade; `RuntimeConfig` carve-out; `record*` read `self.id.session`; per-session cwd; `agent.turn` gains `session_id`/`user_id` span attrs. |
| `crates/agent-runtime/src/builder.rs` | build a `Backend` (not an `Agent`-with-identity); the identity moves to session creation. |
| `crates/agent-runtime/src/git.rs` | fold `user` into the worktree path (`<worktrees_dir>/<user>/<session>/`) so the worktree tree mirrors the workspace tree. |
| `crates/agent-cli/src/{main.rs,repl.rs,grpc_server.rs}` | mint the single default `SessionKey`; drive sessions through the manager. |

## Tests

- `positive_` — two sessions send concurrently; each keeps its own transcript/mode;
  the single-session REPL path is unchanged (a snapshot/regression test on the
  existing behaviour).
- `corner_` — a second `send` for a busy session serializes (awaits the lock).
- `boundary_` — `remove` drops the session, its events sink, and its metric series.
- Confirm no measurable overhead on the single-session path (one map entry).
