# Campaigns — implementation progress (lane A: CP-01 → CP-02; lane B: CP-03 → CP-04)

Crash-resilient running journal, finer-grained than [`STATUS.md`](STATUS.md) (one row per
increment). Updated after every step; the decisions log is append-only. Design contract:
[`README.md`](README.md), [`01-schema.md`](01-schema.md), [`02-transactions.md`](02-transactions.md),
[`06-test-matrix.md`](06-test-matrix.md). Two PRs, each off `main`, never stacked, each gated by
`nix flake check`.

Legend: ✅ done · 🟡 in progress · ⬜ not started · ❌ dropped

## Now

- **Next:** CP-04 step 3 (runtime `campaign` / `campaign-postgres` features, `agent_runtime::campaign::open_campaign_store`, planner provider on `Agent`, `multi-tenant.toml` + fixture 10 + cli-help) on `campaigns/cp-04` (off `main` at `ed03ff1`, the CP-03 merge). CP-03 is on `main` (#525). Lane A is done — CP-01 (#501, `71d4abf`) and CP-02 (#508, `630098a`) are on `main`. Lane B scope = CP-03 (planner, T9/T10) then CP-04 (`agent campaign` CLI, Postgres store only, T16), two PRs each off `main`, never stacked.

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

## CP-02 — `PgCampaigns`, migration 0001, live suite, invariants, pg-integration — ✅ #508 (merged 2026-09-27, `630098a`)

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

## CP-03 — planner: prompt, schema, structured ask, post-validation, T9/T10 — ✅ #525 (merged 2026-09-27, `ed03ff1`)

| Item | State | Notes |
|---|---|---|
| 1. seam: `PlanCloseOutcome::Injection { field }`, `LOW_CONFIDENCE` / `plan_detail`, shared `check_deps`; mem + pg arms; t5 rows | ✅ | `check_deps` (Kahn) moved from the two private store copies to `agent_core::campaign` (11 unit rows); `plan_detail` adds `low_confidence: true` under 0.4 or non-finite on **both** `mark_leaf` and `decompose` (8 unit rows); `Injection` arm: attempt `error` = `injection: <field>`, `blocked` with `detail.reason = injection` + `detail.field`, `attempts` untouched, rollup; t5 rows `positive_plan_close_injection`, `corner_mark_leaf_low_confidence`, `corner_decompose_low_confidence` run on mem and pg via the macro list; 02 (b) tail + 06 T5 amended |
| 2. Cargo (`sha2` workspace dep, manifest, `pub mod planner`) + `hash.rs` + `schema.rs` | ✅ | `sha256_joined` (NUL-separated) + `idem_key`; `Decision`, `allowed_decisions(depth, cap)` (root never executes, `cap − 1` and deeper never split, cap 1 keeps the root rule), `decision_schema(allowed)` over the seam caps (child goal 2000, reason 600), `MAX_RESPONSE_BYTES = 1 MiB`; T9's pure schema-failure rows (25) run through `Draft07Validator` here; `agent-validate` (`validate-draft07`), `async-trait`, `serde_json`, `sha2`, `uuid` unconditional deps; `campaign-postgres = ["dep:sqlx"]` |
| 3. `prompt.rs` (random fence, 24 KiB cap, canonical hash render), `brief.rs`, `touches.rs` | ✅ | fences are `BEGIN <name> <tag>` / `END <name> <tag>` lines, tag = uuid v4 simple (32 hex) or `"0" × 32` for the hash render; `screen_inputs` names `title` / `goal` / `acceptance[i]` / `touches[i]` / `ancestor:<id>:title\|goal` / `sibling:<id>:title`; cap loop brief → siblings → ancestor goals (halving), `[truncated]` markers, `TooLarge` when the node's own block is over (multibyte flood row); `FallbackBrief` = `docs/architecture.md` (cut to what is left) + CLAUDE.md `## Conventions` / `## Security` (≤ 3 KiB), 64 KiB read cap, one source may be missing; `WorktreeTouches::check`: ≤ 200 chars, no `: % * ? [ { \`, not absolute, `safe_segment` per segment, `confine`, exists, not a symlink |
| 4. `ask.rs` (structured loop, byte cap, usage sum), `validate.rs` (03 step 4 table) | ✅ | `ask_structured`: `response_format` always set (`campaign_decision`, strict) + a schema directive only when the provider cannot constrain natively; body over 1 MiB refused unparsed (no repair); repairs as assistant / user pairs ≤ `max_repairs`; `AskError { failure: Provider \| TooLarge \| Exhausted, tokens, calls, repairs }` so a failed ask still costs what it cost (`u32` → `i64`, saturating). `post_validate(&Value, &Ctx) -> Result<Validated, ValidateError::{Injection{field}, Invalid}>`: screening first in answer order (`reason`, `question`, `acceptance[i]`, `touches[i]`, `children[i].title\|goal\|acceptance[j]\|touches[j]`), then the step-4 table (root never executes, no live children, leaf size, ≥ 1 acceptance / touch, `children` ≤ `min(8, max_children − live)`, `check_deps`, depth, `max_nodes`); 48 rstest rows (T9's `negative_*` / `corner_*` / `boundary_*` / `adversarial_*` pure half) |
| 5. `planner/mod.rs` (`Planner`, `plan_node`, `tick`), exports, tests harness, T9 | ✅ | rows: 36/36 (+ 13 extra: `corner_confidence_low_split`, `corner_brief_unavailable_still_plans`, `corner_tick_plans_the_queue`, `corner_plan_leaf_is_skipped`, the upper boundaries `boundary_acceptance_7` / `boundary_touches_13` / `boundary_title_121` / `boundary_goal_2001`, `adversarial_touches_glob` / `_backslash`, `adversarial_nan_confidence`, `adversarial_confidence_string`, `adversarial_model_label_capped`; `corner_unchanged_input_no_call` lands with the overlay in item 6; † `positive_execute_node_key` / `negative_execute_unknown_node_key` deferred to CP-07). `Planner::{new, draft07, with_route, with_max_repairs, with_max_tokens, plan_node, tick}`; `PlanOutcome::{Executed, Split, NeedsInfo, Rejected, Blocked, Errored, Skipped(NotReady \| AlreadyApplied), Conflict}` + `label()`; `Planned { calls, repairs, tokens, prompt_hash }`; `TickSummary` tallies + `failures` (store errors never fail a tick). Sequence: `plan_start` → context (root policy, ≤ 6 ancestors, live siblings, live children, `subtree` count, existing attempt keys; a store error closes the node best-effort before propagating so it cannot wedge in `decomposing`) → brief (`Err` → `[brief unavailable: …]`) → `allowed_decisions` / schema → `build_prompt` (`Screened` → `plan_close(Injection)` → `Blocked`, no call; `TooLarge` → attempt `error`) → pre-call idem scan (hit → closed under a replay key, `Skipped(AlreadyApplied)`, no call) → `ask_structured` → `post_validate` → `touches.resolve` → the write. Write errors: `Conflict` → nothing written, warn, `Conflict`; `AlreadyApplied` → `Skipped`; `Invalid \| TooLong \| Denied` → the same attempt closes `error`; `NotFound \| Backend \| LeaseLost` propagate. Answer-side injection → attempt `error` `injection: <field>: …`, node `ready`, `attempts + 1`. `low_confidence` reported on `Executed` and `Split`. Harness: `Fx` (`MemCampaigns` under `ta`, `Recorder` = `ScriptedProvider` + request log, tempdir worktree with `src/lib.rs`, `src/a..m.rs`, `docs/architecture.md`, `CLAUDE.md`), JSON builders; `tracing` dep added |
| 6. T10 + `Overlay` double | ✅ | rows: 13/13 (+ T9's `corner_unchanged_input_no_call`). `planner/tests/t10.rs`: `Overlay` delegates all 27 seam methods to a `MemCampaigns` and plants what the real tiers refuse to write — `children(parent)` may add a fabricated sibling (`adversarial_sibling_title_injection` → `Blocked{Injection}`, `detail.field = sibling:999:title`, no call), `attempts(task)` may add a closed attempt under `idem_key(tenant, task, task.version, hash)` (`corner_unchanged_input_no_call`: a first tick's `prompt_hash` replayed → `Skipped(AlreadyApplied)`, no call, node released `ready` under a distinct replay key). `adversarial_goal_screened` needs no overlay: `NewCampaign` does not screen the goal, so an injected root goal is stored and the child blocks with `ancestor:<root>:goal`. `boundary_prompt_cap` seeds five levels of eight maximal children (goal 4000, title 120, 6 × 300 acceptance, 12 touches) under a 6 KiB brief: ≤ 24 KiB, brief cut to `none [truncated]`, siblings dropped, ancestor goals halved, every fence closed, the node's own block intact. `boundary_prompt_hash_stable` = two fresh stores with identical trees → same `Planned.prompt_hash` and same `attempts()[0].prompt_hash`, different user messages of the same length. `adversarial_fence_breakout` plants the canonical `END node 0…0` line in a goal: it appears once, verbatim, inside the random-tag block. `positive_siblings_bounded` asserts 7 lines (8 live children minus self; doc says 8 — amended in item 7). Harness: `Fx::with_brief` (`StaticBrief`), `Fx::user_message(n)`, `root_and_child` / `node_at_depth` / `BAD` shared with T9 |
| 7. docs (03 / 02 / 06 / 05 amendments, STATUS 🟡, PROGRESS); gate | ✅ | 03: as-built preamble (`Planner`, own structured loop and why), `(campaign_id, path)` order, NUL-separated idem formula + the pre-call scan and replay key, screening order and the `Injection` close (`attempts` untouched; brief not screened), random fences + canonical hash render, cap tiers, step 3 cites `planner/ask.rs` (stale `agent.rs:1163` → `1184`, `lib.rs:1121` → `1135`, also in README D6), enum narrowed at `depth ≥ depth_cap − 1` with the store as backstop, `TouchResolver` paths-only until RK-08, shared `check_deps`, answer-side injection rule, `low_confidence` on execute and split, Conflict = write nothing (+ store-error best-effort close, CP-05 reaper), cost-controls pre-call scan; 02: Errors-table `Conflict` row; 06: T9/T10 harness paragraph in the legend, T9 extra-row note, `positive_siblings_bounded` → 7, `adversarial_sibling_title_injection` / `corner_unchanged_input_no_call` via `Overlay`, `adversarial_fence_breakout` wording; 05: CP-03 row; STATUS: CP-03 🟡 `campaigns/cp-03`. Verification (2026-09-27): `cargo fmt --check` clean; workspace clippy `--all-targets --all-features -D warnings` clean; `cargo test -p agent-campaign --all-features` 279 passed / 160 ignored (pg suite); `-p agent-testkit campaign` 149; `-p agent-core campaign` 290; `cargo machete` clean; `cargo deny check licenses bans sources` (the flake's scope) ok — the full `cargo deny check` fails only on `advisories` from a freshly fetched RustSec DB (`rustls` RUSTSEC-2026-0285, `rustls-pemfile` RUSTSEC-2025-0134, `proc-macro-error2` RUSTSEC-2026-0173), none touched by this branch (`Cargo.lock` differs from `main` only by `agent-campaign`'s `tracing` edge) and the flake's `cargo-audit` runs against the pinned advisory-db input — to be handled on `main` as its own change; T9 row diff: only the two † rows missing; T10: 13/13 + 1 |

## CP-04 — `agent campaign …` CLI + runtime wiring — 🟡 `campaigns/cp-04` (off `main` at `ed03ff1`)

| Item | State | Notes |
|---|---|---|
| 1. `is_hidden_control` pub; `display::escape_terminal` + rows; `PgCampaigns::ensure_migrated` | ✅ | `agent_core::is_hidden_control` is `pub` (re-exported by `pub use security::*`) so the renderer shares the one table; `escape_terminal` rewrites every `char::is_control` (C0, DEL, C1) and hidden/bidi char as `\u{..}` — 18 rstest rows (`positive_` plain / unicode / empty, `adversarial_` ANSI SGR, OSC + BEL, CRLF, bidi override + isolate, zero-width, C1 CSI, tag block, NUL, inner BOM, `boundary_` DEL / last C0, `corner_tab`) each asserting no control survives, plus `boundary_escape_terminal_output_bounded` (≤ 10 B/char); `PgCampaigns::ensure_migrated(&self)` = `run_migrations` on the store's own pool for the lazy CLI path, with the live row `positive_ensure_migrated_on_lazy_store` (lazy pool → ensure twice → one ledger row → usable under a tenant) |
| 2. `CampaignCfg` + `Config.campaign` + validator; `config/agent.toml` block; config tests | ✅ | `CampaignCfg { store, pool_max, planner_model, plan_per_tick, max_repairs, repo_root, repos: BTreeMap<String, i64> }` with the `config-schema` derive block, `#[serde(default)]` on `Config.campaign`, `validate()` called at load beside `[auth]`/`[telemetry]` (so CLI, doctor and `--check-config` all refuse a bad block): `store` ∈ `{"", "postgres"}` (trimmed; the error echoes ≤ 40 chars), `pool_max` 1..=64, `plan_per_tick` 0..=32, `max_repairs` 0..=5, `planner_model` ≤ `MAX_MODEL` chars and no control chars, `repo_root` no control chars, every `[campaign.repos]` slug `safe_segment` and id ≥ 1; `repo_root_or(working_dir)`, `planner_label(main_model)`; `config_schema::ENUM_CHOICES` gains `campaign.store`; `config/agent.toml` ships the live `[campaign]` block after `[scheduler]` (no `dsn_ref`: reuses `[config_store]`). Tests: `campaign_validate_cases` (21 rows: defaults, postgres + repos, every bound both sides, unknown / padded / huge store, control chars in `planner_model` / `repo_root`, slug traversal / leading dash / space, id 0 / negative — each error < 200 chars), `campaign_planner_model_length_rows` (128 / 129 / 100 000 chars, ASCII and multibyte fillers), `positive_campaign_defaults_match_the_shipped_reference`, `campaign_repo_root_or_rows`, `campaign_planner_label_rows`; `config_schema` (5) and `config_unknown_keys` (7) still pass |
| 3. runtime features / dep, `mod dsn` list, `campaign.rs` resolver, planner provider on `Agent`; `multi-tenant.toml`; fixture 10; cli-help require | ⬜ | |
| 4. `campaign_cli.rs` parse + refs + tests; `Mode::Campaign`, parser arm, help, `--check-config`; e2e rows | ⬜ | |
| 5. `run()` store-only verbs + `render`; early dispatch; `MemCampaigns` run-level tests; disabled-store e2e | ⬜ | |
| 6. `plan` / `run --once` via `Planner::draft07`; in-process test | ⬜ | |
| 7. docs (component doc, README / extending, 04 / 05, STATUS 🟡, PROGRESS); gate | ⬜ | T16 rows: 0/8 |

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
- 2026-09-27 — Lane B scope: CP-03 then CP-04, two PRs each off `main`, never stacked; the CLI
  store is Postgres only (`MemCampaigns` reaches the CLI only by injection in tests).
- 2026-09-27 — The planner owns its structured-output loop in `agent-campaign` (`planner/ask.rs`):
  `agent_runtime::structured` cannot be reused (agent-runtime will depend on agent-campaign for
  CP-04, and the runtime loop discards `Usage` and has no byte cap). `agent-validate` is an
  unconditional dep (`validate-draft07`) so `Planner::draft07` can build the validator.
- 2026-09-27 — Hashing: `sha2 = "0.10"` workspace dep. `prompt_hash = sha256(SYSTEM ‖ \0 ‖
  user_canonical ‖ \0 ‖ schema json)` where the user message is rendered with a canonical fence
  tag (`"0" × 32`); the messages sent use a random 32-hex tag (`uuid` v4 simple) of the same
  length, so truncation is byte-identical and model text cannot close a fence. `idem_key =
  sha256(tenant \0 task_id \0 expected_version \0 prompt_hash)`.
- 2026-09-27 — Seam additions for the planner: `PlanCloseOutcome::Injection { field }` (a prompt
  **input** hit `scan_for_injection` → `blocked`, `attempts` untouched; a hit inside the model's
  **answer** is an ordinary attempt `error` prefixed `injection: <field>` and the node returns to
  `ready`); `low_confidence` marker on both `mark_leaf` and `decompose` (03 said execute only;
  amended for symmetry so `show` can mark both); `check_deps` shared from `agent_core::campaign`.
- 2026-09-27 — `Conflict` at the finishing write: the planner writes nothing further (the attempt
  row was inside the rolled-back tx), warns and counts it; a `plan_close(Error{conflict})` would
  CAS on the same stale version. Pre-call idempotency: the planner scans `attempts(task)` for the
  computed key and skips the provider call on a hit; the store's `AlreadyApplied` stays the
  backstop.
- 2026-09-27 — Touches resolve through `TouchResolver` (`WorktreeTouches { root }`: relative,
  `safe_segment` per segment, `confine`, exists, not a symlink; node-key syntax rejected until
  RK-08 / CP-07). Brief through `BriefSource` (`FallbackBrief { repo_root }` = first 6 KiB of
  `docs/architecture.md` + CLAUDE.md `## Conventions` / `## Security` sections; not
  injection-screened because CLAUDE.md discusses injection phrases). Plannable order is the
  store's `(campaign_id, path)`.
- 2026-09-27 — CP-04: `[campaign]` config reuses `[config_store] dsn_ref` (no `dsn_ref` of its
  own); the store opens with `connect_lazy` and migrates on the first verb via
  `PgCampaigns::ensure_migrated` when `migrate_on_start`, never eagerly (fixture 10's dummy DSN
  must not dial). Store-only verbs run before metrics / `build_agent`; every untrusted string is
  rendered through `display::escape_terminal` (`is_hidden_control` becomes pub); letters are
  minted from the unfiltered listing so `A` is stable across `list` / `show` / `add`.
- 2026-09-27 — A pre-call idempotency hit is closed under a distinct *replay* key
  (`sha256("replay" ‖ \0 ‖ prompt_hash)`), not the original: the store would answer
  `AlreadyApplied` to the original key and leave the node in `decomposing`. The enum is narrowed
  at every `depth ≥ depth_cap − 1` (not only `=`) so a row deeper than the cap cannot split; the
  store's depth check stays the backstop. `TouchResolver` / `BriefSource` errors, prompt
  `TooLarge` and store `Invalid | TooLong | Denied` at the write all close the same attempt as
  `error`; only `NotFound` and backend errors leave `plan_node`, and `tick` counts those as
  `failures` without failing.

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
| 2026-09-27 | CP-03 step 1: `cargo fmt --all` + `cargo clippy -p agent-core -p agent-testkit -p agent-campaign --all-targets --all-features -- -D warnings` + `cargo test -p agent-core campaign` / `-p agent-testkit campaign` / `-p agent-campaign --all-features` | green first run: 290 / 149 / 34 in-gate (pg suite `#[ignore]`) |
| 2026-09-27 | CP-03 step 1: `CONTAINER_RUNTIME=podman nix run .#pg-integration` | green first run (`PASS: …`); campaign pg suite 160/160 (157 + the three new t5 rows), 91 s |
| 2026-09-27 | CP-03 step 7: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo test -p agent-campaign --all-features` / `-p agent-testkit campaign` / `-p agent-core campaign`, `cargo machete`, `cargo deny check licenses bans sources` | green first run: fmt clean, clippy clean, 279 (+ 160 `#[ignore]` pg) / 149 / 290, machete clean, deny ok in the flake's scope (the `advisories` scope fails on three pre-existing RustSec entries a fresh DB knows about — `rustls` 2026-0285, `rustls-pemfile` 2025-0134, `proc-macro-error2` 2026-0173 — none from this branch; see step 7 notes) |
| 2026-09-27 | CP-03 step 7: `CONTAINER_RUNTIME=podman nix run .#pg-integration` | green first run (`PASS: …`); campaign pg suite 160/160, 190 s (the planner tests compile alongside) |
| 2026-09-27 | CP-03: `nix flake check "git+file://…?ref=refs/heads/campaigns/cp-03"` (@ `609e9aa`) | green, `all checks passed!` — the first client was reaped for host memory pressure (an unrelated `nix eval` held ~105 GB) after 63 of 65 checks had built; the remaining two (`fleet-store`, `review-toolbox`) were built with `nix build` and the full check rerun from cache |

## Open questions / blockers

- `cargo deny check advisories` against a freshly fetched RustSec DB fails on `rustls`
  (RUSTSEC-2026-0285, a real vulnerability), `rustls-pemfile` (2025-0134, unmaintained) and
  `proc-macro-error2` (2026-0173, unmaintained). Not from this track (`Cargo.lock` on
  `campaigns/cp-03` differs from `main` only by `agent-campaign`'s `tracing` edge); the flake's
  `cargo-audit` runs against the pinned `advisory-db` input, so the gate does not see it yet. Needs
  its own change on `main`: bump `rustls`, then bump the `advisory-db` input.
- A node left in `decomposing` by a crash between `plan_start` and the close (the best-effort
  close cannot run if the process dies) needs a reaper — CP-05's driver tick (`reap()` already
  handles leases; `decomposing` older than a bound is the analogous rule).
