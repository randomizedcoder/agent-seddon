# Campaigns — status

Legend: ✅ merged · 🟡 in progress · ⬜ not started · ❌ dropped

Design: [`README.md`](README.md) · sequence: [`05-increments.md`](05-increments.md) · tests:
[`06-test-matrix.md`](06-test-matrix.md) · progress journal: [`PROGRESS.md`](PROGRESS.md) · source:
[self-improvement gap analysis](../../gap-analysis/self-improvement.md) §3 SI-11.

| # | Increment | Closes | State | PR |
|---|---|---|---|---|
| CP-00 | This track, SI-11 in the gap analysis, index links | — | ✅ | #495 |
| CP-01 | `CampaignStore` seam, path grammar, `allowed()`, rollup, policy, `MemCampaigns` | SI-11 | ✅ | #501 |
| CP-02 | `PgCampaigns`, migration 0001, protocols (a)–(g), live suite, invariants query | SI-11 | ✅ | #508 |
| CP-03 | Planner: prompt, schema, validation, caps, `needs_info` / `reject`, fallback brief | SI-11 | ✅ | #525 |
| CP-04 | CLI `agent campaign …` | SI-11 | ✅ | #531 |
| CP-05 | `CampaignDriver` tick + `[campaign]` config | SI-11 | ✅ | #553 |
| CP-06 | Worker `--run-task`, worktree → PR, `PrPoller`, e2e check | SI-11 | 🟡 | #561 (poller, `campaigns/cp-06a`), #569 (worker, `SubprocessExec` / `InProcessExec`, `campaign-e2e`, `campaigns/cp-06b`); ✅ with the close-out docs PR |
| CP-07 | RK-12 brief, `touches` against `RepoGraphStore`, RK-08 tool for workers | SI-7, SI-11 | ⬜ | — |
| CP-08 | Metrics, ClickHouse events, component doc | — | ⬜ | — |
| CP-09 | gRPC `CampaignService` (`scoped`), mt-audit, constants | — | ⬜ | — |
| CP-10 | Merge webhook, re-run on "changes requested", fleet auto-review | — | ⬜ | — |

## As-built log

