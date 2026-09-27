# 04 — Executor: driver tick, worker subprocess, PRs

## `CampaignDriver::tick`

Mirrors `SchedulerDriver::tick_with_exec` (`crates/agent-runtime/src/scheduler_driver.rs:233-288`):
tenants in rotated order (`rotated_tenants`, `:208`), a `JoinSet` and `Semaphore`s (`:43-44,266`).

```
tick():
  for tenant in rotated_tenants():                  # round-robin start
      s = store.with_tenant(tenant)
      s.reap()                                      # protocol (c): expired leases → ready
      s.poll_prs(batch = 20)                        # in_review → done | failed
      planner.plan(s, n = plan_per_tick)            # 03-decomposition, bounded
      free = per_tenant_workers − running(tenant)
      claims[tenant] = s.claim(owner, n = free, lease = policy.lease_secs)   # protocol (c)
  for (tenant, task) in interleave(claims):         # A,B,C,A,B,C …
      permit_t = per_tenant[tenant].acquire()       # fixed order: tenant first,
      permit_g = global.acquire()                   #             then global (no deadlock)
      joinset.spawn(dispatch(tenant, task, permit_t, permit_g))
  joinset.join_all_with(timeout = worker_timeout_secs)   # a timeout marks the leaf failed
```

`owner` is a random 128-bit hex token minted once per driver process. `running(tenant)` counts
this process's live workers for that tenant; `in_review` leaves do not count.

The store used by the tick is `PerTenant<PgCampaigns>` (`crates/agent-runtime/src/tenant.rs:71`)
built from `PgCampaigns::with_tenant`, exactly like the scheduler's `StoreScheduler::with_tenant`
(`crates/agent-scheduler/src/store.rs:123`). A store error for one tenant is logged and the tick
continues with the next tenant; permits are released by drop.

## Dispatch

`sandbox = "subprocess"` (default): `agent --run-task --tenant <T> --task <id>`, a hidden mode
beside `--run-scheduled-job` (`crates/agent-cli/src/main.rs:761,327`), inheriting the same
sandbox, config path and DSN resolution. The subprocess receives the owner token through an
environment variable, not an argument (arguments are visible to every process on the host). Exit
code 0 means the worker wrote its own terminal state; any other exit makes the driver call
`fail(task, owner, 'worker exited <code>')`, which is a no-op `LeaseLost` if the worker already
completed.

`sandbox = "in_process"` (tests, single-user CLI `agent campaign run --once`): the same worker
function under `agent_core::scope(SessionKey, fut)` (`crates/agent-core/src/identity.rs:305`).

## Worker protocol (`--run-task`)

1. `PgCampaigns::with_tenant(T)`; `get(task)`; require `state = 'claimed' AND claimed_by = owner`,
   else exit `LeaseLost` without touching anything.
2. `claimed → running` (event, `actor = driver:<owner>`).
3. Start a heartbeat task every `lease / 3`; a heartbeat returning 0 rows aborts the session
   (cancel token) and the worker exits without pushing.
4. `RepoBackend::worktree_add` (`crates/agent-core/src/lib.rs:5284`) at the repo's default
   branch, as the review fleet does per PR (`crates/agent-review-fleet/src/orchestrator.rs:675`).
   A stale worktree left by a crashed run is removed first (`worktree_remove`, `:5288`).
   Branch name: `campaign/<campaign_id>-<path with dots as dashes>` (digits and dashes only, so it
   passes `safe_segment` trivially).
5. Build the goal from a Rust template: the campaign title, ancestor titles, this leaf's title,
   the acceptance list, the `touches` list, then the model-written goal inside a random-tag fence
   labelled untrusted, then fixed instructions: work only in this worktree, run the repo's gate,
   commit with a conventional message, do not push. Run an Implement-mode session
   (`TaskMode::Implement`, `crates/agent-core/src/lib.rs:5310`) through
   `SessionManager::admit` (`crates/agent-runtime/src/agent/session_manager.rs:135`) under the
   `SessionKey` `<tenant>/campaign-<task_id>`, with the token budget
   `policy.max_worker_tokens_per_leaf` and the wall clock `worker_timeout_secs`. The RK-08
   `repo_graph` tool is in the worker's tool set when configured (CP-07).
