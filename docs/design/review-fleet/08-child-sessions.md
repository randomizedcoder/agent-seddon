# Increment 8 — child sessions + workspace inheritance (DESIGNED, build deferred)

Components: **C20** (child spawn) · **C21** (lineage + workspace inheritance) · **C22**
(identity + cancellation propagation). Status: **fully designed here, build deferred** — the
fleet ships on in-process parallelism (review fan-out) and does **not** block on this. Build
it when a sub-task genuinely needs its own context window (a deep-dive that would blow the
parent's window, or an isolated "apply-a-fix-and-test" run).

## Why this is net-new (not a tweak)

Anchor verification confirmed **none** of this exists today:

- No spawn/subagent/delegate tool; `Agent` has no child-run method (`agent-tasks` is just a
  to-do list). All "parallel" work is in-process, sharing one context: review collectors
  (`agent-review/src/orchestrator.rs:305`), per-tool `tokio::spawn`
  (`agent-runtime/src/agent.rs:1790`), provider fork (`agent-providers/src/branching.rs:472`,
  which never even reaches `ToolContext`).
- `SessionKey` is flat (`agent-core/src/identity.rs:134`) — no parent/child, no lineage.
- `AGENT_IDENTITY` is a `task_local` scoped **once per turn** (`session.rs:98`) and, by its
  own doc comment (`identity.rs:196`), **not inherited across `tokio::spawn`**. Nothing
  re-scopes it around spawned work.

So "a session triggers a sub-agent that inherits the workspace" is a subsystem, not a flag.

## The invariant this preserves

```
child workspace  ⊆  parent (repo) workspace  ⊆  fleet_root
```

A child can do work *for the parent's repo* but can never reach outside the parent's
confined root. The workspace is a resource of the **repo (the fleet session)**; a child
attaches to it — it is never derived from the child's own identity (that was the bug: a
child deriving cwd from `path_under(child_key)` lands in an empty `root/<user>/<child-id>`).

## C20 — child-session spawn

- **API:** `Agent::spawn_child(parent: &SessionKey, goal: String, opts: ChildOpts) ->
  Result<ChildHandle>`. Mints a child `SessionKey`, establishes lineage (C21), re-scopes
  identity (C22), drives a nested agent run on the same backend `Agent`, and returns the
  child's final answer / structured output to the parent (the parent aggregates, exactly as
  the review loop aggregates its narrative).
- **Tool surface:** a `spawn` (a.k.a. `subagent`) tool, **Policy-gated**, so the model can
  delegate. Args: `goal`, `mode`/`skill`, `writable: bool`. Fail-closed defaults.
- **`ChildOpts`:** `{ writable, skill, inherit: WorkspaceInherit }` where `WorkspaceInherit`
  ∈ `{ SharedReadOnly, OwnWorktree, ScratchSubdir }` (see C21).
- **Caps (load-bearing at fleet scale):** max spawn **depth** and max **breadth** per
  parent, enforced fail-closed; children count against `SessionManager::with_limits`
  accounting so a fleet of 100 sessions each spawning N children cannot exhaust the host.
  Exceeding a cap is a clean error to the parent, never a silent drop.

## C21 — session lineage + workspace inheritance

**Lineage without a wire change.** Keep `SessionKey` flat (no proto churn). `SessionManager`
gains a `parent: Option<SessionKey>` per entry (a lineage map alongside the existing flat
`HashMap<SessionKey, Entry>` at `session_manager.rs:126`). The manager knows the tree; the
key stays simple. Child session id = a fresh UUID (as `SessionRegistry::open` already mints,
`:370`); the map records who spawned it.

**cwd inheritance — the seam designed in increment 1.** Increment 1's C4 defines cwd
resolution as one function (not a hardcoded `path_under`):

```
resolve_cwd(key, opts):
    if opts.working_dir is Some:      confine(parent_workspace / working_dir)   // explicit
    else if opts.inherited_workspace: opts.inherited_workspace                   // child
    else:                             key.path_under(fleet_root)                 // top-level
```

