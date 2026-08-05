# Parity spec 31 — sub-agent orchestration graph

Per-feature parity spec for a **`Subagent` / `AgentGraph` seam**: a first-class,
inspectable **orchestration graph** of running child agents — `spawn` a child and
get back a **handle**, `wait` on it, `send` it more input, `interrupt` / `resume`
/ `close` it, and `list` the live tree — instead of today's fire-and-wait
`delegate` that can only run a child to completion and hand back its final
summary. The unit of work stops being "one boomerang round-trip" and becomes a
**live parent/child graph** the parent (and an operator) can steer.

> **Status: ⬜ spec written, not started.** Proposed new **`Subagent` seam** (an
> `async` trait in `agent-core`, with a companion `AgentGraph` handle/registry
> store), impl in a new sibling crate **`agent-subagents`** behind the existing
> `subagents` cargo feature, selected by config key **`agent.subagent_backend`**
> (`"boomerang"` = today's fire-and-wait `DelegateTool`, kept as the default;
> `"graph"` = the new handle-based orchestrator). Nothing here is implemented:
> today's `crates/agent-runtime/src/subagent.rs` `DelegateTool` is the *only*
> sub-agent surface, and it has no handle, no send/interrupt/resume, and no
> graph. **Differentiator:** no peer — codex included — exposes a sub-agent
> graph that is simultaneously a *distributed* seam (gRPC + reflection,
> `--serve-subagents`), *benchmarked* (iai-callgrind on the graph store), *leak-
> tested* (dhat over spawn→wait→close), and *metric + span instrumented* (live
> child gauge, per-spawn span). **Deferred:** cross-agent message-passing
> between siblings (codex's `send_message` mailbox — a routing layer on top of
> the graph), a persisted graph store (codex's `agent-graph-store` on-disk
> parent/child edges — this spec keeps the graph in-memory first), and running
> each child inside its own isolation backend (spec 14) or as a fully remote
> session (folding into the multi-session `SessionManager`).

## Feature & why it matters

agent-seddon can already **delegate**: `delegate { goal, tools? }` builds a fresh
child `Agent`, runs its own tool loop in an isolated context, and returns only the
child's final summary — the "boomerang" pattern from DESIGN.md §4.5. That is
exactly right for a **well-scoped, one-shot** subtask ("summarise this module",
"find where X is configured") where the parent wants a clean answer and none of
the child's intermediate noise.

But `delegate` is **fire-and-wait**: the parent's tool call blocks on
`child.run(goal).await` and the child is *gone* the instant it returns. That model
is structurally unable to express the class of work that needs an **orchestration
graph**:

- **Fan-out with a handle.** Spawn three explorers over three sub-trees, keep
  *all three handles*, and `wait` on them as they finish — rather than blocking on
  one child at a time. The parent holds identity for each running child.
- **Long-running background children.** Launch a child to grind on an independent
  task, return to the user immediately, and be **notified when it finishes** —
  the child outlives the single tool call that started it.
- **Steering a running child.** `send` a correction or extra context to a child
  mid-run ("also check the tests"), instead of only being able to phrase the whole
  task up front and hope.
- **Interrupt / resume / close.** Stop a child that has gone off the rails
  (`interrupt`), let it be picked back up later (`resume`), or tear it and its
  whole subtree down (`close`) — with the parent able to `list` the live graph
  and see each node's status.

The unit of work is a **graph**, not a call: `spawn` returns a `ChildId` + handle;
the parent then `wait`/`send`/`interrupt`/`resume`/`close`/`list`s against that
handle. Recursion is bounded by `max_depth` (already true of `delegate`), and the
number of concurrently live children is bounded by a **spawn cap** so a runaway
parent can't fork unbounded agents. That bound, and the fact that a child is now a
*retained, queryable node* rather than a transient `await`, is the whole reason
this is a **seam with a registry** and not just a fancier tool.

## agent-seddon today

**A sub-agent tool exists, but it is fire-and-wait — there is no graph.** The
entire surface is [`crates/agent-runtime/src/subagent.rs`](../../crates/agent-runtime/src/subagent.rs):

- **`DelegateTool` is a boomerang.** `Tool::name() == "delegate"`, args
  `{ goal, tools? }`. `execute()` builds a child `Agent` from a shared
  `SubagentContext` (provider / context / policy / memory / metrics / a
  `worker_tools` registry / `child_settings`) and does exactly
  `child.run(goal).instrument(span).await`, returning the child's final answer as a
  single `Observation`. **No handle escapes `execute()`**: the moment the child's
  `run` resolves, the child is dropped. There is no `wait`, `send`, `interrupt`,
  `resume`, `close`, or `list`.
