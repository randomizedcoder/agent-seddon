# Campaigns: objectives → a hierarchical, ACID task tree → workers → PRs (design of record)

> **Status:** design / pre-implementation, opened 2026-09-26 from the
> [self-improvement gap analysis](../../gap-analysis/self-improvement.md) (SI-11). Nothing in
> this track is built yet; [`STATUS.md`](STATUS.md) is the tracker and
> [`05-increments.md`](05-increments.md) the build sequence. Every claim about existing code
> carries a `path:line` against `main` `5ddcda7`.

## Why this exists

The [repo-knowledge track](../repo-knowledge/README.md) gives the agent a deterministic code
graph, a cited inventory and a goal-shaped brief. The gap analyses list what is missing. What
does not exist is the **process** between the two: a persisted record of a major objective, its
decomposition into smaller and smaller pieces, and the farming-out of the pieces that are small
enough to headless workers that open PRs.

Today:

| Fact | Evidence |
|---|---|
| The only plan store is a flat `Todo { content, status, priority }` behind `TaskTracker`, in memory, one in-progress item at a time | `crates/agent-core/src/lib.rs:1055,1074`, `crates/agent-tasks/src/memory.rs` |
| No table, struct or migration in the workspace has a parent pointer, a materialized path, `ltree` or a closure table; the one parent-linked structure is the file-backed session checkpoint DAG | `crates/agent-session/src/file.rs:19`, `crates/agent-core/src/lib.rs:1557` |
| Scheduler jobs and sessions take a free-text goal only | `crates/agent-core/src/lib.rs:3832,3878` |
| Sub-agents exist but are serial, depth-capped and off by default | `crates/agent-runtime/src/subagent.rs:39`, `crates/agent-runtime/src/config.rs:2939-2952` |
| `TaskMode::Implement` exists; only `Review` has a flow | `crates/agent-core/src/lib.rs:5310` |

A **campaign** is one objective (a gap id, an issue, a feature request) owned by one
`(tenant, repo)`. A large model, given the repo brief, answers one question per node: *is this
small and specific enough to execute, or must it be split?* Splits become child rows; leaves are
claimed by a driver, run in a sandboxed worker session on a fresh worktree, and end as a PR. The
whole tree is one Postgres table; every transition is one transaction that also writes its audit
row.

## The shape in one picture

```
 gap / issue / feature ──► agent campaign add ──► tasks (root, kind = objective, policy)
                                                        │
                    ┌───────────────────────────────────┤ planner tick (bounded)
                    │  brief (RK-12) + ancestors + siblings + node → LLM (schema-validated)
                    │  execute | split | needs_info | reject
                    │      │        │         │           │
                    │   mark leaf  N children  await     blocked
                    │               ↻ (each child is asked next tick)
                    ▼
        leaves (ready, deps satisfied) ──► CampaignDriver claim (SKIP LOCKED, lease)
                                                        │
                                   worker subprocess: worktree → Implement session → commit
                                                        │
                                   push → Forge::create_pr (draft) → in_review
                                                        │
                                   PR poller: merged → done ──► rollup to ancestors → campaign done
 side rails: task_events (every transition) · task_attempts (every LLM / worker run)
```

## Decisions

**D1 — The tree is one table: adjacency plus a materialized path.** `tasks` carries
`parent_id`, `path`, `depth` and `ordinal`. The root segment of `path` is the campaign's own
`task_id` (`'1042'`, `'1042.1'`, `'1042.1.3'`), obtained inside the insert from
`nextval(pg_get_serial_sequence('tasks','task_id'))` in a CTE, so there is no counter table and
no placeholder write; `campaign_id = task_id` lands in the same statement. Display maps the root
to a letter (`A.1.3`) at render time only. Subtree reads are `path LIKE $p || '.%'` over a
`text_pattern_ops` index; ancestor reads are `$path LIKE path || '.%'`. Depth is capped at 6.
Rejected: `ltree` (needs `CREATE EXTENSION`; the deployed database provisions none,
`nix/nixos/agent-postgres.nix:79-81`), a closure table (a second table for the same fact),
triggers (none exist in the workspace; parent consistency is enforced app-side under the parent's
row lock and asserted by an invariant query in the test suite). DDL in
[`01-schema.md`](01-schema.md).

**D2 — Paths never change.** There is no move operation. Re-decomposition supersedes the live
children (`state = 'superseded'`, `superseded_by`) and the new children take the next ordinals.
At most 8 children per parent, live plus superseded, keeps `UNIQUE (tenant, path)` valid forever
and the path grammar closed.

**D3 — Concurrency is READ COMMITTED plus row locks plus a `version` compare-and-swap.**
`SELECT … FOR UPDATE` on the parent for decompose and replan; `FOR UPDATE SKIP LOCKED` for claims;
the ancestor chain is locked root → leaf (ascending `depth`) before a rollup, so two workers
completing siblings never deadlock. Every state write inserts a `task_events` row in the same
transaction; the store helper does both or neither. The config store's `apply` is the precedent
(`crates/agent-config-store/src/postgres.rs:287-302`). No SERIALIZABLE retry loops. Protocols in
[`02-transactions.md`](02-transactions.md).