- **2026-09-26 — CP-00 (#495).** Opened the track from
  [`gap-analysis/self-improvement.md`](../../gap-analysis/self-improvement.md) SI-11. Decisions
  D1–D10 in [`README.md`](README.md); DDL in [`01-schema.md`](01-schema.md); protocols in
  [`02-transactions.md`](02-transactions.md); test matrices T1–T16 in
  [`06-test-matrix.md`](06-test-matrix.md). No code. `docs/components/campaigns.md` was planned
  for CP-08 with the metrics; it arrived with the CLI in CP-04 (the first user-facing surface),
  and CP-08 adds its observability section.
- **2026-09-27 — CP-01 (#501).** The `CampaignStore` seam and everything pure live in
  `agent_core::campaign` (`rules.rs` enums + `allowed()` / `rollup()` / `clamp_lease()`,
  `path.rs` `TaskPath`, `policy.rs` `Policy`, `mod.rs` errors / value types / requests / typed
  `Actor` / the trait), not in `agent-campaign`, so `agent-testkit` can host `MemCampaigns`
  without depending on the impl crate; `agent-campaign` is the thin display-letters crate until
  CP-02. `MemCampaigns` is a clone-mutate-swap transaction under one `Mutex` with an injectable
  epoch-ms clock; the T3–T8 rows are `pub async fn`s in `agent_testkit::campaign::conformance`
  stamped by `campaign_conformance_suite!` so the Postgres tier reruns them unchanged. Deviations
  from the design, all amended in the docs: the transition table gained six rows (planner
  `ready → blocked`, `decomposing → cancelled`, blocked ↔ decomposed / done rollups, objective
  `needs_info`); the planner's attempt row is inserted inside the finishing transaction, never at
  `plan_start`; a planner-`blocked` node rolls up (found by T8 `positive_retry_blocked_task`);
  `IdemKey` is validated (64 hex), computed by the planner in CP-03. Tests: T1 (27 rows, agent-core
  + `display`), T2 (26 rows + `boundary_exhaustive` = 82 of 13 × 13 × 3 × 8), T3–T8 as
  `mem::tN::<row>` (131 shared rows; the three lock-dependent rows are pg-only), mem-only
  tenant / clock / rollback tests. Deferred: runtime registry wiring and the umbrella `postgres`
  feature: CP-04/05; `sha2` idempotency keys: CP-03. Gate: `nix flake check` green on the
  committed ref (the dirty-tree form fails only in unrelated `portal-report-tests`).
- **2026-09-27 — CP-02 (#508).** `PgCampaigns` behind `agent-campaign`'s `campaign-postgres`
  feature: `migrations/0001_campaigns.sql` applied by the same versioned runner and advisory-lock
  shape as the digest and config-store tiers (ledger `_campaign_migrations`); every protocol
  (a)–(g) one transaction with the same errors, caps and rollup as `MemCampaigns`; tenant-bound
  handles that fail closed on `safe_segment` before any statement; the injectable epoch-ms clock
  bound wherever the design says `now()`; every statement a `const` in `postgres/sql.rs`;
  constraint violations mapped by kind + constraint name, never message text. Deviations from
  [`01-schema.md`](01-schema.md) (recorded there): `IF NOT EXISTS` DDL, named `tasks_path_key` /
  `task_attempts_idem_key`, `tenants` shared verbatim with the config store, so any suite that
  truncates `tenants` must `CASCADE`. Deviations from the plan: subtree writes are row by row
  inside the transaction, no bulk CAS; `complete` locks only the leaf; `claim` restores queue
  order in Rust after `RETURNING`. Tests: T3–T8 rerun unchanged as `pg::tN::<row>` with the T15
  invariants query after every case, T14 (10), T15 (4), the three lock-dependent rows
  (`adversarial_double_claim`, `adversarial_concurrent_decompose`, `corner_reap_skips_locked`),
  durability; 157 live tests under `nix run .#pg-integration`, 34 in-gate. Deferred: registry
  wiring and the umbrella `postgres` feature: CP-04/05; `repos` FK: RK-02. Gate: `nix flake
  check` green on the committed ref before and after the rebase onto `main`; `pg-integration`
  green three times.
- **2026-09-27 — CP-03 (#525).** The planner in `agent_campaign::planner`: `Planner::plan_node` /
  `tick` over the seam, with its own structured loop (`planner/ask.rs`: `response_format` always
  set, 1 MiB body cap before parsing, ≤ `max_repairs` repairs, usage summed even when the ask
  fails) because `agent_runtime::structured` sits on the wrong side of the dependency edge and
  discards usage. Prompt assembly (`prompt.rs`) screens every input — node fields, ancestor
  title / goal, sibling titles — before the render and closes a hit as `Injection` with **no
  provider call**; fences carry random 32-hex tags with a canonical render for `prompt_hash`; the
  24 KiB cap drops brief → siblings → ancestor goals with visible `[truncated]` markers.
  `hash.rs` gives the NUL-separated idem key over `(tenant, task_id, expected_version,
  prompt_hash)` and a pre-call scan skips a replayed tick without spending tokens; `schema.rs`
  narrows the decision enum by depth; `validate.rs` is the 03 step-4 table, screening the answer
  first (an injected answer costs the model an attempt, not the node); `TouchResolver` /
  `WorktreeTouches` (`safe_segment` per segment, `confine`, exists, not a symlink) and
  `BriefSource` / `FallbackBrief` (`docs/architecture.md` + CLAUDE.md sections, ≤ 6 KiB) are the
  seams CP-07 swaps for RK-08 / RK-12. Seam additions in `agent_core::campaign`:
  `PlanCloseOutcome::Injection { field }`, `LOW_CONFIDENCE` + `plan_detail` on both `mark_leaf`
  and `decompose`, shared `check_deps`. Deviations from the design, amended in the docs: planner
  owns its structured loop; `Conflict` at the finishing write means write nothing (the next tick
  re-reads); `low_confidence` is recorded on split as well as execute; `positive_siblings_bounded`
  is 7 lines (self excluded). Tests: T5 +3 conformance rows (mem + pg), T9 36/36 non-† + 13
  extra, T10 13/13 + `corner_unchanged_input_no_call` via the `Overlay` store double, unit rows
  per planner file. Deferred: † `positive_execute_node_key` / `negative_execute_unknown_node_key`
  (node keys need RK-08): CP-07; a reaper for a node wedged in `decomposing`: CP-05; three
  pre-existing RustSec advisories on `main` (`rustls` 2026-0285, `rustls-pemfile` 2025-0134,
  `proc-macro-error2` 2026-0173): their own change. Gate: `nix flake check` green on the
  committed ref; `pg-integration` green (campaign pg suite 160/160).
- **2026-09-28 — CP-04 (#531).** The first human-usable surface: `agent campaign add | plan
  [<ref>] [--max N] | list [--needs-attention] | show | approve [--children] | answer (<text> | -)
  | retry | replan | cancel | run --once` in `crates/agent-cli/src/campaign_cli.rs` (grammar,
  refs, rendering, dispatch), wired in `main.rs` as `Mode::Campaign` — the bare word `campaign`
  is the first non-option token only (after `--` it is a goal word), precedence `--check-config
  > doctor > campaign > …` (S12's `login` / `logout` / `whoami` sit ahead of all three).
  Store-only verbs run before metrics and the agent build like `doctor`; `plan` / `run --once`
  build the agent for the planner's provider and run inside the session scope. Refs are ids
  (`[1-9][0-9]{0,17}`) or letter paths (`A`, `B.2`, `AB.1.3`; ordinals `1..=8`, ≤ 6 deep)
  minted from the **unfiltered** listing so `A` is the same campaign in every verb; scripts use
  `#id`. Principal is `user:local`; `--tenant` only selects `with_tenant`. Every stored string
  reaches the terminal through `agent_campaign::display::escape_terminal` (C0 / C1 / hidden
  and bidi controls as `\u{..}`; `agent_core::is_hidden_control` made `pub`). `[campaign]`
  config (`CampaignCfg`: `store`, `pool_max`, `planner_model`, `plan_per_tick`, `max_repairs`,
  `repo_root`, `[campaign.repos]` slug → id until RK-02) validated at load;
  `agent_runtime::campaign::open_campaign_store` opens `PgCampaigns` lazily over
  `[config_store] dsn_ref` and migrates on the first real verb (`PgCampaigns::ensure_migrated`),
  so `--check-config` never dials; features `campaign` (default) / `campaign-postgres` (in the
  `postgres` umbrella); `Agent::campaign_planner_provider()` resolves `planner_model` through
  `resolve_provider_ref`. Fixed on the way: a misplaced `#[cfg(feature = "grpc")]` in
  `registry.rs` that gated `resolve_provider_ref` on `grpc`. Deviations from the design, all
  amended in the docs: no `[campaign] dsn_ref` (reuses `[config_store]`); the CLI runs its own
  tick loop over one `plannable` read so it can print a line per node (children a split creates
  wait for the next tick); `--max` is ignored when a target is given;
  `docs/components/campaigns.md` arrived here rather than in CP-08, which adds its
  observability section. Tests: T16 8/8 (`positive_add`, `positive_show_letters`,
  `negative_unknown_id`, `corner_answer_from_stdin`, `boundary_goal_file_4000`,
  `adversarial_id_traversal`, `adversarial_repo_slug`, `adversarial_source_ref_injection`) plus
  parser / ref / render rows, run-level verbs over `MemCampaigns`, the in-process `add → run
  --once → plan → show` path over `ScriptedProvider`, `escape_terminal` rows, config bounds +
  lazy-resolver rows (a 5 s timeout proves no dial), e2e help / `--` / disabled-store rows;
  `cli-help` requires `campaign`, `config-roundtrip` fixture 10 prints `campaign  = postgres`.
  Live smoke with Kimi-K3 against podman Postgres: `add` → `plan` split the root into three
  children → `approve --children` → `plan` marked two leaves and refused one whose `touches`
  named a file that does not exist yet. Deferred: touches for not-yet-existing files (node keys,
  RK-08): CP-07; a gated level is approved twice, as a task and again as a leaf (UX): recorded under PROGRESS
  open questions in CP-05, still open;
  `agent-runtime --no-default-features` failed to build on `main` (pre-existing, 21 errors):
  fixed in #546, which also added the `feature-matrix` gate so it stays fixed (the two flaky
  gate tests were fixed in #541 (`agent-runtime` `progress::tests`) and #545 (`agent-search`
  `tests/leak.rs`)). Gate: `pg-integration` green
  (161/161); `nix flake check` green on the committed ref (third pass after the two flakes)
  and again first pass on the merge of `main` (#524–#530) into the branch.
- **2026-09-28 — CP-05 (#553).** The driver tick in `agent_campaign::driver`: `Driver::tick`
  harvests finished workers, serves the tenants (a `--tenant` list, or `CampaignBackend::tenants()`
  under `[tenancy] per_tenant`, rotated round-robin) and per tenant, inside the tenant's session
  scope, runs `reap()` → `reap_decomposing()` → `PrPoller::poll` → `TickPlanner::tick` (a CP-03
  `Planner` per tenant per tick, bounded by `plan_per_tick`) → `claim` sized to
  `min(per-tenant free, global budget)`; claims are interleaved across tenants and dispatched into
  a persistent `JoinSet` under a global and a per-tenant `Semaphore`. A worker's `Err`, timeout
  (`tokio::time::timeout` inside the spawned task) or panic is settled by the driver
  (`claimed → running → failed`, error text bounded by `MAX_ERROR`); `drain(deadline)` joins the
  rest and aborts the leftovers (their leases expire and `reap()` returns them). **The shipped
  driver has no worker exec, so its claim phase is off** and `run --once` prints `claimed 0
  dispatched 0  (workers: CP-06)` — `fail` blocks dependents and rolls parents up, so a stub
  that failed every leaf would wreck CP-04's `add → run --once → show`. Seam: `CampaignStore::
  reap_decomposing(max_age_secs)` (a non-leaf `decomposing` for longer than
  `DECOMPOSING_MAX_SECS = 900` back to `ready` by `reaper`, `detail.reason = plan_stale`, no
  attempt touched; T2 exhaustive 82 → 84), `CampaignBackend { tenants, with_tenant }` over the
  `tasks` table's live states, `safe_segment`-filtered (`PgCampaigns` with `SKIP LOCKED`),
  `CAMPAIGN_OWNER_ENV`, `LIVE_STATES`. Config: `enabled` (needs a `store`), `tick_secs`
  5..=3600, `per_tenant_workers` 1..=32, `global_workers` 1..=256, `sandbox` (`subprocess` |
  `in_process`, validated now, dispatched in CP-06), `worker_timeout_secs` 60..=86400;
  `agent_runtime::campaign::open_campaign_backend`, `campaign_driver::{driver_config,
  tenants_for, build_driver}`. CLI: `agent campaign run` (resident, refused unless `[campaign]
  enabled` after the config load and before any store opens; one counts line per tick; `^C`
  drains) and `run --once` (one tick + drain, the CP-04 plan lines kept); the hidden `agent
  --run-task --tenant T --task <id>` worker mode is a stub that exits before the config is
  read (`AGENT_CAMPAIGN_OWNER` missing / unsafe ⇒ 3 `lease lost`, present ⇒ 4 `not implemented
  (CP-06)`; the token is never printed). Deviations from `04-executor.md`, recorded there under
  "As built in CP-05": the tick does not join its workers (persistent `JoinSet`, harvested per
  tick); the claim limit never waits on a permit; `lease_secs` is the policy default until the
  CP-06 heartbeat; `POLL_BATCH = 20` until `poll_batch` lands with the poller; subprocess
  dispatch (the `ExecSpec.env_set` design) moves to CP-06. Tests: T11 18/18 (15 in
  `driver/tests.rs` over `MemCampaigns` with a recording store / backend / poller, a counting
  planner and closure execs, incl. `adversarial_worker_panics` / `_hangs` (paused clock) /
  `_store_error_mid_tick`; `boundary_config_floor` / `_ceiling` as `campaign_validate_cases`
  rows; `adversarial_owner_from_env_missing` as an e2e row plus `run_task_stub_rows`), T6 +8 on
  both tiers (+1 mem-only `adversarial_tenants_never_unsafe`), T2 +5, runtime backend / driver
  builder rows, CLI parse / render / `run --once` rows, four e2e rows. Gate: `pg-integration`
  green (169/169); the manual smoke over the dev Postgres green on every path; `nix flake check`
  green on the committed ref first pass (72 checks).