- **Recursion is bounded, breadth is not.** Children get the `worker_tools` set
  (never including `delegate`); while `depth + 1 < max_depth` a *deeper* `delegate`
  (at `depth + 1`) is added to the child, so nesting is capped by
  `SubagentContext::max_depth`. But nothing caps how many children a single parent
  turn spawns, and a spawned child is not tracked anywhere after it returns.
- **`parallel_safe() == false`** (`subagent.rs`, `parallel_safe`) — "delegation
  serializes child loops; keep it off the parallel path." So even the fan-out case
  runs children one at a time; there is no concurrent handle set.
- **Feature- and config-gated, wired directly (not via the registry).** Gated by
  the `subagents` cargo feature (`crates/agent-runtime/Cargo.toml`) **and**
  `cfg.agent.subagents` (default `false`, `config/agent.toml`; bound
  `cfg.agent.subagent_max_depth = 2`). Wiring is inline in
  [`builder.rs`](../../crates/agent-runtime/src/builder.rs) (~line 926): it builds
  the `SubagentContext` and registers `DelegateTool::root(ctx)` — it does **not**
  go through `register_builtins` in
  [`registry.rs`](../../crates/agent-runtime/src/registry.rs), because it needs the
  fully-built worker toolset captured *before* `delegate` is added. A `Subagent`
  seam would formalise this into a config-selected factory.
- **Distinct from the multi-session machinery.** agent-seddon already has a
  `SessionManager` / `SessionRegistry`
  ([`crates/agent-runtime/src/agent/session_manager.rs`](../../crates/agent-runtime/src/agent/session_manager.rs),
  [`crates/agent-grpc/src/server/session_registry.rs`](../../crates/agent-grpc/src/server/session_registry.rs))
  for **multi-user session isolation** — many *independent top-level* conversations,
  keyed by identity, GC'd on idle (see `docs/design/multi-session/`). That is a
  *sibling* concern, not this one: a sub-agent graph is a **parent-owned tree of
  children within one session**, with parent→child edges and cascade semantics.
  The graph store should *reuse* the SessionManager's lifecycle/idle-GC discipline
  and its per-session metric labelling, but the topology (parent owns children,
  close cascades to the subtree) is new.
- **Reusable scaffolding already in the tree.** The metered-seam pattern
  ([`metered.rs`](../../crates/agent-runtime/src/metered.rs)), the gauges/counters
  in [`agent-metrics`](../../crates/agent-metrics/src/lib.rs), the per-op span
  pattern in [`agent-telemetry`](../../crates/agent-telemetry/), and the
  gRPC-seam + roundtrip pattern
  ([`crates/agent-grpc/tests/roundtrip.rs`](../../crates/agent-grpc/tests/roundtrip.rs))
  are all directly reusable.

Honest gap: the `Subagent` trait, the `AgentGraph` handle + registry, the
spawn-cap, the send/interrupt/resume/close/list operations, the background-child
notification path, the proto service + `--serve-subagents` wiring, and the graph
metrics/span **do not exist yet**. Today agent-seddon sits exactly where **pi**
does — a fire-and-wait boomerang — and this spec is the design of record for
leapfrogging to codex's graph while keeping the boomerang as the default backend.

## Peer implementations & their tests

| Peer | Impl path | Test path | Framework |
| --- | --- | --- | --- |
| codex | `codex-rs/core/src/tools/handlers/multi_agents_v2/{spawn,wait,send_message,interrupt_agent,list_agents,followup_task}.rs` + `multi_agents_spec.rs` (tool schemas: `spawn_agent`/`wait_agent`/`send_input`/`send_message`/`resume_agent`/`close_agent`/`interrupt_agent`/`list_agents`); `codex-rs/core/src/agent/control.rs` (`send_input`/`interrupt_agent`/`list_agents`, spawn/close/resume); `codex-rs/agent-graph-store/` (persisted parent/child edges); `codex-rs/agent-identity/` | `codex-rs/core/src/tools/handlers/multi_agents_tests.rs` (77 `#[tokio::test]`), `multi_agents_spec_tests.rs` (11) | cargo `#[tokio::test]` + insta |
| opencode | `packages/opencode/src/tool/task.ts` (`task` tool: child session, `subagent_type`, `background`, resume via `task_id`, depth via `cfg.subagent_depth`, abort→cancel cascade); `packages/opencode/src/agent/subagent-permissions.ts` | `packages/opencode/test/tool/task.test.ts` (`it.instance` cases) | bun:test + Effect |
| pi | `packages/coding-agent/examples/extensions/subagent/index.ts` — **example extension, not core**: spawns a separate `pi` process per subagent (single / parallel / chain modes, `MAX_PARALLEL_TASKS = 8`, `MAX_CONCURRENCY = 4`); fire-and-wait like agent-seddon | — (no test ships with the example extension) | vitest |
| hermes | `tools/delegate_tool.py` (`delegate_task`: child `AIAgent`, isolated context, restricted toolset, single + batch/parallel modes, `DELEGATE_BLOCKED_TOOLS` incl. no recursive delegation, depth limit, interrupt) | `tests/tools/test_delegate.py` (~40 cases) + `tests/cli/test_cli_interrupt_subagent.py`, `tests/run_agent/test_real_interrupt_subagent.py` | pytest |

