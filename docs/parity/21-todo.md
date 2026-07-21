# Parity spec 21 — todo / plan tracking

Per-feature parity spec for a structured, inspectable agent plan: a new
`TaskTracker` seam plus a `todo_write` tool that maintains a list of todos with a
status (pending / in_progress / completed / cancelled) and a priority
(high / medium / low), so the model carries an explicit multi-step plan it can
read back and revise.

> **Status: implemented** (seam + in-memory backend + tool + observability +
> leak). New **`TaskTracker` seam** (an `async` trait in `agent-core`: `write` /
> `update` / `list` / `clear`) with a concrete `memory` impl in
> [`agent-tasks`](../../crates/agent-tasks), wired by the builder and exposed as a
> `todo_write` **`Tool`** in [`agent-tools`](../../crates/agent-tools/src/todo.rs).
> **Differentiator landed:** *metered plan progress* — `agent_tasks_open` /
> `agent_tasks_closed` Prometheus gauges refreshed on every mutation, plus
> `tasks.write`/`update`/`clear` OTel spans carrying `{op, total, in_progress,
> completed}`. Tighter than the peers: typed enums (unknown status/priority
> rejected), **at-most-one-`in_progress`** invariant, atomic full-list replace.
> **Deferred to a follow-up** (staged like the tokenizer / web seams): the
> `agent.v1.TaskService` gRPC worker (`agent --serve-tasks`) and `SessionStore`
> persistence (parity spec [19](19-session-checkpoint.md) — until it lands the
> plan is per-session in-memory). See
> [`docs/components/tasks.md`](../components/tasks.md).

## Feature & why it matters

A coding agent that can only hold its plan in the transcript re-derives "what's
left" every turn from prose, and silently drops steps as the window compacts. An
explicit todo list turns the plan into first-class, mutable state: the model
writes the plan up front, flips one item to `in_progress`, completes it, and the
next turn reads back an authoritative list instead of re-reading its own earlier
messages. This measurably improves multi-step reliability — the model stops
losing track of the tail of a long task — and it makes the plan *inspectable*: a
human (or a supervising process) can see exactly what the agent thinks remains.

The two behaviours worth getting right are (a) the **status/priority state
model** and its legal transitions, and (b) **atomic full-list replacement** — the
tool takes the whole desired list and swaps it in, rather than mutating items
in place, which keeps the store consistent even if the model rewrites the plan
wholesale. Our differentiator layers observability on top: the count of open
(pending + in_progress) vs. closed (completed + cancelled) todos becomes a gauge,
so plan progress is a graphable metric.

## agent-seddon today

**Absent.** There is no structured todo, plan, or task-list tool, and no
`TaskTracker` seam. The model's only "plan" today is whatever prose it keeps in
the working message set, which the `ContextStrategy` seam is free to compact away.

The pieces this spec builds on already exist:

