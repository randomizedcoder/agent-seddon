# 04 — Executor: driver tick, worker subprocess, PRs

## `CampaignDriver::tick`

Mirrors `SchedulerDriver::tick_with_exec` (`crates/agent-runtime/src/scheduler_driver.rs:233-290`):
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

**As built in CP-05** (`agent_campaign::driver`, `crates/agent-campaign/src/driver/mod.rs`;
runtime wiring `crates/agent-runtime/src/campaign_driver.rs`; CLI `agent campaign run [--once]`
in `crates/agent-cli/src/campaign_cli.rs`). The tick is a pure `Driver` over seams so T11 runs
it on `MemCampaigns` with doubles: the tenants come from a `CampaignBackend`
(`agent_core::campaign`; `tenants()` = `SELECT DISTINCT tenant FROM tasks WHERE state = ANY
(live)` over `ready | decomposing | claimed | running | in_review`, results `safe_segment`-filtered,
`with_tenant(t)` the bound store — the config store's `tenants()` counts config cards, which
campaigns never write), the plan phase is a `TickPlanner` (`FactoryPlanner` builds a CP-03
`Planner` per tenant per tick and runs the CLI's own loop: one `plannable` read, `plan_node` per
node), the poll phase a `PrPoller` (`NoopPoller`, zeros and no store call, until CP-06's forge
poller; the batch is the const `POLL_BATCH = 20` until `poll_batch` lands with it), the worker a
`WorkerExec`. Deviations from the sketch above, each for a reason:

- **The tick does not join its workers.** A leaf may legitimately run for `worker_timeout_secs`;
  a tick that waited on it would stop reaping and planning for every other tenant. The `JoinSet`
  and both semaphores persist on the `Driver`; each tick first *harvests* the workers that
  finished since the last one, `running(tenant)` is the tenant semaphore's permits in use, and
  the claim limit per tenant is `min(free per-tenant permits, remaining global budget)`, so
  permit acquisition (tenant first, then global) never waits. `Driver::drain(deadline)` joins with
  a deadline for once-mode, tests and shutdown; whatever it aborts holds a lease that expires and
  `reap()` returns to `ready`.
