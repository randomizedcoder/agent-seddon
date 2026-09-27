# Campaigns — implementation progress (lane A: CP-01 → CP-02)

Crash-resilient running journal, finer-grained than [`STATUS.md`](STATUS.md) (one row per
increment). Updated after every step; the decisions log is append-only. Design contract:
[`README.md`](README.md), [`01-schema.md`](01-schema.md), [`02-transactions.md`](02-transactions.md),
[`06-test-matrix.md`](06-test-matrix.md). Two PRs, each off `main`, never stacked, each gated by
`nix flake check`.

Legend: ✅ done · 🟡 in progress · ⬜ not started · ❌ dropped

## Now

- **Next:** CP-02 is rebased onto `main` @ `71d4abf` (#501 merged) and gate-green on `campaigns/cp-02` @ `2cc7fdd` (+ this record). Push `campaigns/cp-02` and open the CP-02 PR against `main` (body drafted) once the user asks; after it merges, `STATUS.md` CP-02 → ✅ #NNN + as-built entry, then lane B / CP-03 planning.

## CP-01 — seam, pure rules, `MemCampaigns`, T1–T8 (mem) — ✅ #501 (merged 2026-09-27, `71d4abf`)

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
| T7 complete / fail (mem) | ✅ | rows: 19/19; two doc amendments for step 12: `adversarial_pr_url_long` is `TooLong` (the cap class, like `boundary_title_121`), not `Invalid`; `positive_failed_does_not_block_done_dependent` uses a `cancelled` dependent (a dependent is never `done` before its dependency, and `done` never fails) and also checks a non-dependent `ready` sibling is untouched |
| T8 approve / answer / retry / cancel / replan (mem) | ✅ | rows: 25/25; `positive_retry_blocked_task` found a store bug — a planner `blocked` (`plan_start` caps, `plan_close` reject / attempts exhausted) did not roll up; fixed in `MemCampaigns` and logged below for `PgCampaigns`; `adversarial_actor_from_arg` sweeps every non-human `Actor` variant over the seven human protocols |
| doc amendments (02 transitions, 03 attempt note, 05 row, 06 harness/dims) | ✅ | 02: (b) inputs + step 1 / step 3 comments (attempt row inside the finishing tx), planner-`blocked` rollup paragraph; 03: step 1 (no attempt row at `plan_start`, `blocked` rolls up); 05: CP-01 row (pure rules in `agent_core::campaign`, `agent-campaign` = display letters); 06: harness bullets (`campaign_conformance_suite!`, suffixed rows), T7 `adversarial_pr_url_long` → `TooLong`, `positive_failed_does_not_block_done_dependent` wording; testkit `lib.rs` doc bullets (step 11) |
| gate `nix flake check` | ✅ | 2026-09-26, green first run against the committed ref (see Gate status); an earlier dirty-tree run failed only in `portal-report-tests` because the working tree carries unrelated, uncommitted `test/**` deletions (`Path 'test/portal-report' does not exist in Git repository`) — not this branch; workspace clippy `--all-features` first surfaced the exhaustive `Error::Campaign` match in `agent-proto` (`89ecaaf`) |

## CP-02 — `PgCampaigns`, migration 0001, live suite, invariants, pg-integration — 🟡 branch `campaigns/cp-02`

| Item | State | Notes |
|---|---|---|
| feature `campaign-postgres` + deps | ✅ | `campaign-postgres = ["dep:sqlx", "dep:async-trait", "dep:serde_json"]` (no `sqlx/migrate` / `macros`); dev-deps `agent-testkit`, `tokio`, `rstest` |
| `migrations/0001_campaigns.sql` | ✅ | deviations: `IF NOT EXISTS` everywhere; named `tasks_path_key` / `task_attempts_idem_key`; `tenants` = config-store definition verbatim; conditional `tasks_repo_fk` block kept (RK-02) — recorded in `01-schema.md` "As built" |
| `postgres.rs` skeleton (connect, migrations, `with_tenant`, `with_clock`, `map_db`, `row_to_task`) | ✅ | lock key `"agcampgn"`, ledger `_campaign_migrations`; `map_db` maps by `ErrorKind` + constraint name only (idem UNIQUE → `AlreadyApplied`); `row_to_task` fails closed (`Backend`) on any undecodable column; every statement is a `const` in `postgres/sql.rs` |
| tx helpers (lock, transition, patch, bulk, rollup) | ✅ | `Tx { conn, tenant, now }` mirrors the mem `Tx`: `peek` / `lock` / `lock_ancestors` / `children_of` / `subtree_of(lock)` / `transition` (CAS + event) / `insert_attempt` / `plan_attempt` / `close_work` / `block_dependents` / `rollup_from`; subtree writes are row-by-row inside the tx (≤ 200 rows), no `bulk_cas` |
| protocols (a) (c) (e) | ✅ | (a) CTE `nextval` root insert; (c) one `WITH cand AS MATERIALIZED (… FOR UPDATE OF t SKIP LOCKED) UPDATE … RETURNING` + events/attempts per row (queue order restored in Rust); heartbeat one conditional UPDATE; reap CTE `SKIP LOCKED`; (e) node lock + CAS |
| protocols (b) + `mark_leaf` / `plan_close` | ✅ | idem checked before the lookup and again under the lock; children inserted one `INSERT … RETURNING` each; `plan_start` / `plan_close` lock the ancestors before the node (they may block → roll up) |
| protocols (d) + `resolve_review` | ✅ | `fail` / `resolve_review` lock ancestors root → parent, then the leaf; `complete` locks the leaf only (it never rolls up) |
| protocols (f) (g) | ✅ | ancestors, then the subtree `ORDER BY depth, path COLLATE "C" FOR UPDATE`; `superseded_by` patched after the state change (CHECK) |
| T3–T8 via `campaign_conformance_suite!(pg, …)` | ✅ | `postgres/tests.rs`: `pg_harness()` = reset + shared pool + harness clock; `after = assert_invariants` |
| T14 multi-tenant | ✅ | rows: 10/10 (`adversarial_tenant_string_sql` + traversal / empty / 129 as an in-gate rstest over `connect_lazy`, no statement possible) |
| T15 invariants + negatives | ✅ | `INVARIANTS` = one `WITH RECURSIVE … UNION ALL` query (path, depth, campaign/repo, > 8 children, dep not sibling, dep cycle ≤ 9 hops, leaf has children, policy on root only, lease ⇔ state, superseded_by ⇔ state, last event version); negatives plant rows that pass every CHECK |
| concurrency: `adversarial_double_claim` / `adversarial_concurrent_decompose` / `corner_reap_skips_locked` | ✅ | two pools, `multi_thread` runtime; double_claim releases by back-dating `lease_until` + `reap()` so the history stays valid |
| `nix/pg-integration.nix` + config-store `TRUNCATE … CASCADE` | ✅ | `AGENT_CAMPAIGN_TEST_DSN` exported; campaign suite block last; `contract_exit` text += campaign; config-store reset is `TRUNCATE cards, tenants CASCADE` |
| gate: pg suite local, `nix run .#pg-integration` ×2, `nix flake check` | ✅ | pg suite local green (157); `nix run .#pg-integration` green twice (second pass over the persisted volume, config-store CASCADE proven in place); `nix flake check` green on the committed ref `74c5bcd` — see Gate status |

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
- 2026-09-26 — A node the **planner** moves to `blocked` (`plan_start` at `attempts_exhausted` /
  `token_cap`, `plan_close` `reject` / attempts exhausted) runs the same rollup pass as (d) step 5
  on its ancestors; the rule's "any `failed` or `blocked` child" row is otherwise unreachable
  from (b). Found by T8 `positive_retry_blocked_task`; `PgCampaigns` must lock the ancestors in
  those three branches too (CP-02). Doc amendment for `02-transactions.md` (b) in step 12.

- 2026-09-27 — CP-02 is developed on `campaigns/cp-02` = `origin/main` (`f6809be`) + `campaigns/cp-01`
  merged in, because #501 is still open and every file CP-02 touches exists only there. The PR
  stays unstacked: once #501 merges, the branch is rebased onto `main`
  (`git rebase --onto origin/main campaigns/cp-01 campaigns/cp-02`, which replays only CP-02's
  own commits) before it opens.
- 2026-09-27 — `PgCampaigns` writes subtrees row by row inside the transaction (`transition` per
  node, ≤ 200 rows per campaign) instead of the planned `bulk_cas` / `bulk_events` helpers: the
  same code path as every other write, one event per row, and nothing to keep in step with the
  memory tier. `complete` locks the leaf only (no rollup); `fail`, `resolve_review`, `retry`,
  `cancel`, `replan`, `plan_start` and `plan_close` lock the ancestors first.

## Gate status

| When | Command | Result |
|---|---|---|
| 2026-09-26 | `cargo fmt --all -- --check` (dev shell) | green |
| 2026-09-26 | `cargo clippy --workspace --all-targets --all-features -- -D warnings` (dev shell) | red once (`E0004` non-exhaustive `Error::Campaign` in `agent-proto`, fixed in `89ecaaf`), then green |
| 2026-09-26 | `nix flake check "git+file:///…/agent-seddon?ref=refs/heads/campaigns/cp-01"` (@ `89ecaaf`) | green, `all checks passed!`; the dirty-tree form failed in `portal-report-tests` for the unrelated uncommitted `test/**` deletions, so the gate runs against the committed ref |
| 2026-09-27 | CP-02: `cargo clippy -p agent-campaign --all-targets --all-features -D warnings` | red once (`redundant_closure`), then green |
| 2026-09-27 | CP-02: `cargo test -p agent-campaign --features campaign-postgres -- --ignored --test-threads=1` against `nix run .#postgres-up` (podman) | green first run: 157 passed (T3–T8 pg + T14 + T15 + concurrency + durability); the in-gate rstest over `connect_lazy` needed `#[tokio::test]` (the lazy pool spawns on the runtime) |
| 2026-09-27 | CP-02: `cargo test -p agent-config-store --features config-store-postgres -- --ignored` against the populated campaign schema | green, 33 passed (proves `TRUNCATE … CASCADE`) |
| 2026-09-27 | CP-02: `cargo fmt --all -- --check` + `cargo clippy --workspace --all-targets --all-features -- -D warnings` | green |
| 2026-09-27 | CP-02: `CONTAINER_RUNTIME=podman nix run .#pg-integration` ×2 | green both passes (`PASS: … + campaign suites green.`); campaign suite 157/157 each time, ~78 s |
| 2026-09-27 | CP-02: `nix flake check "git+file:///…/agent-seddon?ref=refs/heads/campaigns/cp-02"` (@ `74c5bcd`) | green, `all checks passed!` |
| 2026-09-27 | End-to-end verification (plan items 2–3): `06-test-matrix.md` row ids vs `cargo test -- --list` (`mem::tN::<id>`, `pg::tN::<id>`, pg-only fns, T14/T15); relative-link check over `docs/design/campaigns/*.md` | every shared T3–T8 row present on both tiers (134 row ids; 146 mem fns, 135 pg conformance fns + 22 pg-only), T14 10/10 (`adversarial_tenant_string_sql` is the in-gate rstest, not in the `--ignored` list by design), T15 4/4; no broken links |
| 2026-09-27 | Rebase dry-run in a throwaway worktree: `git rebase --onto campaigns/cp-01 6d7b87f` (the four CP-02 commits over the cp-01 tip, standing in for post-merge `main`) | clean, no conflicts; the real rebase waits for #501 |
| 2026-09-27 | `main` advanced to `538bb34` (#502, config-store `tenants()` skip scan + one `suite!` row); dry-run `git merge origin/main` into `campaigns/cp-02` in a throwaway worktree | clean; #502 touches only `cards` and does not overlap the CP-02 `TRUNCATE … CASCADE` hunk in the same test file; the post-rebase gate covers the combined tree |
| 2026-09-27 | #501 merged (`71d4abf`, merge commit); `git rebase --autostash --onto origin/main 6d7b87f campaigns/cp-02` | clean, six commits replayed; `cargo fmt --all -- --check` + `cargo clippy --workspace --all-targets --all-features -- -D warnings` + `cargo test -p agent-campaign --features campaign-postgres` (34 in-gate) green |
| 2026-09-27 | post-rebase `CONTAINER_RUNTIME=podman nix run .#pg-integration` | green on the combined tree: config-store 34 (33 + #502's `boundary_tenants_dedups_many_per_tenant`, `CASCADE` reset in place), campaign 157/157 (108 s) |
| 2026-09-27 | post-rebase `nix flake check "git+file:///…/agent-seddon?ref=refs/heads/campaigns/cp-02"` (@ `2cc7fdd`) | green, `all checks passed!` — a cold build (the rebase changed the source hash), ~35 min alongside another nix build on the host |

## Open questions / blockers

- (none)
