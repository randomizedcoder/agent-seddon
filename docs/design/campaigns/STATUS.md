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
| CP-04 | CLI `agent campaign …` | SI-11 | 🟡 | #531 |
| CP-05 | `CampaignDriver` tick + `[campaign]` config | SI-11 | ⬜ | — |
| CP-06 | Worker `--run-task`, worktree → PR, `PrPoller`, e2e check | SI-11 | ⬜ | — |
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