**D4 — LLM-driven writes are idempotent.** Each planner call records a `task_attempts` row whose
`idem_key = sha256(tenant, task_id, version, prompt_hash)` is unique per tenant. A replayed
decomposition is `AlreadyApplied`, not a second set of children. The parent's `expected_version`,
read before the model call, must still match at write time or the write is `Conflict`.

**D5 — Policy lives on the root.** `policy JSONB`, root only, snapshotted at creation and editable
by humans only. Defaults: `approve_levels = [1]`, `require_pr_approval = true`, `draft_prs = true`,
`max_depth = 6`, `max_children = 8`, `max_nodes = 200`, `max_plan_attempts = 3`,
`max_plan_tokens = 400000`, `max_worker_tokens_per_leaf = 2000000`, `auto_replan = false`,
`lease_secs = 1800`. No policies table.

**D6 — The planner asks one question with one schema.** `{ decision: execute | split |
needs_info | reject, reason, confidence, question?, children[≤ 8] }` through the existing
schema-validated call with bounded repairs (as built: `ask_structured`,
`crates/agent-campaign/src/planner/ask.rs`, the same loop shape as `Agent::complete_structured`,
`crates/agent-runtime/src/agent.rs:1184`, kept separate for the dependency direction and the
token sum; the Draft-07 `OutputSchema` seam, `crates/agent-core/src/lib.rs:1135`).
Post-validation is fail-closed in Rust, and the counts are
re-checked **inside** the decompose transaction under the parent lock. Details in
[`03-decomposition.md`](03-decomposition.md).

**D7 — A leaf must be citeable.** `execute` is accepted only with at least one acceptance
criterion and at least one `touches` entry that resolves to a `node_key` in the repo-knowledge
store (RK-08 / RK-12) or to an existing path in the worktree that passes `safe_segment`
(`crates/agent-core/src/identity.rs:26`); `est_size ∈ { xs, s }`. Roots are never executed
directly.

**D8 — The executor has the scheduler driver's shape.** `CampaignDriver::tick` mirrors
`SchedulerDriver::tick_with_exec` (`crates/agent-runtime/src/scheduler_driver.rs:233-288`):
tenants in rotated order, round-robin interleave, a `JoinSet` under a per-tenant and then a global
`Semaphore`, and a sandbox subprocess per unit of work (`agent --run-task --tenant T --task <id>`
beside `--run-scheduled-job`, `crates/agent-cli/src/main.rs:761`). The worker adds a worktree at
the default branch, runs an Implement-mode session, commits, pushes and opens a draft PR. A poller
moves merged PRs to `done` and closed ones to `failed`. Details in [`04-executor.md`](04-executor.md).

**D9 — Humans act through the principal, never through arguments.** `approve`, `answer`,
`replan` and `cancel` record `actor = 'user:' || principal` from `current_tenant()` /
`SessionKey.user` (`crates/agent-core/src/identity.rs:298,205`); a caller-supplied actor is
ignored. `policy` is writable only when the principal is not `model:*`. There is no model-facing
campaign tool in v1.

**D10 — Multi-tenant and multi-repo like every other Postgres tier.** Every primary key leads
with `tenant` and references `tenants` (created idempotently by the config store,
`crates/agent-config-store/migrations/0001_config_store.sql:10`). `repo_id` references
`repos(tenant, repo_id)` from RK-02, added conditionally until that migration exists.
`PgCampaigns::with_tenant` (the scheduler's constructor shape,
`crates/agent-scheduler/src/store.rs:123`) binds the tenant on every statement and the runtime
wraps it in `PerTenant` (`crates/agent-runtime/src/tenant.rs:71`). Cross-tenant adversarial tests
ship with the store (CP-02).

## Recommendation summary

| Need | Decision | Doc | Increment |
|---|---|---|---|
| Hierarchy in one table | adjacency + materialized path, root = `task_id`, depth ≤ 6, 8 children | 01 | CP-01, CP-02 |
| ACID transitions | one transaction per transition, row locks, version CAS, events in the same transaction | 02 | CP-02 |
| Idempotent model writes | `task_attempts.idem_key`, `expected_version` | 02 | CP-02 |
| The "small enough?" loop | one schema, one question per node, fail-closed validation, caps | 03 | CP-03, CP-04 |
| Farming out leaves | driver tick, lease + heartbeat + reap, worker subprocess, PR poller | 04 | CP-05, CP-06 |
| Repo context in the question | RK-12 brief, `touches` validated against RK-08 | 03 | CP-07 |
| Tenancy, multi-repo | tenant-led keys, `repo_id` FK, `with_tenant`, `PerTenant` | 01 | CP-02 |
| Observability | metrics, ClickHouse events, component doc | 04 | CP-08 |
| Tests | table-driven matrices per component with all case classes | 06 | every CP |

## Threat model

The planner's input below the root is model-written. Worker sessions run model-chosen edits.
The store spans tenants.