**codex** is the deep anchor — the only peer with a genuine **orchestration
graph** and the full verb set this spec ports:

- **Full tool surface** (`multi_agents_spec.rs`): `spawn_agent` (with `task_name`,
  `agent_type`/role, model/effort overrides, `fork_turns` = `none`/`all`/N to fork
  parent history), `wait_agent` (clamped timeout, returns final status or times out),
  `send_input` (steer a running child; interrupts before prompting if needed),
  `send_message` (sibling mailbox — the deferred layer), `interrupt_agent`,
  `resume_agent` (restore a *closed* child and accept further `send_input`),
  `close_agent` (submit shutdown, return previous status), and `list_agents`
  (path-prefixed tree with a status schema that includes `interrupted`).
- **A persisted graph store** (`agent-graph-store/`): a storage-neutral
  `AgentGraphStore` trait over directional parent/child **thread-spawn edges**
  (`upsert_thread_spawn_edge` / `set_thread_spawn_edge_status` /
  `list_thread_spawn_children` / `list_thread_spawn_descendants`), with a
  two-state `ThreadSpawnEdgeStatus::{Open, Closed}` and **BFS descendant traversal**
  that filters *every traversed edge* (a closed edge hides its whole subtree).
  This is the topology model agent-seddon's in-memory `AgentGraph` mirrors.
- **The tests pin exactly the behaviours we must cover** (`multi_agents_tests.rs`):
  depth bounds — `spawn_agent_rejects_when_depth_limit_exceeded`,
  `spawn_agent_allows_depth_up_to_configured_max_depth`,
  `resume_agent_rejects_when_depth_limit_exceeded`; steering —
  `send_input_reports_missing_agent`, `send_input_interrupts_before_prompt`,
  `send_input_accepts_structured_items`; resume — `resume_agent_reports_missing_agent`,
  `resume_agent_noops_for_active_agent`,
  `resume_agent_restores_closed_agent_and_accepts_send_input`; wait —
  `wait_agent_times_out_when_status_is_not_final`,
  `wait_agent_clamps_short_timeouts_to_minimum`,
  `wait_agent_returns_final_status_without_timeout`,
  `wait_agent_returns_not_found_for_missing_agents`; close/cascade —
  `close_agent_submits_shutdown_and_returns_previous_status`,
  `tool_handlers_cascade_close_and_resume_and_keep_explicitly_closed_subtrees_closed`;
  and `spawn_agent_errors_when_manager_dropped`.

**opencode** ships a first-class-but-lighter `task` tool: a child *session*
(`sessions.create({ parentID })`), a `subagent_type` selecting a named agent, a
**depth limit** (`depth >= cfg.subagent_depth` ⇒ typed error —
`"allows nested subagents up to the configured depth"`,
`"prevents subagents from launching subagents by default"`), **resume** by passing
a prior `task_id` (`"execute resumes an existing task session from task_id"`,
`"execute creates a child when task_id does not exist"`), an experimental
**background** mode with a completion notification injected back into the parent
(`"rejects background execution when the experiment is disabled"`,
`"promotes a running foreground task without restarting it"`), and — the
cascade behaviour agent-seddon's `close` must match — **abort/cancel that recurses
into descendants** (`"execute cancels child session when abort signal fires"`,
`"cancelling a parent run recursively cancels descendant background tasks"`).
Child permissions are shaped by `deriveSubagentSessionPermission`
(`subagent-permissions.ts`).

**hermes** has a real `delegate_task` tool (`tools/delegate_tool.py`): a child
`AIAgent` with a **fresh conversation**, its **own** `task_id`/terminal session, a
**restricted toolset** (`DELEGATE_BLOCKED_TOOLS` strips `delegate_task` itself —
no recursion — plus `memory`/`send_message`/`execute_code`/`cronjob`), and single
*or* **batch/parallel** modes where the parent blocks until all children complete.
Its ~40-case `test_delegate.py` pins the pieces this spec cares about: depth
(`test_depth_limit`, `test_depth_increments`), the spawn/breadth cap
(`test_batch_capped_at_3`, `test_batch_mode`), toolset restriction
(`test_removes_blocked_toolsets`, `test_strips_cronjob_toolset`,
`test_no_recursive`-style blocklist via `test_strip_set_derived_from_blocklist`),
live-child tracking (`test_active_children_tracking`), partial failure
(`test_failed_child_included_in_results`,
`test_global_tool_names_restored_after_child_failure`), and observability
(`test_observability_fields_present`). **Interrupt** is covered separately —
`tests/cli/test_cli_interrupt_subagent.py::test_full_delegate_interrupt_flow` and
`tests/run_agent/test_real_interrupt_subagent.py::test_interrupt_child_during_api_call`.
hermes is fire-and-wait-with-interrupt: parallel + interruptible, but no
persisted graph, no `send_input` steering, no `resume`.

