# Campaigns — implementation progress (lane A: CP-01 → CP-02)

Crash-resilient running journal, finer-grained than [`STATUS.md`](STATUS.md) (one row per
increment). Updated after every step; the decisions log is append-only. Design contract:
[`README.md`](README.md), [`01-schema.md`](01-schema.md), [`02-transactions.md`](02-transactions.md),
[`06-test-matrix.md`](06-test-matrix.md). Two PRs, each off `main`, never stacked, each gated by
`nix flake check`.

Legend: ✅ done · 🟡 in progress · ⬜ not started · ❌ dropped

## Now

- **Next:** CP-01 step 10 — conformance tables `t7.rs` (complete / fail), `t8.rs`.

## CP-01 — seam, pure rules, `MemCampaigns`, T1–T8 (mem) — 🟡 branch `campaigns/cp-01`

| Item | State | Notes |
|---|---|---|
| root `Cargo.toml` member + path dep | ✅ | `crates/agent-campaign`, `default-features = false`; doc-only crate skeleton so every commit builds |
| agent-core `Error::Campaign`, `pub mod campaign` | ✅ | namespaced module, no glob re-export |
| `campaign/rules.rs` enums, `allowed()`, `rollup()`, `clamp_lease()` | ✅ | `allowed()` is a `match`; the test holds the doc table as data |
| `campaign/path.rs` `TaskPath` | ✅ | `TaskId` newtype landed in `mod.rs` with it; `root(TaskId)` is fallible (non-positive id refused) |
| `campaign/policy.rs` `Policy` | ✅ | `from_json` shape-checks keys before serde so every rejection names `policy.<key>`; `from_stored` maps to `Backend`; T4 policy rows at the pure level (3 named + 28 rejected + 15 accepted) |
| `campaign/mod.rs` errors, types, `Actor`, `CampaignStore` | ✅ | errors landed with policy (step 5); `Owner` / `IdemKey` validate on deserialize too; shared checkers `check_len` / `check_max` / `screen` / `check_list` / `truncate_chars`; `NewCampaign` / `ChildSpec` / `PlanAttempt` / `PrRef` carry `validate()`; `Actor::from_scope` never yields `Model` (a gateway constructs it) |
| crate `agent-campaign` (display letters) | ✅ | `Letters` per listing (bijective base 26, capped at 1000 roots), `letters()` / `parse_letter()`; re-exports the seam |
| testkit `MemCampaigns` | ✅ | clone-mutate-swap tx under one `Mutex`; global identities; every protocol (a)–(g) + reads; `Actor::Rollup` writes rollup and `dependency_failed` events; mem-only tests in `campaign/tests.rs` (tenant refusal incl. `adversarial_tenant_string_sql`, shared state, clock, rollback) |
| testkit conformance harness + `campaign_conformance_suite!` | ✅ | `Harness { clock, open }`, `Harness::mem()` / `from_factory`; fixtures (`campaign`, `split`, `leaf`, `ready_leaves`, `running`, `in_review`, `done`, `failed`, …); macro is three levels (`suite!` → `__campaign_table!` → `__campaign_row!`) so `after =` / `ignore =` pass through a `$(…)*`; names `mem::tN::<row>` |
| T1 path grammar | ✅ | rows: 27/27 — 26 in agent-core (+16 extra boundary / adversarial rows), `positive_display` in `agent-campaign::display` |
| T2 `allowed()` + `boundary_exhaustive` | ✅ | rows: 26/26 (+11 rows for the amendments); count = 82 of 13 × 13 × 3 × 8; rollup pure half of T3 (20 rows) and `clamp_lease` (9 rows) here too |
| T3 rollup (mem) | ✅ | rows: 15/15 (`conformance/t3.rs`) |
| T4 create (mem) | ✅ | rows: 20/20 as 24 fns (`negative_policy_out_of_range_{max_depth,max_children,max_nodes,lease_secs}`, `negative_policy_bad_level_{zero,seven}`); pg-only: the "CHECK also rejects if bypassed" half of `boundary_title_121`, the `tenants` row of `positive_tenant_ensured`; `negative_policy_unknown_key` goes through `Policy::from_json` (the typed request cannot carry a stray key) |
| T5 decompose / mark_leaf (mem) | ✅ | rows: 30/31 (`adversarial_concurrent_decompose` pg-only); `boundary_max_nodes` builds the 198-node tree with 25 splits; `negative_mark_leaf_with_children` uses a replan in flight so the has-children branch (not the CAS) is what refuses; `corner_attempt_exhausted` ends by showing `plan_start` on the blocked root is `Conflict` and `replan` resets `attempts` |
| T6 claim / heartbeat / reap (mem) | ✅ | rows: 22/24 (`corner_reap_skips_locked`, `adversarial_double_claim` pg-only); rows look at `work` attempts only (a fixture leaf also owns its `execute` attempt); lease rows assert `lease_until_ms` against the harness clock, so they run unchanged on pg; `adversarial_owner_empty` holds at `Owner::parse` (a `ClaimRequest` cannot carry a bad owner) |
| T7 complete / fail (mem) | ⬜ | rows: 0/19 |
| T8 approve / answer / retry / cancel / replan (mem) | ⬜ | rows: 0/25 |
| doc amendments (02 transitions, 03 attempt note, 05 row, 06 harness/dims) | ⬜ | |
| gate `nix flake check` | ⬜ | |