- **The shipped CP-05 driver has no exec, so its claim phase is off.** `fail` blocks dependents
  and rolls parents to `blocked`, so a stub that failed every leaf would wreck CP-04's `add →
  run --once → show`; instead `run --once` prints `claimed 0  dispatched 0  (workers: CP-06)`
  and nothing is burned. When an exec is wired (tests: `ClosureExec`; CP-06: the subprocess), a
  worker that returns `Err`, times out (`tokio::time::timeout` inside the spawned task, permits
  held through the settle write) or panics is settled by the driver — `claimed → running →
  failed` (or `running → failed`) under its owner, cause `error` / `timeout`, error text cut to
  `MAX_ERROR` — and a worker that already wrote its own terminal state is left alone
  (`LeaseLost` / `Conflict` swallowed with a warning).
- **A second reaper: `CampaignStore::reap_decomposing(max_age_secs)`.** A planner that dies
  between `plan_start` and its close cannot run the best-effort close (`03-decomposition.md`), so
  every non-leaf `decomposing` for longer than the bound (`DECOMPOSING_MAX_SECS = 900`, clamped
  like a lease) goes back to `ready` by `actor = reaper` with `detail.reason = plan_stale`, no
  attempt touched (the planner's row is written inside the finishing transaction) — the new
  `decomposing → ready | objective, task | reaper` row of `02-transactions.md`. A planner still
  alive past the bound loses its CAS at the finishing write and writes nothing. Postgres uses
  `FOR UPDATE SKIP LOCKED`, so a planner mid-write holds its row and is skipped.
- `ClaimRequest.lease_secs` is `Policy::default().lease_secs` (a claim spans campaigns); the
  per-campaign lease is honoured by the CP-06 heartbeat. Each tenant's phases run under
  `agent_core::scope(SessionKey::parse(tenant, "campaign"))`, mirroring the scheduler driver, so
  provider calls attribute to the tenant; actors are typed inside the stores (claim `driver:<owner>`,
  reap `reaper`, planner `model:<attempt>`), so nothing here can spoof one. Tracing only
  (`campaign.tick` span, one `info!` of counts per tick); metrics are CP-08.
- **Subprocess dispatch moved to CP-06.** `EnvPolicy` is only `Inherit | Scrub`
  (`agent-core/src/lib.rs`), so handing the owner token to the child through a per-exec
  variable (never an argument: arguments are visible to every process on the host) needs a new
  `ExecSpec.env_set` across `agent-core` / `agent-sandbox` / `agent-grpc` / `agent-proto`; with no
  exec in CP-05 it would be dead plumbing. CP-05 lands what T11 needs: the `[campaign] sandbox`
  key (validated, unused until CP-06) and the hidden **`agent --run-task --tenant T --task <id>`
  stub** (`campaign_cli::run_task_stub`): `AGENT_CAMPAIGN_OWNER` (`CAMPAIGN_OWNER_ENV`) missing or
  not a `safe_segment` ⇒ stderr `run-task: lease lost (owner missing)`, **exit 3**, before the
  config is read; present ⇒ **exit 4** `run-task: worker not implemented (CP-06)`, store untouched.
  The token is never printed. `--tenant` is validated fail-closed at parse, `--task` is an id
  (`[1-9][0-9]{0,17}`), and after `--` both flags are goal words.
- **CLI.** `agent campaign run` (resident) refuses unless `[campaign] enabled` — after the
  config load, before any store opens, naming the key — then builds the driver (`build_driver`:
  `[campaign]` keys → `DriverConfig`, tenants = `--tenant T`, else discovery under `[tenancy]
  per_tenant`, else `local`), prints `campaign: ticking every Ns — ^C to stop`, and one line per
  tick `tick: tenants n  reaped n  released n  planned n  claimed n  dispatched n  failed n
  errors n`; on `^C` / `SIGTERM` it drains for `worker_timeout_secs`. `run --once` is `tick()` +
  `drain()` over the one tenant the verb's store is bound to, `enabled` ignored, printing
  `reaped n  released n`, CP-04's per-node plan lines and `plan:` summary, then `claimed n
  dispatched n  failed n  (workers: CP-06)`.

## Dispatch

`sandbox = "subprocess"` (default): `agent --run-task --tenant <T> --task <id>`, a hidden mode
beside `--run-scheduled-job` (`crates/agent-cli/src/main.rs`, `parse_args_from` and
`Mode::RunTask`; a stub in CP-05, see "As built" above), inheriting the same sandbox, config path
and DSN resolution. The subprocess receives the owner token through an
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

**As built in CP-04** (`CampaignCfg`, `crates/agent-runtime/src/config.rs`): the block shipped
with the keys the CLI needs — `store` (`""` | `"postgres"`), `pool_max` (`4`, `1..=64`),
`planner_model`, `plan_per_tick` (`4`, `0..=32`), `max_repairs` (`2`, `0..=5`), `repo_root`
(`""` = `[agent] working_dir`) and `[campaign.repos]` (slug → `repo_id` until RK-02). There is
**no `dsn_ref`**: like the scheduler and digest tiers the store reuses `[config_store] dsn_ref`,
so one secret reference names the one Postgres. **CP-05** added the driver keys as tabled above
— `enabled` (`false`; `true` needs a `store`; `run --once` ignores it), `tick_secs`,
`per_tenant_workers`, `global_workers` (not cross-checked against `per_tenant_workers`: several
tenants may together exceed one tenant's share), `sandbox` (validated, dispatched in CP-06) and
`worker_timeout_secs` — each range-checked at load with an error under 200 chars naming the key
(T11 `boundary_config_floor` / `boundary_config_ceiling` as `campaign_validate_cases` rows).
`worker_model` and `poll_batch` land with the worker and the poller (CP-06). Component doc:
[`docs/components/campaigns.md`](../../components/campaigns.md).

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