**pi** has only an **example extension** (`examples/extensions/subagent/index.ts`,
not core, no test) that spawns a separate `pi` process per subagent in
single/parallel/chain modes and captures structured JSON output — a fire-and-wait
boomerang, the same shape as agent-seddon's `delegate` today. It is the datapoint
that shows the *baseline* everyone starts from; codex is the datapoint that shows
the graph agent-seddon should reach.

## Completeness gaps

Behaviour agent-seddon must add to be the most complete (spec only — do **not**
implement here). Each maps to a test case below.

- **`Subagent` seam + `AgentGraph` handle store** (spec only — do **not**
  implement here). New async trait in `agent-core`:
  `spawn(spec) -> ChildId`, `wait(id, timeout) -> ChildStatus`,
  `send(id, input) -> ()`, `interrupt(id) -> ChildStatus`, `resume(id) -> ()`,
  `close(id) -> ChildStatus`, `list() -> Vec<ChildNode>`. Backed by an
  in-memory `AgentGraph` (`Map<ChildId, Node>` with a parent edge + child set,
  mirroring codex's `ThreadSpawnEdgeStatus::{Open, Closed}` two-state edge). Impl
  in `agent-subagents` behind the `subagents` feature; config key
  `agent.subagent_backend` (`"boomerang"` default | `"graph"`); the `delegate`
  boomerang is retained as the default backend. (Port codex `agent-graph-store`
  topology + control.rs verbs.)
- **Handle-returning spawn + concurrent fan-out** (spec only — do **not**
  implement here). `spawn` returns a `ChildId` instead of blocking to completion,
  so a parent can hold N handles and `wait` on them as they finish. The child runs
  on its own task; the parent turn is not pinned to one `await`. (Port codex
  `spawn_agent`, hermes batch mode, opencode background.)
- **Depth bound *and* breadth (spawn) cap** (spec only — do **not** implement
  here). Keep today's `max_depth` recursion bound, and **add** a per-parent live-
  child cap (`agent.subagent_max_children`): `spawn` past the cap is a typed
  `SpawnCapExceeded`, not an unbounded fork. (Port codex depth cases + hermes
  `test_batch_capped_at_3`.)
- **`wait` with a clamped timeout** (spec only — do **not** implement here).
  `wait(id, timeout)` returns the child's final status if it finished, else a
  typed `Timeout` once the (min-clamped, max-capped) deadline elapses — never
  wall-clock `sleep`; deterministic via an injected clock. (Port codex
  `wait_agent_times_out_when_status_is_not_final`,
  `wait_agent_clamps_short_timeouts_to_minimum`.)
- **`send` to steer a running child** (spec only — do **not** implement here).
  `send(id, input)` appends input to a *running* child's queue; to a missing child
  it is a typed `NotFound`; to an *exited* child it is a typed error, not a panic.
  (Port codex `send_input_reports_missing_agent`, `send_input_interrupts_before_prompt`.)
- **`interrupt` / `resume` / `close` + subtree cascade** (spec only — do **not**
  implement here). `interrupt` stops a running child (status → `Interrupted`);
  `resume` restores a *closed/interrupted* child and re-accepts `send`; `close`
  tears the child **and its whole subtree** down (edges → `Closed`, matching
  codex's descendant-hiding traversal), returning the previous status; a
  `close`/`resume` on an unknown id is a typed no-op/error, not a panic. (Port
  codex `resume_agent_restores_closed_agent_and_accepts_send_input`,
  `tool_handlers_cascade_close_and_resume_and_keep_explicitly_closed_subtrees_closed`;
  opencode `"cancelling a parent run recursively cancels descendant background tasks"`.)
- **`list` the live graph** (spec only — do **not** implement here). `list()`
  returns the parent-scoped tree in **stable order** (like codex's graph store),
  each node carrying `{ id, parent, status, goal }` — the introspection surface
  behind `list_agents` and, over gRPC, reflection. (Port codex
  `list_agents_tool_includes_path_prefix_and_agent_fields`,
  `list_agents_tool_status_schema_includes_interrupted`.)
- **Child-toolset restriction** (spec only — do **not** implement here). A child's
  tools are the requested subset of `worker_tools` (already true), and a
  deeper `delegate`/`spawn` is added **only while depth remains** — a leaf child
  can never re-spawn. Formalise codex/hermes's blocklist so a graph backend can't
  be tricked into unbounded recursion by a hostile `tools` arg. (Port hermes
  `test_strip_set_derived_from_blocklist`, `DELEGATE_BLOCKED_TOOLS`.)
- **Metered graph + per-spawn span (differentiator)** (spec only — do **not**
  implement here). `subagent_active_children` gauge (inc on `spawn`, dec on
  close/exit), `subagent_spawns_total{outcome=completed|interrupted|closed|reaped}`
  counter, `subagent_graph_depth` gauge, and a per-child OTel span
  (`subagent.child`, attrs `child_id`, `parent_id`, `depth`, `goal`, `status`,
  `duration_ms`, `iterations`) reusing
  [`agent-metrics`](../../crates/agent-metrics/src/lib.rs) +
  [`agent-telemetry`](../../crates/agent-telemetry/). (New — no peer emits a metered,
  span-traced graph.)
- **gRPC service** (spec only — do **not** implement here). `subagent.proto` with
  `Spawn`/`Wait`/`Send`/`Interrupt`/`Resume`/`Close`/`List` unary RPCs, reflection,
  `--serve-subagents`; a remote graph is dialable like any other seam. (New — no
  peer distributes its graph.)

## Table-driven test plan

New `#[rstest]` tables in `agent-subagents` (graph store + orchestration), driving
the child agent with a **`ScriptedProvider`** double from
[`agent-testkit`](../../crates/agent-testkit/src/lib.rs) so every child loop is
deterministic and hermetic (the exact pattern the current
[`subagent.rs`](../../crates/agent-runtime/src/subagent.rs) test
`delegate_runs_a_child_loop_and_returns_to_parent` already uses — one shared
`ScriptedProvider` scripts both parent and child turns). Doubles: `RecordingMemory`,
`StaticContext`, `AutoApprove`/`DenyPolicy`, `tempdir()` for child cwd, and a
**new `TestClock`** (injected `now()`) so the `wait`-timeout and idle-reap cases
advance the clock by hand — never a wall-clock `sleep`. Prefixes: `positive_`
succeeds, `negative_` rejects, `corner_` odd-but-valid, `boundary_` at a limit.
`(port: <peer>)` marks a case mined from a peer test; `(new: agent-seddon)` are ours.

```rust
// ---- spawn returns a handle; wait resolves to the child's final status -------
#[rstest]
#[tokio::test]
async fn positive_spawn_returns_handle_then_wait_resolves() {                 // (port: codex spawn_agent + wait_agent_returns_final_status_without_timeout)
    // graph.spawn({goal:"sub"}) -> ChildId (does NOT block to completion).
    // list() contains the node status=Running; wait(id, big_timeout) -> Completed
    // with the scripted child summary. assert subagent_active_children == 1 during,
    // -> 0 after, subagent_spawns_total{outcome="completed"} += 1.
}

// ---- concurrent fan-out: N handles, wait on each ----------------------------
#[rstest]
#[tokio::test]
async fn positive_fanout_three_children_all_waited() {                        // (port: hermes test_batch_mode / opencode background fan-out)
    // spawn three children, hold all three ChildIds, wait each. All three summaries
    // return; list() shows three distinct nodes under the same parent edge.
}

// ---- wait times out while the child is not final ----------------------------
#[rstest]
#[case::positive_wait_final_before_deadline(/*child_finishes=*/ true,  Ok("Completed"))]  // (port: codex wait_agent_returns_final_status_without_timeout)
#[case::boundary_wait_times_out_not_final(/*child_finishes=*/ false, Err("timeout"))]      // (port: codex wait_agent_times_out_when_status_is_not_final)
#[tokio::test]
async fn wait_timeout_cases(#[case] child_finishes: bool, #[case] expect: Result<&str, &str>) {
    // spawn a child; if !child_finishes it stays Running. wait(id, ttl); advance the
    // injected TestClock past ttl for the timeout case (deterministic, no sleep).
}

// ---- wait timeout is clamped to the configured minimum ----------------------
#[rstest]
#[tokio::test]
async fn boundary_wait_timeout_clamped_to_minimum() {                         // (port: codex wait_agent_clamps_short_timeouts_to_minimum)
    // wait(id, 0) does not busy-return: the effective deadline is clamped up to
    // min_wait_timeout_ms before the (advanced-clock) timeout fires.
}

// ---- send steers a running child; typed errors on missing/exited ------------
#[rstest]
#[case::positive_send_to_running(Target::Running,  Ok(()))]                    // (port: codex send_input_accepts_structured_items)
#[case::negative_send_missing(Target::Missing,     Err("not found"))]          // (port: codex send_input_reports_missing_agent)
#[case::negative_send_after_exit(Target::Exited,   Err("not running"))]        // (port: codex; cf. send_input_interrupts_before_prompt)
#[tokio::test]
async fn send_cases(#[case] target: Target, #[case] expect: Result<(), &str>) {
    // send(id, "also check tests"): Running enqueues; Missing/Exited are typed
    // errors (never a panic), gauge unchanged.
}

// ---- recursion is bounded by max_depth --------------------------------------
#[rstest]
#[case::boundary_spawn_at_max_depth_ok(/*depth=*/ 1, Ok("child"))]            // (port: codex spawn_agent_allows_depth_up_to_configured_max_depth)
#[case::negative_spawn_beyond_max_depth(/*depth=*/ 2, Err("depth limit"))]    // (port: codex spawn_agent_rejects_when_depth_limit_exceeded)
#[tokio::test]
async fn depth_bound_cases(#[case] depth: usize, #[case] expect: Result<&str, &str>) {
    // with max_depth=2: a leaf child (depth==max_depth-1) has NO deeper spawn tool,
    // so a spawn attempt at max depth is rejected before any child is built.
}

// ---- breadth (spawn) cap: too many live children is rejected ----------------
#[rstest]
#[tokio::test]
async fn negative_spawn_beyond_children_cap() {                               // (port: hermes test_batch_capped_at_3)
    // with subagent_max_children=3: spawn 3 (ok), the 4th -> typed SpawnCapExceeded,
    // no 4th child built, subagent_active_children stays == 3.
}

// ---- close tears down the whole subtree; edges go Closed ---------------------
#[rstest]
#[tokio::test]
async fn positive_close_cascades_to_subtree() {                               // (port: codex tool_handlers_cascade_close_and_resume…; opencode "recursively cancels descendant background tasks")
    // parent -> child A -> grandchild B. close(A) reaps A AND B (edges Closed),
    // list() no longer walks into the closed subtree, gauge drops by 2,
    // subagent_spawns_total{outcome="closed"} += 2. close(A) again -> typed no-op.
}

// ---- interrupt then resume restores a closed child --------------------------
#[rstest]
#[tokio::test]
async fn corner_interrupt_then_resume_accepts_send() {                        // (port: codex resume_agent_restores_closed_agent_and_accepts_send_input)
    // spawn -> interrupt(id) (status Interrupted) -> resume(id) (status Running) ->
    // send(id, "continue") is accepted. resume on an ACTIVE child is a no-op
    // (codex resume_agent_noops_for_active_agent).
}

// ---- corner: send after the child already exited ----------------------------
#[rstest]
#[tokio::test]
async fn corner_send_after_child_exit_is_typed_error() {                      // (new: agent-seddon; cf. codex send_input on final agent)
    // spawn a child that runs to Completed, wait() it, THEN send(id,..) and
    // interrupt(id): both return typed "not running"/"already final", never panic;
    // the exited node is still list()-able (retained) until reaped.
}

// ---- unknown-id operations are typed, not panics ----------------------------
#[rstest]
#[case::negative_wait_missing(Op::Wait,      Err("not found"))]              // (port: codex wait_agent_returns_not_found_for_missing_agents)
#[case::negative_interrupt_missing(Op::Interrupt, Err("not found"))]         // (port: codex)
#[case::negative_resume_missing(Op::Resume,  Err("not found"))]             // (port: codex resume_agent_reports_missing_agent)
#[case::corner_close_missing_is_noop(Op::Close, Ok("noop"))]               // (new: agent-seddon) idempotent close
fn unknown_id_cases(#[case] op: Op, #[case] expect: Result<&str, &str>) {
    // drive each op with a never-issued ChildId; NotFound is typed, close is Ok(noop).
}

// ---- adversarial: hostile `tools`/`goal` args (model is untrusted) ----------
#[rstest]
#[case::adversarial_tools_cannot_readd_delegate(json!({"goal":"x","tools":["delegate","bash"]}))]  // (new: agent-seddon)
#[case::adversarial_goal_is_huge(json!({"goal": "A".repeat(1_000_000)}))]                          // (new: agent-seddon)
#[case::adversarial_tools_unknown_names_ignored(json!({"goal":"x","tools":["../../etc"]}))]        // (new: agent-seddon)
#[tokio::test]
async fn adversarial_spawn_arg_cases(#[case] args: serde_json::Value) {
    // a hostile `tools` list can NEVER re-add `delegate`/`spawn` past max_depth
    // (child stays leaf), unknown tool names are dropped not errored, and an
    // oversized goal is capped before it reaches the child — no unbounded spawn.
}

// ---- policy gates spawn (a child is a fresh capability surface) --------------
#[rstest]
#[case::positive_policy_allows_spawn(/*policy=*/ Allow, Ok("child"))]        // (new: agent-seddon)
#[case::negative_policy_denies_spawn(/*policy=*/ Deny,  Err("denied"))]      // (new: agent-seddon; cf. spec 08)
#[tokio::test]
async fn policy_gate_cases(#[case] policy: PolicyKind, #[case] expect: Result<&str, &str>) {
    // spawn() runs through the Policy seam; Deny rejects before any child agent is
    // built (no leaked child, gauge stays 0).
}
```

gRPC roundtrip (extend
[`crates/agent-grpc/tests/roundtrip.rs`](../../crates/agent-grpc/tests/roundtrip.rs)):
`Spawn` a scripted child over the wire (TCP + UDS), `List` and assert the node is
present with `Running`, `Send` it input, `Wait` and assert the final status frame
arrives, then `Close` — asserting the seam is identical in-process vs. served, the
pattern every other seam's roundtrip test uses.

Prefix legend (repo convention): `positive_` expected success, `negative_` expected
error, `corner_` odd-but-valid, `boundary_` at a limit, `adversarial_` mandatory
for the untrusted `spawn` args. `(port: <peer>)` names the peer a case was mined
from; `(new: agent-seddon)` marks the idempotent-close, send-after-exit,
adversarial-args, policy-gate, and metered-graph assertions with no peer analogue.

## Harness obligations

The implementing PR must satisfy all of the following (follows the #21–45 pattern).

- **Seam + registry:** `Subagent` trait in `agent-core` (+ an `AgentGraph`
  handle/registry type); impl in a new sibling crate `agent-subagents` behind the
  existing `subagents` cargo feature; a config-selected factory line so
  `agent.subagent_backend` picks `"boomerang"` (today's `DelegateTool`, default) or
  `"graph"` — wired where `builder.rs` (~line 926) registers the sub-agent tool
  today, and where practical folded into
  [`register_builtins`](../../crates/agent-runtime/src/registry.rs); a
  `MeteredSubagent` in [`metered.rs`](../../crates/agent-runtime/src/metered.rs);
  doc in `docs/components/subagents.md`.
- **Proto + gRPC:** `crates/agent-proto/proto/agent/v1/subagent.proto`
  (`Spawn`/`Wait`/`Send`/`Interrupt`/`Resume`/`Close`/`List` unary RPCs) + `build.rs`
  entry + server/client in `agent-grpc` (mirroring the existing seam servers) +
  `--serve-subagents` + reflection; commit the `buf.image.binpb` bump
  (`nix run .#buf-image`); add the endpoint to `nix/constants.nix` →
  `nix run .#gen-constants`.
- **Metrics + OTel:** `subagent_active_children` gauge, `subagent_graph_depth`
  gauge, `subagent_spawns_total{outcome}` counter in
  [`agent-metrics`](../../crates/agent-metrics/src/lib.rs); a per-child
  `subagent.child` span (attrs `child_id`, `parent_id`, `depth`, `goal`, `status`,
  `duration_ms`, `iterations`) reusing
  [`agent-telemetry`](../../crates/agent-telemetry/) — the metered-graph
  differentiator. Where a graph runs under a session, reuse the multi-session
  `session`/`user` label convention (`docs/design/multi-session/06-observability.md`).
- **Bench:** the **graph store** is the deterministic CPU hot path (edge
  upsert/status-flip and the **BFS descendant walk** codex pins in
  `list_thread_spawn_descendants`) — add an iai-callgrind bench over a fixed
  N-node graph (spawn N edges, then `list`/descendant-walk) with an absolute Ir
  ceiling in `nix/checks/bench.nix`. The *child agent run itself* is provider-bound
  (network/model latency), so like `bash`/`pty` it is bench-**SKIP**; document the
  split (only the pure graph-topology helpers are benched).
- **Leak:** a dhat `tests/leak.rs` (iteration-based, `dhat-heap` feature) over the
  **spawn → wait → close** path, asserting that closing a child (and cascading to
  its subtree) frees the node, its child `Agent`, its retained buffer/queue, and its
  edges — no leaked children across iterations, and the graph map returns to empty.
  Retained/exited nodes must be bounded (a ring + idle-TTL reap, reusing the
  SessionManager discipline) so a long fan-out session can't grow the graph
  unbounded — the leak-critical path of this seam.

## References

- **agent-seddon:**
  [`crates/agent-runtime/src/subagent.rs`](../../crates/agent-runtime/src/subagent.rs) (`DelegateTool` / `SubagentContext` — the fire-and-wait boomerang this seam extends; `max_depth`, `parallel_safe`, the `ScriptedProvider` parent+child test),
  [`crates/agent-runtime/src/builder.rs`](../../crates/agent-runtime/src/builder.rs) (~line 926 — where `DelegateTool::root` is wired under the `subagents` feature + `cfg.agent.subagents`),
  [`crates/agent-runtime/src/registry.rs`](../../crates/agent-runtime/src/registry.rs) (`register_builtins` — the factory pattern to fold the backend into),
  [`crates/agent-runtime/src/agent/session_manager.rs`](../../crates/agent-runtime/src/agent/session_manager.rs) + [`crates/agent-grpc/src/server/session_registry.rs`](../../crates/agent-grpc/src/server/session_registry.rs) (the *sibling* multi-session lifecycle/GC discipline to reuse — distinct from a parent-owned child graph),
  [`crates/agent-runtime/src/metered.rs`](../../crates/agent-runtime/src/metered.rs) (metered-seam pattern),
  [`crates/agent-metrics/src/lib.rs`](../../crates/agent-metrics/src/lib.rs) (gauges/counters to extend),
  [`crates/agent-telemetry/`](../../crates/agent-telemetry/) (per-child span),
  [`crates/agent-grpc/tests/roundtrip.rs`](../../crates/agent-grpc/tests/roundtrip.rs) (roundtrip pattern),
  [`crates/agent-testkit/src/lib.rs`](../../crates/agent-testkit/src/lib.rs) (`ScriptedProvider`, `RecordingMemory`, `StaticContext`, `tempdir`, doubles),
  dependencies: [`08-permissions-policy.md`](08-permissions-policy.md) (spawn policy gate), [`14-sandbox.md`](14-sandbox.md) (deferred: run a child inside isolation), `docs/design/multi-session/` (the session machinery this graph is distinct from but reuses).
- **codex (anchor):** `codex-rs/core/src/tools/handlers/multi_agents_v2/{spawn.rs,wait.rs,send_message.rs,interrupt_agent.rs,list_agents.rs,followup_task.rs}`,
  `codex-rs/core/src/tools/handlers/multi_agents_spec.rs` (tool schemas: `spawn_agent`/`wait_agent`/`send_input`/`send_message`/`resume_agent`/`close_agent`/`interrupt_agent`/`list_agents`),
  `codex-rs/core/src/agent/control.rs` (`send_input`, `interrupt_agent`, `list_agents`; spawn/close/resume),
  `codex-rs/agent-graph-store/src/{store.rs,types.rs,local.rs}` (`AgentGraphStore`, `ThreadSpawnEdgeStatus::{Open,Closed}`, BFS `list_thread_spawn_descendants`),
  `codex-rs/agent-identity/`;
  tests: `codex-rs/core/src/tools/handlers/multi_agents_tests.rs` (`spawn_agent_rejects_when_depth_limit_exceeded`, `spawn_agent_allows_depth_up_to_configured_max_depth`, `send_input_reports_missing_agent`, `send_input_interrupts_before_prompt`, `resume_agent_restores_closed_agent_and_accepts_send_input`, `resume_agent_noops_for_active_agent`, `wait_agent_times_out_when_status_is_not_final`, `wait_agent_clamps_short_timeouts_to_minimum`, `close_agent_submits_shutdown_and_returns_previous_status`, `tool_handlers_cascade_close_and_resume_and_keep_explicitly_closed_subtrees_closed`),
  `multi_agents_spec_tests.rs` (`spawn_agent_tool_v2_requires_task_name_and_lists_visible_models`, `list_agents_tool_status_schema_includes_interrupted`).
- **opencode:** `packages/opencode/src/tool/task.ts` (`task` tool: child session, `subagent_type`, `background`, `task_id` resume, `cfg.subagent_depth`, abort→cancel cascade), `packages/opencode/src/agent/subagent-permissions.ts` (`deriveSubagentSessionPermission`);
  tests: `packages/opencode/test/tool/task.test.ts` (`allows nested subagents up to the configured depth`, `prevents subagents from launching subagents by default`, `execute resumes an existing task session from task_id`, `execute creates a child when task_id does not exist`, `execute cancels child session when abort signal fires`, `cancelling a parent run recursively cancels descendant background tasks`, `rejects background execution when the experiment is disabled`, `promotes a running foreground task without restarting it`).
- **hermes:** `tools/delegate_tool.py` (`delegate_task`: child `AIAgent`, isolated context, restricted toolset, single + batch/parallel modes, `DELEGATE_BLOCKED_TOOLS` — no recursive delegation, depth limit, interrupt);
  tests: `tests/tools/test_delegate.py` (`test_depth_limit`, `test_depth_increments`, `test_batch_capped_at_3`, `test_batch_mode`, `test_removes_blocked_toolsets`, `test_strip_set_derived_from_blocklist`, `test_active_children_tracking`, `test_failed_child_included_in_results`, `test_global_tool_names_restored_after_child_failure`, `test_observability_fields_present`), `tests/cli/test_cli_interrupt_subagent.py` (`test_full_delegate_interrupt_flow`), `tests/run_agent/test_real_interrupt_subagent.py` (`test_interrupt_child_during_api_call`).
- **pi:** `packages/coding-agent/examples/extensions/subagent/index.ts` — **example extension only, not core** (spawns a separate `pi` process per subagent; single/parallel/chain modes; `MAX_PARALLEL_TASKS = 8`, `MAX_CONCURRENCY = 4`; fire-and-wait); no test ships with the example.