## CP-02 — `PgCampaigns`, migration 0001, live suite, invariants, pg-integration — ⬜

| Item | State | Notes |
|---|---|---|
| feature `campaign-postgres` + deps | ⬜ | |
| `migrations/0001_campaigns.sql` | ⬜ | deviations from `01-schema.md` listed here |
| `postgres.rs` skeleton (connect, migrations, `with_tenant`, `with_clock`, `map_db`, `row_to_task`) | ⬜ | |
| tx helpers (lock, transition, patch, bulk, rollup) | ⬜ | |
| protocols (a) (c) (e) | ⬜ | |
| protocols (b) + `mark_leaf` / `plan_close` | ⬜ | |
| protocols (d) + `resolve_review` | ⬜ | |
| protocols (f) (g) | ⬜ | |
| T3–T8 via `campaign_conformance_suite!(pg, …)` | ⬜ | |
| T14 multi-tenant | ⬜ | rows: 0/7 |
| T15 invariants + negatives | ⬜ | |
| concurrency: `adversarial_double_claim` / `adversarial_concurrent_decompose` / `corner_reap_skips_locked` | ⬜ | |
| `nix/pg-integration.nix` + config-store `TRUNCATE … CASCADE` | ⬜ | |
| gate: pg suite local, `nix run .#pg-integration` ×2, `nix flake check` | ⬜ | |

## Decisions log (append-only)

- 2026-09-26 — Everything pure (`allowed()`, `rollup()`, `TaskPath`, `Policy`, the seam) lives in
  `agent_core::campaign`; `agent-testkit` must not depend on `agent-campaign` (impl crates
  dev-depend on testkit, never the reverse). `agent-campaign` holds display letters now and the
  Postgres tier in CP-02.
- 2026-09-26 — The planner's `task_attempts` row is inserted inside the finishing transaction
  (`decompose` / `mark_leaf` / `plan_close`), never at `plan_start` (protocol (b) step 1; T5
  `negative_version_conflict`).
- 2026-09-26 — Time is epoch milliseconds from an injectable clock on both tiers; `PgCampaigns`
  binds `$now` where the sketches say `now()`; no `chrono` / `time`.
- 2026-09-26 — JSONB is bound as TEXT with `::jsonb` casts and read as `::text`; the sqlx `json`
  feature stays off.
- 2026-09-26 — `IdemKey` is validated by the store (64 lowercase hex) and computed by the planner
  (CP-03); no `sha2` dependency yet. `work` attempts use a synthetic key.
- 2026-09-26 — `Actor` is a typed enum; request structs carry no actor / `created_by` / path /
  ordinal / depth fields, so the spoofing rows hold by construction.
- 2026-09-26 — Transition table amended with six rows (root `ready → blocked`, `decomposing →
  cancelled`, `blocked → decomposed`, `blocked → done`, root `decomposing → awaiting_approval`,
  root `awaiting_approval → ready`); `allowed()` takes the pre-write kind; T2 sweep is
  13 × 13 × 3 × 8 actor classes; the expanded table has 82 tuples.
- 2026-09-26 — `rollup()` changes only a `decomposed` or `blocked` parent (a `decomposing` parent
  mid-replan, or a terminal one, is never touched by a child); the "all cancelled → blocked" row
  applies to a `decomposed` parent only. Recorded in `02-transactions.md` "Rollup rule".

## Gate status

| When | Command | Result |
|---|---|---|

## Open questions / blockers

- (none)