6. On session success: require at least one new commit on the branch (a clean tree is a failure
   "no changes"); `RepoBackend::push` (`:5292`); `Forge::create_pr`
   (`CreatePrRequest`, `crates/agent-core/src/lib.rs:3954`, `:4007`) with `draft =
   policy.draft_prs`, title `<campaign title> / <leaf path>: <leaf title>`, body = acceptance
   list, `touches`, `campaign:<id> task:<path>` trailer, ≤ 8 KiB. Forge writes are policy-gated by
   the caller exactly as elsewhere; a denial is a `failed` leaf with the policy name in `error`.
7. `complete(in_review, pr_number, pr_url, branch)` (protocol (d)), attempt `pr`.
8. On session error, timeout, or lease loss: `fail(...)` with the bounded error and attempt
   `error | timeout | lease_lost`; worktree removed either way.

Nothing the worker writes to the store omits `AND claimed_by = $owner`.

## PR poller

Per tenant per tick, `in_review` leaves oldest first, batch 20 (`tasks_review` index):
`Forge::get_pr` (`crates/agent-core/src/lib.rs:4000`).

| Forge says | Policy | Result |
|---|---|---|
| merged | `require_pr_approval = false`, or an event with `detail.pr_approved = true` exists for the leaf | `in_review → done` via protocol (d) with `actor = 'poller'`, rollup |
| merged | approval required and absent | stays; one event `detail.awaiting_pr_approval = true` (not repeated per tick) |
| closed, not merged | any | `in_review → failed`, dependents `blocked` |
| open, changes requested | any | stays (v1); CP-10 re-runs on the same branch |
| not found / error | any | stays; one error event; bounded retries per tick |
| unknown state string | any | no transition (fail closed) |

Lookups are keyed by `(tenant, task_id)`; a PR number is data on the row, never a key.

## Dependencies

`depends_on` is honoured in the claim CTE (protocol (c)): a leaf is claimable only when every
dependency is `done`. A failed dependency blocks its dependents in the same transaction as the
failure. Dependencies are siblings only; ordering across subtrees comes from the `ORDER BY
campaign_id, path` claim order, which is a preference, not a guarantee.

## Config

`[campaign]` beside `SchedulerCfg` (`crates/agent-runtime/src/config.rs:412-465`), same
`deny_unknown_fields` and load-time validation style:

| Key | Default | Bounds |
|---|---|---|
| `enabled` | `false` | — |
| `tick_secs` | `30` | `5..=3600` |
| `per_tenant_workers` | `2` | `1..=32` |
| `global_workers` | `8` | `1..=256` |
| `plan_per_tick` | `4` | `0..=32` |
| `sandbox` | `"subprocess"` | `subprocess \| in_process` |
| `planner_model` | the default provider | a registered provider name |
| `worker_model` | the default provider | a registered provider name |
| `worker_timeout_secs` | `3600` | `60..=86400` |
| `poll_batch` | `20` | `1..=200` |
| `dsn_ref` | none | `env:` / `file:` reference (`crates/agent-runtime/src/store_backend.rs:26`) |

`lease_secs` lives in the campaign policy (per campaign), not here; the config floor of `60`
applies to it as well.

## Observability (CP-08)

Metrics: `agent_campaign_nodes_total{tenant,kind,state}`, `agent_campaign_attempts_total
{tenant,kind,outcome}`, `agent_campaign_tokens_total{tenant,kind}`, `agent_campaign_claims_total
{tenant}`, `agent_campaign_leases_lost_total{tenant}`, `agent_campaign_tick_seconds`. Every
`task_events` row is also emitted as an `agent_events` row in ClickHouse (`nix/clickhouse/schema.sql`)
with `kind = 'campaign'`. Hostile token counts from a provider are clamped to `≥ 0` before
`inc_by`.

## Interaction with the review fleet

None in v1. A campaign PR is a PR; if the fleet watches the repo it reviews it. CP-10 adds an
explicit hook so the fleet reviews campaign PRs first and the poller reads the verdict.