Increment 1 wires the first and third branches (fleet sessions); **this increment supplies
the middle branch**. Because the abstraction already exists, building children never reopens
increment 1.

**Three inheritance modes** (`WorkspaceInherit`):
- `SharedReadOnly` (default, most analysis children) — cwd = the parent's **read-only** PR
  worktree. Concurrent reads are safe; N children analyze the same checkout at once.
- `OwnWorktree` (a child that must build/test/patch) — `worktree_add` off the parent's
  **shared bare mirror** into `<workspace>/.children/<child-id>/wt` (confined, writable).
  Git-native isolation *inside one repo*: children don't stomp each other, and the mirror is
  shared (git handles worktree locking).
- `ScratchSubdir` — a confined writable `<workspace>/.children/<child-id>/` for ephemeral
  output, with the read-only worktree still visible via the parent root.

Every child path goes through `confine()` under the parent workspace — the invariant above
is enforced, not assumed. Child dirs are GC'd when the child (or parent) ends.

## C22 — identity + cancellation + obs propagation

- **Identity (mandatory).** The spawn helper wraps the child run in
  `agent_core::scope(child_key, child_future)` (`identity.rs:198`). Without it,
  `current_identity()` inside the child is empty and tenancy/memory/metrics misattribute.
  This is the single easiest thing to get wrong (task-locals aren't inherited) and the most
  important to test.
- **Tenancy.** Child inherits the parent's **`user`** (a sub-agent is the same principal), so
  per-user memory (`agent-memory/src/tenant.rs`) isn't fragmented; only the `session` differs
  (for obs + isolation). Child inherits the parent's **per-session token** (C5) — a sub-agent
  acts as the same principal on the same repo; creds are **never** broadened on spawn.
- **Cancellation cascade.** The child holds a `CancellationToken` derived from the parent's;
  the `SessionManager` lineage map drives teardown so cancelling/removing a parent (or the
  `RunHandle` drop that already cancels a run, `agent_session.rs:162`) cancels its whole
  subtree. No orphaned child runs.
- **Metrics/tracing.** Child `SessionMetrics` labeled with the child `(session, user)`,
  **retired on child end** (multi-session 06 discipline, `agent-metrics/src/lib.rs:2409`); the
  child OTEL span is a *child span* of the parent's, so the trace shows the delegation tree.

## Interaction with the fleet

Once built, the fleet gains genuine parallel deep-dives: e.g. the code-review skill (C11) can
delegate "audit the security of file X" or "apply the suggested fix and run `-race`" to a
child with its own context window and (for the fix-and-test case) its own writable worktree,
then fold the result back into the draft (C13). Until built, those run as in-process
collectors (C12) sharing the one session — which is why the fleet doesn't block on this.

## Test matrix (when built)

- Workspace inheritance: `positive_shared_readonly_child_sees_parent_checkout`;
  `positive_own_worktree_child_gets_isolated_writable_dir`;
  `positive_two_writable_children_do_not_collide`;
  `adversarial_child_working_dir_escaping_parent_root_rejected` (confine);
  `adversarial_child_cannot_reach_sibling_repo_workspace`.
- Lineage/caps: `positive_lineage_map_records_parent`;
  `boundary_spawn_depth_cap_enforced`; `boundary_spawn_breadth_cap_enforced`;
  `boundary_children_count_against_session_limits`.
- Identity: `positive_child_identity_is_scoped_in_spawned_task` (the task-local trap);
  `positive_child_inherits_parent_user_tenancy`;
  `adversarial_child_does_not_get_broader_creds_than_parent`.
- Cancellation: `positive_cancel_parent_cancels_children`;
  `corner_child_finishing_first_is_clean`.
- Obs: `boundary_child_metrics_retired_on_end`; `positive_child_span_is_child_of_parent`.

## Done when (deferred)

`nix flake check` green; a session can `spawn_child` with a goal; the child runs with its own
identity + context window, sees the parent's checkout (or gets its own writable worktree),
cannot escape the parent's confined root, is cancelled with its parent, and its metrics
retire on end. Fleet skills can delegate isolated sub-tasks and fold results back.