| Threat | Mitigation |
|---|---|
| Model-written goals and titles become prompts for later calls | Screened with `scan_for_injection` (`crates/agent-core/src/security.rs:97`) before persist and again before use; a hit blocks the node with `detail.reason = 'injection'`; rendered inside a random-tag fence labelled untrusted; the system prompt is fixed Rust text |
| Runaway decomposition | `max_depth`, `max_children`, `max_nodes` re-checked inside the transaction under the parent lock; `max_plan_tokens` read from `SUM(task_attempts.tokens_*)` before every call; `max_plan_attempts` per node |
| Worker does damage | Same sandbox as scheduled jobs; a fresh worktree per leaf; draft PRs; forge writes are policy-gated by the caller exactly as today (`Forge::create_pr`, `crates/agent-core/src/lib.rs:4007`); branch names are built from digits and dashes only |
| Forged approvals | D9: actor from the principal; approval of a PR is an event on the leaf, and the poller requires it when the policy says so |
| Cross-tenant reads or writes | D10: tenant bound on every statement; a foreign `task_id` is `NotFound`; adversarial suite T14 |
| Lease theft | `claimed_by` is a random 128-bit owner token per driver process; every worker write carries `AND claimed_by = $owner AND tenant = $t` |
| Oversize fields | `title ≤ 120`, `goal ≤ 4000`, `acceptance ≤ 6 × 300`, `touches ≤ 12`, `question ≤ 600`, `error ≤ 2000`, event `detail ≤ 4 KiB`; CHECKs back the app-side caps |
| Hostile `path` strings reaching `LIKE` | Paths are never taken from the caller; they are computed under the lock and validated by the grammar before any `LIKE` |
| Pathological JSON from the model | The structured-output byte cap rejects before parse; `additionalProperties: false`; `max_repairs = 2` |

## Non-goals

- A portal UI over campaigns.
- Cross-repo campaigns. One `repo_id` per campaign in v1 (v2 note in `05-increments.md`).
- A model-facing campaign tool. The model answers questions; it does not create or edit nodes.
- Replacing `STATUS.md` or the gap docs as the source of gaps. `source_ref` on the root points at
  `gap:SI-4` or `issue:123`; the gap docs stay the source of truth (repo-knowledge D8).
- Automatic merge. The poller observes; humans and the review fleet decide.
- Moving nodes between parents (D2).

## Relationship to other tracks

- [`repo-knowledge/`](../repo-knowledge/README.md): supplies `repos(tenant, repo_id, slug)`
  (RK-02, hard prerequisite for the FK), the brief (RK-12) and the `repo_graph` tool (RK-08) that
  validates `touches`. Campaigns is the work tracker its D8 declined to be.
- [`config/`](../config/README.md) PG-01..PG-11: the migration runner, DSN resolution and
  live-test pattern (`PgDigests`, `crates/agent-digest/src/postgres.rs:44-119`) are copied.
- [`security-hardening/`](../security-hardening/README.md): S2 supplies the principal that D9
  records.
- [`multi-session/`](../multi-session/README.md): the worker runs under a `SessionKey`
  (`crates/agent-core/src/identity.rs:205`) exactly like a scheduled job.
- Review fleet: reviews campaign PRs like any other PR. No coupling in v1 (CP-10 adds the
  auto-review hook).
- Scheduler: the driver shape is borrowed (`crates/agent-runtime/src/scheduler_driver.rs:233`);
  campaigns do **not** become scheduler jobs.

## Build order

Three lanes; one PR per increment; `nix flake check` gates each. Full table in
[`05-increments.md`](05-increments.md).

| Lane | Increments | Runs after |
|---|---|---|
| A store | CP-01 seam + pure rules + memory double → CP-02 Postgres | CP-00 |
| B planner + CLI | CP-03 planner → CP-04 CLI → CP-07 RK wiring | CP-01 |
| C executor | CP-05 driver → CP-06 worker + poller → CP-08 observability | CP-02, CP-03 |

First value: CP-04 (`agent campaign add / plan / show` over agent-seddon with the fallback brief).
First autonomous PR: CP-06.

## Risks

| Risk | Mitigation |
|---|---|
| The planner splits forever or into vague leaves | D7 citeability; `max_depth`; schema enum narrowed at `max_depth − 1`; `max_plan_attempts` |
| Two drivers claim the same leaf | `FOR UPDATE SKIP LOCKED` plus owner token; adversarial concurrency test in T6 |
| A worker dies holding a lease | Lease + heartbeat every `lease / 3`; reaper returns the leaf to `ready`; attempt recorded as `lease_lost` |
| Rollup races between siblings | Ancestors locked root → leaf in one fixed order before any update |
| The brief is not there yet (RK-12) | Fallback brief from `docs/architecture.md` + `CLAUDE.md`; CP-07 swaps in the real one |
| Cost | Token caps per campaign and per leaf; attempts are the ledger |
| App-side tenancy only | Tenant on every statement; RLS is the cross-tier RK-14 |
