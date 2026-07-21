# todo / plan tracking — the `TaskTracker` seam

A structured, inspectable agent plan: the model keeps an explicit todo list
instead of re-deriving "what's left" from the transcript every turn (which the
`ContextStrategy` seam is free to compact away). The `todo_write` tool writes the
plan up front, flips one item to `in_progress`, completes it, and the next turn
reads back an authoritative list. See parity spec
[`21-todo.md`](../parity/21-todo.md).

- **Trait:** `agent_core::TaskTracker` ([`agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs)) —
  `write(todos)` (atomic full-list replace), `update(patch)` (patch one item by
  `content`), `list()` (priority-ordered plan), `clear()`. Every mutation returns
  the resulting list; a rejected mutation leaves the store unchanged.
- **Types:** `Todo { content, status, priority }`; `TodoStatus ∈ {Pending,
  InProgress, Completed, Cancelled}` (`Pending`/`InProgress` are *open*);
  `TodoPriority ∈ {High, Medium, Low}` (also the `list()` sort key). Enums, not
  free strings — an unknown value is a precise error (`TodoStatus::parse` /
  `TodoPriority::parse` return `None`).
- **Impl crate:** [`agent-tasks`](../../crates/agent-tasks).
- **Shipped backend:** `memory` (`tasks-memory`) — a `Mutex<Vec<Todo>>`, kept
  priority-ordered (stable sort, so insertion order is preserved within a
  priority). Mutations validate on a copy and commit only if valid, so a rejected
  `write`/`update` never mutates. A `SessionStore`-backed backend (so a plan
  survives compaction / checkpoint / fork, parity spec 19) drops in behind the
  same trait as a follow-up.
- **Tool:** `todo_write` (`agent-tools`, `tool-todo`) — `{"todos": [...]}`
  replaces the plan; `{"update": {content, status?, priority?}}` patches one item.
  `parallel_safe() -> false` (mutates shared plan state). A single invalid item
  rejects the whole `write` (store unchanged).
- **Runtime feature:** `tasks` (default) — builds the `memory` tracker, meters it,
  and registers `todo_write`.
- **Config:** `[tasks] backend = "memory"`.

## Invariants (tighter than the peers)

- **At most one `in_progress`.** The agent works one step at a time; a
  `write`/`update` that would leave two `in_progress` is rejected atomically.
  Neither opencode nor hermes enforces this.
- **Typed enums.** Unknown `status`/`priority` strings are rejected with a clear
  error, rather than silently accepted as free strings (the peers accept any
  string).
- **Atomic full-list replace.** `write` swaps the whole list in one transaction —
  all-or-nothing — mirroring opencode's whole-list swap and its
  "deny leaves the store intact" guarantee.

## Observability (the differentiator)

No peer surfaces plan progress as a metric. Recorded by the `MeteredTasks`
decorator ([`agent-runtime/src/metered.rs`](../../crates/agent-runtime/src/metered.rs)):

- **Metrics:** `agent_tasks_open` / `agent_tasks_closed` **gauges**, refreshed on
  every `write`/`update`/`clear` — so open-vs-closed plan progress is graphable.
- **Tracing:** `tasks.write` / `tasks.update` / `tasks.clear` spans; the mutation
  spans carry `{op, total, in_progress, completed}` attributes.

## Tests, bench, leak

- **Backend:** table-driven `#[rstest]` in `agent-tasks` (write replace + priority
  order, single-item update, the single-`in_progress` invariant rejected
  atomically, unknown-content update, clear).
- **Tool:** `todo_write` table over the real in-memory tracker (write/ordering/
  enum-validation/atomicity/update), plus a `parallel_safe() == false` pin.
- **Observability:** a `MetricsProbe` gauge-delta test (open/closed reflect plan
  state across write + updates) and a `captured_spans` `tasks.write` assertion.
- **Bench:** none — plan mutation is a tiny `Vec` swap + validation loop (per the
  "iai only for a genuine CPU hot path" rule; documented skip).
- **Leak:** `tests/leak.rs` runs repeated write→update→list→clear cycles under
  dhat and asserts flat live blocks.

## Deferred (staged like the tokenizer / web seams)

- The `agent.v1.TaskService` gRPC worker (`agent --serve-tasks`) with reflection.
- `SessionStore`-backed persistence (parity spec 19), so the plan is checkpointed
  and forked with the session rather than held only for the process lifetime.
