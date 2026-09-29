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
node), the poll phase a `PrPoller` (the `ForgePoller` since CP-06a, below; `NoopPoller`, zeros
and no store call, when no `[forge]` backend is configured; the batch is `[campaign] poll_batch`),
the worker a `WorkerExec`. Deviations from the sketch above, each for a reason:

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
  tick `tick: tenants n  reaped n  released n  polled n  merged n  closed n  planned n  claimed n
  dispatched n  failed n  errors n` (the poll counts since CP-06a); on `^C` / `SIGTERM` it drains
  for `worker_timeout_secs`. `run --once` is `tick()` + `drain()` over the one tenant the verb's
  store is bound to, `enabled` ignored, printing `reaped n  released n`, `polled n  merged n
  closed n  awaiting n  poll_errors n` (CP-06a), CP-04's per-node plan lines and `plan:` summary,
  then `claimed n  dispatched n  failed n  (workers: CP-06)`.

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

**As built in CP-06b** (`agent_runtime::campaign_worker`,
`crates/agent-runtime/src/campaign_worker.rs`). `SubprocessExec` runs `[<current_exe>, --config,
<path>, --run-task, --tenant, T, --task, <id>]` through the process `Sandbox` (`Agent::sandbox()`)
as `ExecSpec::argv(..).env(Inherit).network(On).env_set(AGENT_CAMPAIGN_OWNER,
owner).timeout(worker_timeout + 30 s)`. `ExecSpec.env_set` landed in `agent-core` for this: names
`[A-Za-z_][A-Za-z0-9_]*`, NUL-free values, rejected before spawn otherwise, applied in
`agent-sandbox`'s `run_argv` after the `Scrub` block so every backend (local, bwrap, nix) sees it;
proto field 7 `ExecEnvVar` (additive, no `buf-image` bump). Exit mapping (`map_exit`): `0` ⇒ `Ok`;
`1` ⇒ `Err("worker failed the leaf")` (the child wrote `failed` itself, so the driver's
`settle_failure` finds a terminal state and only logs) — with `: <stderr tail>` appended when
stderr is not empty, because a child that dies **before** its own `fail` write (a config or
build error) also exits 1, and that text is then what the driver stores; `3` ⇒ `Err("worker:
lease lost")`;
`timed_out` ⇒ `Err("worker timed out after <N>s")`; anything else ⇒ `Err("worker exited <N>:
<stderr tail>")` with the tail cut to 512 chars and NUL dropped. The 30 s grace lets the child's
own `fail` write land before the parent's kill in the normal case; the driver's `worker_timeout`
still settles `Timeout` first. `InProcessExec` runs `run_leaf` under
`scope(SessionKey::parse(tenant, "campaign-<id>"))` (an unsafe tenant is `Err` before any store
call) and maps `LeafExit` to the same texts. `build_driver` picks the exec from `[campaign]
sandbox`: `"subprocess"` with no `[sandbox] backend`, no binary path (`current_exe`) or no
`--config` path is an **error naming what is missing** — the driver refuses to start rather than
fall back to in-process — and `run --once` follows the same path. `agent --run-task` is the
subprocess body: the pre-config owner check (`campaign_cli::run_task_owner`; a missing or unsafe
`AGENT_CAMPAIGN_OWNER` ⇒ `run-task: lease lost (owner missing)`, exit 3, the token never printed)
→ config load → `campaign_worker::isolate_indexes` (the driver holds the tantivy `IndexWriter`
lock on the shared `[search] index_dir`, so the child gets `<base>/campaign-<task>` — and its own
`[recall]` index when recall is on — removed after the leaf; found by the smoke, where the child
died in `build_agent` with `LockBusy`) → `build_agent_mode(Run)` → `open_campaign_store` for the
tenant (none configured ⇒
exit 1 naming `[campaign] store`) → `run_leaf` under the tenant's `campaign-<id>` session scope →
`exit(LeafExit.code())`: `Completed 0`, `Failed 1`, `LeaseLost 3`. The CP-05 exit-4 stub is gone.

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