- **`Tool` trait** — [`crates/agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs)
  (`trait Tool`, ~line 249: `name`, `schema`, `execute`, defaulted
  `parallel_safe()` ~line 258; `ToolSchema` ~line 215, `Observation` ~line 223).
  `todo_write` is a `Tool` like the file tools in
  [`crates/agent-tools`](../../crates/agent-tools/src/) (see `edit.rs`,
  `search.rs`), registered by
  [`agent-runtime/src/registry.rs`](../../crates/agent-runtime/src/registry.rs)
  (`register_builtins`) behind a cargo feature. It should override
  `parallel_safe() -> false` (it mutates shared plan state, must not interleave).
- **Persistence** — the todo list is stored per session via the **`SessionStore`
  seam** introduced in parity spec [19](19-session-checkpoint.md); the
  `TaskTracker` impl reads/writes the current session's todo blob through it, so a
  plan is checkpointed and forked with the session rather than kept in a private
  file. Until #19 lands, the impl can back onto an in-memory `Mutex<Vec<Todo>>`
  behind the same trait (the seam is the contract; the backing store is swappable).
- **Observability plumbing** — `agent-metrics` (+ `metered.rs` decorators) and
  `agent-telemetry` spans, asserted in tests via `agent-testkit`'s
  [`observe::MetricsProbe`](../../crates/agent-testkit/src/observe.rs) /
  `captured_spans`.

## Peer implementations & their tests

| Peer     | Impl path | Test path | Framework |
| -------- | --------- | --------- | --------- |
| opencode | `packages/schema/src/session-todo.ts` (schema) + `packages/core/src/tool/todowrite.ts` (tool) | `packages/core/test/tool-todowrite.test.ts`, `packages/core/test/session-todo.test.ts` | bun:test + `effect` (`testEffect`) |
| hermes-agent | `tools/kanban_tools.py` (tool surface) + `hermes_cli/kanban_db.py` (store) | `tests/tools/test_kanban_tools.py`, `tests/hermes_cli/test_kanban_db.py` | pytest + `monkeypatch` |
| pi       | — (no structured todo/plan tool) | — | — |

**pi** ships no todo/plan/task-list tool — `rg -i 'todo|task.?list|plan'` under
`packages/coding-agent/src/` returns only vendored highlight.js and changelog
hits, no tool. It is intentionally absent from the table.

**opencode — `session-todo.ts` + `tool-todowrite.test.ts`** (the anchor). The
schema (`SessionTodo.Info`) is exactly `{ content, status, priority }` with
`status ∈ {pending, in_progress, completed, cancelled}` and
`priority ∈ {high, medium, low}`; a `todo.updated` event carries
`{ sessionID, todos: Info[] }`. The tool test asserts:

- *registers, approves the wildcard resource, persists, returns typed output* —
  `toolDefinitions(registry)` lists exactly `todowrite`; calling it with
  `[{content:"Implement slice", status:"in_progress", priority:"high"}]` returns a
  **typed output** `{ structured: { todos }, content:[{type:"text", …}] }` whose
  text value is the pretty-printed JSON of the list; the permission layer sees one
  `{action:"todowrite", resources:["*"], save:["*"]}` assertion; and
  `service.get(sessionID)` afterwards **equals the written list** (full-list
  replace, persisted).
- *does not update persisted todos when permission is denied* — with an existing
  persisted list `[{keep, pending, low}]`, a denied call returns
  `{type:"error", value:"Unable to update todos"}` and `service.get` **still
  returns the old list** (deny leaves the store untouched, no partial write).

**hermes-agent — `kanban_tools.py` + `test_kanban_tools.py`** — a heavier "kanban"
task board (a SQLite `kanban.db` via `kanban_db.py`) rather than a per-session
todo list. The tool surface is **gated**: a normal `hermes chat` session sees
**zero** `kanban_*` tools in its schema; only a dispatcher-spawned worker
(`HERMES_KANBAN_TASK` set) or a profile with the `kanban` toolset gets task
lifecycle tools. The tests assert the gating (hidden without the env var, visible
with it), each handler's happy path, and structured-JSON error paths (missing
required args, bad metadata type). It is a useful second data point for
status-lifecycle + structured errors, but its board/worker model is out of scope
here — we port the *per-session todo* shape from opencode.

## Completeness gaps

Behaviour agent-seddon must add to be the most complete (spec only — do **not**
implement here):

- **`TaskTracker` trait — full CRUD-ish surface.** `write(todos)` (atomic
  full-list replace), `update(id, patch)` (incremental single-item status/priority
  change), `list()` (current plan, priority-ordered), `clear()` (empty the plan).
  `write` is the primary path (mirrors opencode's whole-list swap); `update` is the
  incremental convenience.
- **Status + priority model.** `status ∈ {Pending, InProgress, Completed,
  Cancelled}`, `priority ∈ {High, Medium, Low}`, as typed enums (not free strings),
  deserialized from the tool's JSON args; an unknown status/priority string is
  **rejected** with a clear error (peers accept free strings — we tighten this).
- **Atomic full-list replace vs. incremental update.** `write` replaces the entire
  list in one transaction (all-or-nothing: a single invalid item rejects the whole
  call, store unchanged — mirrors the opencode deny-leaves-store-intact assertion);
  `update` touches exactly one item and leaves siblings untouched.
- **Validation — at most one `in_progress`.** Enforce the plan invariant that only
  one todo is `in_progress` at a time (the agent works one step at a time); a
  `write`/`update` that would leave two `in_progress` is rejected. (Neither peer
  enforces this — a genuine correctness improvement.)
- **Persistence via `SessionStore`.** The list is keyed by session and persisted
  through the spec-19 `SessionStore`, so it survives compaction, checkpoint, and
  fork — not held only in the transcript.
- **Metrics gauge — plan progress.** An `agent_tasks_open`/`agent_tasks_closed`
  gauge pair (or one gauge labelled `state="open"|"closed"`) updated on every
  `write`/`update`/`clear`, so open-vs-closed plan progress is graphable; plus an
  OTel `tasks.write` span carrying `{op, total, in_progress, completed}` attributes
  (the #44 span-attribute pattern). No peer surfaces plan progress as a metric.

Each maps to a case below.

## Table-driven test plan

New `#[rstest]` table in the `todo_write` tool module
(`crates/agent-tools/src/todo.rs`), mirroring the `edit.rs` shape: a `Todo`
tracker is created over a fresh backing store (in-memory `TaskTracker` double, or
the spec-19 `SessionStore` fixture from `agent-testkit`); `args` drives one
`todo_write.execute`; `Ok(list)` ⇒ the tracker's `list()` equals `list`,
`Err(substr)` ⇒ the observation is an error containing `substr`. The metrics/span
assertions live in sibling tests that hold a `Metrics` handle and a
[`MetricsProbe`](../../crates/agent-testkit/src/observe.rs).

**Prefixes:** `positive_` (happy path), `negative_` (rejected), `corner_`
(odd-but-valid), `boundary_` (empty / limits). Tags: `(port: opencode|hermes)` when
analogous to a named peer test, `(new: agent-seddon)` otherwise.

```rust
use agent_core::{Observation, Result, Tool, ToolContext};
use agent_testkit::{tempdir, observe::MetricsProbe};
use rstest::rstest;
use serde_json::{json, Value};

// A todo is `{content, status, priority}`; a plan is an ordered list.
// `Ok(&str)` here is the JSON the tracker's `list()` should serialize to
// (priority-ordered, pretty). `Err(&str)` is an error substring.

#[rstest]
// --- write: full-list replace, persisted -----------------------------------
#[case::positive_write_single_in_progress(
    json!({"todos": [{"content": "Implement slice", "status": "in_progress", "priority": "high"}]}),
    Ok(r#"[{"content":"Implement slice","status":"in_progress","priority":"high"}]"#))] // (port: opencode)
#[case::positive_write_replaces_whole_list(
    // pre-seed [{a,pending,low}]; writing [{b,completed,high}] replaces it wholesale
    json!({"todos": [{"content": "b", "status": "completed", "priority": "high"}]}),
    Ok(r#"[{"content":"b","status":"completed","priority":"high"}]"#))] // (port: opencode)
// --- priority ordering ------------------------------------------------------
#[case::corner_priority_ordering(
    json!({"todos": [
        {"content": "lo", "status": "pending", "priority": "low"},
        {"content": "hi", "status": "pending", "priority": "high"},
        {"content": "me", "status": "pending", "priority": "medium"}]}),
    Ok(/* list() returns hi, me, lo — high→medium→low */
       r#"[{"content":"hi",…"high"},{"content":"me",…"medium"},{"content":"lo",…"low"}]"#))] // (new: agent-seddon)
// --- status transition via update ------------------------------------------
#[case::positive_update_transitions_status(
    // pre-seed [{x,pending,high}]; update x -> in_progress
    json!({"update": {"content": "x", "status": "in_progress"}}),
    Ok(r#"[{"content":"x","status":"in_progress","priority":"high"}]"#))] // (port: opencode)
// --- validation: enums ------------------------------------------------------
#[case::negative_invalid_status_rejected(
    json!({"todos": [{"content": "x", "status": "frobnicate", "priority": "high"}]}),
    Err("invalid status"))] // (new: agent-seddon)  peers accept free strings; we reject
#[case::negative_invalid_priority_rejected(
    json!({"todos": [{"content": "x", "status": "pending", "priority": "urgent"}]}),
    Err("invalid priority"))] // (new: agent-seddon)
// --- validation: single in_progress invariant ------------------------------
#[case::negative_two_in_progress_rejected(
    json!({"todos": [
        {"content": "a", "status": "in_progress", "priority": "high"},
        {"content": "b", "status": "in_progress", "priority": "low"}]}),
    Err("only one todo may be in_progress"))] // (new: agent-seddon)
// --- atomicity: one bad item rejects the whole write, store unchanged -------
#[case::negative_write_atomic_no_partial(
    // pre-seed [{keep,pending,low}]; a two-item write with a bad 2nd item
    json!({"todos": [
        {"content": "ok", "status": "pending", "priority": "high"},
        {"content": "bad", "status": "???", "priority": "high"}]}),
    Err("invalid status"))] // store must still be [{keep,pending,low}] // (port: opencode deny-intact)
// --- boundary: empty list clears the plan ----------------------------------
#[case::boundary_empty_list_clears(
    json!({"todos": []}),
    Ok("[]"))] // (new: agent-seddon)
#[tokio::test]
async fn todo_write_cases(
    #[case] args: Value,
    #[case] expected: std::result::Result<&str, &str>,
) { /* build tracker (+ optional pre-seed), run todo_write.execute, assert list()/error */ }
```

Sibling observability tests (hold a `Metrics` + `MetricsProbe`, distinct
signature):

```rust
// --- metrics: the open/closed gauge reflects plan state --------------------
#[tokio::test]
async fn todo_write_meters_plan_progress() {
    let metrics = agent_metrics::Metrics::new();
    let probe = MetricsProbe::new(&metrics);
    // write [{a,in_progress,high},{b,pending,low},{c,completed,high}]
    // → open = 2 (in_progress + pending), closed = 1 (completed)
    // run the metered todo_write …
    assert_eq!(probe.delta(&metrics, "agent_tasks_open",   Some("")), 2.0);   // (new: agent-seddon)
    assert_eq!(probe.delta(&metrics, "agent_tasks_closed", Some("")), 1.0);
    // then update c-> cancelled and a-> completed: open drops to 1, closed rises to 2
}

// --- span: tasks.write is emitted with plan attributes ---------------------
#[test]
fn todo_write_emits_span() {
    let spans = agent_testkit::observe::captured_spans(|| { /* run write */ });
    assert!(spans.iter().any(|s| s == "tasks.write")); // (new: agent-seddon)
}

// --- deny leaves the store intact (once a Policy gate wraps the tool) -------
// (port: opencode "does not update persisted todos when permission is denied"):
// a denied todo_write returns an error observation AND tracker.list() is unchanged.
```

Case-prefix key: `positive_` succeeds, `negative_` rejects, `corner_`
odd-but-valid (ordering), `boundary_` empty/limit edges. `(port: …)` names the peer
the case came from; `(new: agent-seddon)` marks cases with no peer origin (the enum
validation, single-`in_progress` invariant, priority ordering, and the metrics
gauge are all novel).

### Harness obligations

The implementing PR must satisfy the standard #21–45 checklist:

- **Seam + registry + tool.** New `TaskTracker` async trait in `agent-core`
  (`write`/`update`/`list`/`clear`); concrete impl in a sibling crate behind a
  cargo feature; one factory line in `agent-runtime/src/registry.rs`
  (`register_builtins`, config-selected); the `todo_write` `Tool` in
  `crates/agent-tools` (`parallel_safe() -> false`). Doc in
  `docs/components/tasks.md`.
- **Proto + gRPC.** `crates/agent-proto/proto/agent/v1/tasks.proto`
  (`Write`/`Update`/`List`/`Clear` RPCs over a `Todo` message) + `build.rs` entry +
  server/client in `agent-grpc` + `--serve-tasks` + **reflection**; commit the
  `buf.image.binpb` bump (`nix run .#buf-image`); add endpoint constants to
  `nix/constants.nix` (`nix run .#gen-constants`); extend
  `agent-grpc/tests/roundtrip.rs`.
- **Metrics + OTel.** `agent_tasks_open` / `agent_tasks_closed` **gauge** (updated
  on every mutation) in `agent-metrics`, a metered decorator in
  `agent-runtime/src/metered.rs`, and a `tasks.write` span carrying
  `{op, total, in_progress, completed}` attributes.
- **Bench.** Plan mutation is tiny (a `Vec` swap + validation loop) — **likely no
  iai bench**; document the skip (per the plan's "iai only for a genuine CPU hot
  path" rule). If validation/priority-sort ever grows non-trivial, add one with an
  Ir ceiling in `nix/checks/bench.nix`.
- **Leak.** A dhat `tests/leak.rs` (iteration-based, `dhat-heap` feature) over the
  **`update` path** (repeated write→update→clear cycles free everything and stay
  under an allocation budget).
- **Observability assertion.** The `MetricsProbe` gauge delta + `captured_spans`
  `tasks.write` assertions above are part of the test suite.

## References

- **agent-seddon:** `Tool` / `ToolSchema` / `Observation` / `parallel_safe` in
  [`crates/agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs); tool style
  in [`crates/agent-tools/src/edit.rs`](../../crates/agent-tools/src/edit.rs),
  [`crates/agent-tools/src/search.rs`](../../crates/agent-tools/src/search.rs);
  registration in
  [`crates/agent-runtime/src/registry.rs`](../../crates/agent-runtime/src/registry.rs)
  (`register_builtins`); persistence via the `SessionStore` seam of parity spec
  [19](19-session-checkpoint.md); observability doubles in
  [`crates/agent-testkit/src/observe.rs`](../../crates/agent-testkit/src/observe.rs)
  (`MetricsProbe`, `captured_spans`); gRPC round-trip style in
  [`crates/agent-grpc/tests/roundtrip.rs`](../../crates/agent-grpc/tests/roundtrip.rs).
- **opencode:** `packages/schema/src/session-todo.ts`,
  `packages/core/src/tool/todowrite.ts`,
  `packages/core/test/tool-todowrite.test.ts`,
  `packages/core/test/session-todo.test.ts`.
- **hermes-agent:** `tools/kanban_tools.py`, `hermes_cli/kanban_db.py`,
  `tests/tools/test_kanban_tools.py`, `tests/hermes_cli/test_kanban_db.py`.
- **pi:** no structured todo/plan tool (`—`).
