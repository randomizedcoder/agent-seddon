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
| CP-03 | Planner: prompt, schema, validation, caps, `needs_info` / `reject`, fallback brief | SI-11 | 🟡 | `campaigns/cp-03` |
| CP-04 | CLI `agent campaign …` | SI-11 | ⬜ | — |
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
  [`06-test-matrix.md`](06-test-matrix.md). No code. `docs/components/campaigns.md` is written in
  CP-08 with the metrics, not here.
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