**As built in CP-06b** (`run_leaf(agent, store, tenant, task, owner, cfg) -> LeafExit`, one body
for the subprocess, the in-process exec and the tests): the eight steps as designed, with these
deviations and details.

1. `get` then `state == claimed && claimed_by == owner`, else `LeaseLost` with nothing written.
   The root's `policy` (`Policy::default()` when unset) and up to 8 ancestor titles are read here.
3. The heartbeat is a spawned task (under `agent_core::scope_request` with the worker's
   `RequestScope`, so its writes keep the leaf's tenant identity — the served-paths spawn scan
   in `agent-grpc` enforces it) on a `tokio::time::interval` of `clamp_lease(policy.lease_secs)
   / 3` whose first beat is immediate, so it **re-leases the claim to the campaign policy's
   `lease_secs`** (the driver claimed with `Policy::default()`). `LeaseLost` flips a `watch`
   cancel the session `select!`s on; any other error only warns and the next beat retries.
4. Bindings that need no work fail the leaf **before a token is spent**: no `[git]` backend, no
   `[forge]` backend, `[forge] dry_run = true` ("a pull request cannot be opened") and `[git]
   push_policy = never` (compared trimmed and case-insensitively; **the first code that enforces
   `push_policy`**). The worktree is `worktree_remove` then `worktree_add` at `[campaign]
   target_branch` (a new key, `"main"` by default — `GitCfg` has no default-branch key) with id
   `campaign-<cid>-<path with dots as dashes>`. The branch `campaign/<cid>-<path dashed>` is
   validated **per `/`-segment** with `safe_segment`; the sentence above ("passes `safe_segment`
   trivially") was wrong, `safe_segment` rejects `/`.
5. The session is `Agent::worker_session(key, worktree, policy.max_worker_tokens_per_leaf)`:
   `session_with(key)` seeded Implement-mode, the worktree as the tool cwd, the `forge` tool
   withdrawn (the protocol opens the PR after the push), a `Spend` cap counted at the loop's one
   usage-accounting site (`BudgetExceeded` ⇒ `failed "budget: used N of cap M tokens"`, no push)
   and the `[campaign] worker_model` provider when pinned. It runs under
   `timeout(worker_timeout, select! { send, cancel })`; not through `SessionManager::admit` — the
   CLI's own `scope` already carries the tenant. The goal (`build_goal`) puts the fixed
   instructions first ("Work only inside the git worktree at …", "Run the repository's gate",
   "Commit … conventional commit message", "Do not push and do not open a pull request: the
   campaign worker pushes the branch and opens the PR after you finish"), then `Campaign:`,
   `Parents: a > b`, `Task <path>: <title>`, `Acceptance:` and `Touches:` lines, then the
   model-written goal inside `<untrusted-<uuid>> … </untrusted-<uuid>>` labelled as data, with any
   text matching the close tag stripped from the goal.
6. `checkpoint(wt, "pr")` with `oid == head` is `failed "no changes committed"`. `push(&ckpt,
   "refs/heads/<branch>")`. `Policy::authorize(ToolCall { name: "forge", arguments: { action:
   create_pr, source_branch, target_branch } })`: a `Deny` is `failed "policy denied create_pr
   (<reason>); branch <b> was pushed"`. `create_pr` with `draft = policy.draft_prs`, title
   `pr_title` (`<campaign> / <path>: <leaf>`, cut to 200 chars) and body `build_pr_body`
   (`## Acceptance` checklist, `## Touches`, the `campaign:<id> task:<path>` trailer; cut at a char
   boundary to ≤ 8 KiB with the trailer always kept). The forge's answer goes through
   `PrRef::validate` (`number 0`, a non-`https://` url ⇒ `failed "forge returned an invalid pull
   request"`).
7. `complete` with the tokens from `Spend` (clamped into `i64`) and `session_id = campaign-<id>`.
   A `LeaseLost` **here, after `create_pr`**, exits 3 with the PR URL in the log: the reaper
   re-queues the leaf and the next attempt opens a second PR. This duplicate-PR window is accepted
   (CP-10's webhook can close it) and is the one place the protocol is not idempotent.
8. `fail` with `truncate_chars(error, MAX_ERROR)`; `cause = Timeout` only for the wall clock,
   `Error` otherwise (a lost lease writes nothing). The heartbeat is aborted and the worktree
   removed on every path. The push uses the process's ambient git credentials (open question in
   `PROGRESS.md`).

## PR poller

Per tenant per tick, `in_review` leaves oldest first, batch 20 (`tasks_review` index):
`Forge::get_pr` (`crates/agent-core/src/lib.rs:4026`, the `Forge` trait).

| Forge says | Policy | Result |
|---|---|---|
| merged | `require_pr_approval = false`, or an event with `detail.pr_approved = true` exists for the leaf | `in_review → done` via protocol (d) with `actor = 'poller'`, rollup |
| merged | approval required and absent | stays; one event `detail.awaiting_pr_approval = true` (not repeated per tick) |
| closed, not merged | any | `in_review → failed`, dependents `blocked` |
| open, changes requested | any | stays (v1); CP-10 re-runs on the same branch |
| not found / error | any | stays; one error event; bounded retries per tick |
| unknown state string | any | no transition (fail closed) |

Lookups are keyed by `(tenant, task_id)`; a PR number is data on the row, never a key.

**As built in CP-06a** (`agent_campaign::driver::poller::ForgePoller`,
`crates/agent-campaign/src/driver/poller.rs`; wired by `build_driver` from `Agent::forge()`, the
process `[forge]` backend — per-repo forge cards wait for RK-02). The table above holds, with
these precisions:

- **The approval gate lives in the poller, not the store.** `resolve_review` is the poller's verb
  and neither tier reads policy for it; the poller reads the campaign root's `require_pr_approval`
  (once per campaign per batch) and scans the leaf's events for `detail.pr_approved = true`.
- **The two "stays" rows are `CampaignStore::review_note(task, ReviewNote)`**, a new seam method
  that writes an event by `poller` with no transition and no version bump (`from = to =
  in_review`, like `approve`'s `pr_approved` marker; `Conflict` on any other state):
  `AwaitingApproval` → `detail.awaiting_pr_approval = true`, written **once per leaf** (a second
  call returns `false`); `PollError(text)` → `detail.poll_error = <text>`, written every tick it
  happens, the untrusted text cut to `MAX_ERROR` chars with NUL dropped (`jsonb` cannot hold it).
  "Bounded retries per tick" is one `get_pr` per leaf per tick, never a loop.
- **Every forge value is untrusted.** `get_pr` runs under a 30 s timeout
  (`POLL_PR_TIMEOUT_SECS`); a timeout or error is a `poll_error`; a PR whose `number` is not the
  row's is a `poll_error` and never a transition; the `state` string is matched exactly
  (`open` / `merged` / `closed`) and anything else moves nothing, writes nothing and counts as an
  error (the warning shows at most 40 escaped chars of it). "Changes requested" is `open` to the
  poller (v1; CP-10).
- **Counts.** `PollReport { polled, merged, closed, awaiting, errors }` per tenant; `[campaign]
  poll_batch` (`20`, `1..=200`) sizes the batch. Without a `[forge]` backend the driver runs the
  `NoopPoller` and warns once at build that leaves in review are never resolved.

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
| `target_branch` | `"main"` | a branch of path-safe `/`-segments, ≤ 128 chars (CP-06b) |
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
**CP-06a** added `poll_batch` (`20`, `1..=200`, `campaign_validate_cases` rows). **CP-06b** added
`worker_model` (`""` = the main provider; resolved in the builder exactly like `planner_model`
into `Agent::campaign_worker_provider()`, which the worker session's turns use) and
`target_branch` (`"main"`; at most 128 chars, every `/`-segment a `safe_segment`, rows
`negative_target_branch_empty`, `adversarial_target_branch_{traversal, leading_dash, space,
control, trailing_slash}`, `positive_target_branch_nested`): the revision every worker worktree
is added at and the PR's target — `GitCfg` has no default-branch key, so the campaign block
carries it. Component doc:
[`docs/components/campaigns.md`](../../components/campaigns.md).

## Observability (CP-08)

Metrics: `agent_campaign_nodes_total{tenant,kind,state}`, `agent_campaign_attempts_total
{tenant,kind,outcome}`, `agent_campaign_tokens_total{tenant,kind}`, `agent_campaign_claims_total
{tenant}`, `agent_campaign_leases_lost_total{tenant}`, `agent_campaign_tick_seconds`. Every
`task_events` row is also emitted as an `agent_events` row in ClickHouse (`nix/clickhouse/schema.sql`)
with `kind = 'campaign'`. Hostile token counts from a provider are clamped to `≥ 0` before
`inc_by`.

**As built in CP-08.** Two seams, no new dependency between crates:

- **The event mirror is the store's.** `agent_core::campaign::EventSink { emit(tenant,
  campaign, &TaskEvent) }`; `MemCampaigns::with_sink` / `PgCampaigns::with_sink` (shared by
  every tenant view) buffer each transaction's rows and emit them **after the commit**, in
  write order — the memory tier after its clone-mutate-swap with the lock released, the
  Postgres tier in `Tx::commit` after `COMMIT` — so a rolled-back write mirrors nothing. The
  row's `campaign_id` rides along (the two reap statements return it beside the task; every
  other writer has the row); `INSERT_EVENT … RETURNING event_id` gives the mirrored row its
  real id. `agent_telemetry::TelemetryHandle` implements the sink: `session_id =
  campaign-<id>`, `user` = tenant, `role` = the actor **class** (`actor_class`: the lease
  token after `worker:` / `driver:` is never written), `content` = the event as JSON through
  the shared redaction, `detail` bounded at 8 KiB and the row at 16 KiB. The CLI installs the
  process's handle at every store open (verbs, the driver's backend, the `--run-task` child),
  so whichever process performs a write mirrors its own rows; `--check-config` opens with no
  sink.
- **Metrics are the driver's.** `agent_campaign::TickObserver { on_tick(&TickReport,
  elapsed), on_drain(&DrainReport) }` (`Driver::with_observer`; never called for a disabled
  driver) keeps `agent-campaign` free of `agent-metrics`, the scheduler's `RunObserver`
  pattern; `agent_runtime::campaign_metrics::MetricsObserver` walks the report. Because a
  `subprocess` worker's registry dies with the child, `Settled` now carries the `tokens` and
  `model` of the leaf's latest `work` attempt, read back from the store when the driver
  settles the leaf, and `PlanReport` carries the planner's `model` — so `attempts_total`
  gained a `model` label (the parked "failed leaves per planner model" question is a PromQL
  ratio), `tokens_total` a `direction` label, and three families the report makes free were
  added: `agent_campaign_plans_released_total{tenant}`, `agent_campaign_polls_total{tenant,
  outcome}`, `agent_campaign_tick_errors_total`. `nodes_total{tenant,kind,state}` counts what
  the **plan phase** hands back (the node each outcome left behind; a split's children as
  `kind="task", state="created"`) — an exact per-transition count would need the store funnel,
  which the child owns. The `tenant` label is `safe_segment`-validated at the recorder and
  admitted into the shared tenant LRU (`TenantSeries::Campaign`); `model` folds to `other`
  unless short and plain, `unknown` when unread; tick seconds are clamped finite / ≥ 0; a zero
  add mints no series. Bench ceilings moved for the nine families (`new_registry` ~1.547M Ir,
  `record_and_encode` ~2.50M). Component doc:
  [`docs/components/campaigns.md` §Observability](../../components/campaigns.md#observability).

## Interaction with the review fleet

None in v1. A campaign PR is a PR; if the fleet watches the repo it reviews it. CP-10 adds an
explicit hook so the fleet reviews campaign PRs first and the poller reads the verdict.
