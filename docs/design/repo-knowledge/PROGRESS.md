# Repo knowledge — progress journal

Status board: [`STATUS.md`](STATUS.md). Sequence: [`06-increments.md`](06-increments.md).
Test matrix: [`08-test-matrix.md`](08-test-matrix.md). Design: [`README.md`](README.md).

This file is the working journal: the decisions taken as each increment lands, the gate rows
(each citing the committed hash the gate ran on), and what is deferred. It is where a reader
learns *why* the as-built code differs from the design docs.

## Now

**RK-00–02 are merged; RK-03 is published in draft PR #642, awaiting full-gate acceptance.** Current states and acceptance evidence live in
[STATUS](STATUS.md); implementation steps and dependencies live in
[IMPLEMENTATION](IMPLEMENTATION.md). RK-06 waits for RK-03, rather than running independently
of its extractor prerequisite.

## 2026-10-18 — RK-03 extraction

Implementation developed on `d648c894`, with a review handoff after this increment. Published
as commit `db9d9948` on branch `rk-03-extraction` in draft
[PR #642](https://github.com/randomizedcoder/agent-seddon/pull/642), as requested after that handoff.
Delivers Rust/Cargo/docs extraction, E1 fixtures, feature-scoped Nix execution and bench/leak
gates. RK-06 remains queued until RK-03 acceptance and merge.
Historical RK-01/02 results below are not new evidence for this implementation.

### Implementation decisions

- `agent-repo-graph::extract` exposes `RustExtractor`, `CargoExtractor`, `DocsExtractor`,
  combined `extract(root, budget)` and `ExtractionWorker`. The combined pipeline shares its
  confined file inventory; individual extractors compose through the existing builder.
  File/document hashes cover source bytes; Rust hashes cover normalized syntax without docs.
- Added `GraphBuilder::constrain_caps` (never relaxes existing limits) and `report` (insertion
  counters before endpoint resolution), so individual extractors report cap failures as well
  as the combined pipeline. No store interface, migration, runtime or default feature change.
- WalkBuilder's implicit ignore reads are disabled. Local `.gitignore` files are confined,
  byte-capped and parsed through `GitignoreBuilder::add_line`; inherited host/global rules are
  excluded. This prevents an escaping `.gitignore` symlink from bypassing the file-read boundary.
  Directory/module depth is capped at 128 and ignore matcher count follows the file budget.
- Rust parsing runs on a scoped thread: proc-macro2's span-location tables retain each parsed
  source on the parsing thread. Ending that thread releases them without invalidating spans
  owned by a caller. The async wrapper admits one blocking job, rejects busy requests, signals
  cancellation between files and retains admission until timed-out/canceled work exits.
- Matching impl blocks coalesce before emission. Generic specializations that share the
  grammar's erased self-type key receive a deterministic header-hash suffix, propagated to
  their methods. This extends cfg-only suffixing without changing the key grammar or builder's
  duplicate checks. Repeated rstest case labels share one named-case node; parent `cases` counts
  the input rows. Anonymous `_` constants have no addressable symbol node.
- Library roots keep their canonical module keys; binary/integration-test roots use
  `__bin_<target>` / `__test_<target>` modules to avoid collisions between Rust target namespaces.
  Imports/impl endpoints resolve local aliases and re-exports with a bounded cycle check;
  this is syntactic path resolution, not type checking or call resolution.
- Nix registers `repo-graph-rust`, explicitly enables extraction in bench/leak, and keeps
  fixture Markdown/TOML/source inputs under the existing `tests/fixtures/` source filter.
  The callgrind bench explicitly collects the parser thread: default benchmark toggles only
  collected the calling thread. Its initial measured full count was ~8.70M Ir; ceiling 12M.

### Validation

Base revision: `d648c8946729f75446865a4f7b970fc2a2659205`. Final gate source:
`/tmp/rk03-final-source-hjx6jsl7`; full source fingerprint
`2bb351a185569a0cbe563f29a779d4773a4dd7b45df4f661d9b6b0bbbf125136`.
The manifest is `/tmp/rk03-final-source-manifest.json`. Its code/build/fixture fingerprint
(`crates/`, `nix/`, root Cargo/flake files) is
`844fb2b5d3a1c65694cc6678ef5f22ef15acea0512d819b8eb42e2d321489ee2`;
subsequent edits to this evidence and STATUS do not change implementation inputs.

Commands below ran through the pinned `nix develop -c` environment unless they are Nix gates.

| Command / check | Result / execution | Artifact |
|---|---|---|
| `cargo test -p agent-repo-graph --features extract-rust --lib --test extraction` | PASS: 31 fixture cases + 2 worker cases, zero ignored | `/tmp/rk03-final-validation.log` |
| `cargo test -p agent-repo-graph --features extract-rust,dhat-heap --test leak` | PASS: 1 heap/allocation case | `/tmp/rk03-final-validation.log` |
| `cargo clippy -p agent-repo-graph --all-features --all-targets -- -D warnings` | PASS | `/tmp/rk03-final-validation.log` |
| `cargo bench -p agent-repo-graph --features extract-rust --bench extract` | PASS: 1 benchmark, 8,893,782 Ir against 12,000,000 ceiling; parser thread explicitly collected | `/tmp/rk03-final-validation.log` |
| Final-source Nix `repo-graph-rust` and `repo-graph` checks (`nix build`, `--no-link --no-write-lock-file --max-jobs 1`) | PASS: 33 extraction/worker cases; 34 DB-free Pg helper cases. The 57 ignored live Pg cases are not live acceptance. | `/tmp/rk03-final-graph-checks.log`, `/tmp/rk03-nix-84mdnxcb.log`, `/tmp/rk03-nix-59imf397.log` |
| Final-source Nix `bench` / `leak` | PASS: extraction benchmark 8,618,514 Ir; heap case executed successfully | `/tmp/rk03-nix-bench.log`, `/tmp/rk03-nix-leak.log` |
| `cargo test -p agent-core repo_graph`; `cargo test -p agent-testkit repo_graph` | PASS: 146 core + 57 memory/conformance cases, zero ignored | `/tmp/rk03-core-conformance.log` |
| `cargo run -p agent-repo-graph --release --features extract-rust --example extract -- /tmp/rk03-final-source-hjx6jsl7` | PASS: 25,627 nodes, 66,345 edges, 1,669 files, zero parse errors/dropped edges; 4,684 ms while Nix checks were active | `/tmp/rk03-final-smoke.log` |
| Initial `nix flake check path:/tmp/rk03-source-oi_t6nd5 --no-write-lock-file --keep-going --max-jobs 2` | FAIL: one reported failing check, `forge-ghapp`; extraction, bench/leak and remaining checks passed. This earlier snapshot had 25 fixture cases. | `/tmp/rk03-flake-check.log` |
| Final `nix flake check path:/tmp/rk03-final-source-hjx6jsl7 --no-write-lock-file --keep-going --max-jobs 2` | FAIL (exit 1): 75/76 checks succeeded; only `forge-ghapp` failed with the same baseline assertion (58 passed / 1 failed in its runtime leg) | `/tmp/rk03-final-flake-check.log` |
| Untouched baseline `nix build path:/tmp/rk03-baseline-a_a9re9i#checks.x86_64-linux.forge-ghapp --no-link --no-write-lock-file` | FAIL: same assertion as the initial full gate; 58 passed / 1 failed in the failing runtime leg | `/tmp/rk03-baseline-forge-ghapp.log` |
| Built `doc-links --repo-root . --gate` and `--orphans --gate`, after evidence edits | PASS: links resolve and first-party docs remain reachable | `/tmp/rk03-doc-links.log`, `/tmp/rk03-doc-orphans.log` |
| Markdown tables and tracker/plan audit | PASS: 7 changed Markdown files, 35 tables; 22 plan sections and matching tracker rows preserved | Local audit |
| `git diff --check` | PASS | Local working tree |

The frozen-source smoke graph hash is
`5ea21c8fd95a57b34d294ff733b6f85575d9781931d58d81ba41d7e3360c924c`.
Its report is explicitly incomplete (`skipped = 1`, `truncated = true`): `agent-seddon.png`
exceeds the 2 MiB byte cap. No missing symbol/caller/coverage claim follows from that report.
The separate fixture hash is pinned in `tests/fixtures/expected-hash.txt` and checked across
fresh directories with opposite file-creation orders.

The full-gate blocker is the existing
`fleet_review::tests::negative_ghapp_without_minter_fails_before_clone` assertion at
`crates/agent-runtime/src/fleet_review.rs:961`. It also fails on untouched `d648c894`; no runtime or
forge implementation was changed by RK-03. At publication, upstream `d7a78084` (#640) adds
synchronization to this test. `git merge-tree --write-tree HEAD origin/main` confirmed a clean
merge with that revision; the combined tree has not been validated. Integrate the upstream fix
and rerun the full gate before accepting/merging RK-03. RK-06 remains queued for that acceptance
and merge.

No live Postgres or model evaluation is required for RK-03; neither was run or claimed.
Validation used uncommitted source snapshots before publication was requested. Feature defaults
remain unchanged; publication does not promote RK-03 to accepted or merged.

## 2026-10-17 — implementation plan and tracker

- Reconciled against `ed0ab95d`, including the design extension merged in #636.
- Added the execution plan for RK-00–21 and expanded STATUS into the single implementation
  tracker, with prerequisites, acceptance evidence, milestone checklists and rollout decisions.
- Preserved RK-01/02 historical results below. The new plan/checklists are specifications, not
  newly executed implementation tests or measured feature improvements.
- Corrected stale lane guidance: Go/SCIP do not gate structural review delivery; migration
  0002/RK-10 precedes 0003/RK-15 and 0004/RK-19. RK-18's completion includes RK-12's session
  brief consumer. No runtime behavior or migration files changed.

### Documentation validation

On the uncommitted documentation change based on `ed0ab95d`:

- `nix run .#doc-links -- --gate` — PASS; relative links resolve.
- `nix run .#doc-orphans -- --gate` — PASS; first-party docs are reachable from README.
- Markdown rendering/table/anchor validation — PASS; six changed/new Markdown files, 30 tables.
- Plan/tracker audit — PASS; 22 unique RK sections and matching rows, completed foundation
  preserved, all remaining increments not started; historical evidence retained.
- `git diff --check` — PASS. No Rust, live Postgres or model evaluation suites run for this
  documentation change. The implementation test cases remain specifications in the matrix.

## Historical checkpoint — RK-02 closeout

The following checkpoint is preserved as recorded. Its scheduling language is superseded by
the current implementation plan: RK-06 needs RK-03.

**RK-02 merged (#585, gate green on `0d61ff52`).** `PgRepoGraph` is durable (lane A):
migration 0001, `with_tenant`, the `UNNEST` bulk write with the collision check, every read verb,
diff / retention / shape; the `#[ignore]` live-Postgres suite reruns the R3 rows through
`repo_graph_conformance_suite!` unchanged (46 rows) plus 11 R4 pg-only rows, run by a
`nix/pg-integration.nix` leg. Lane A is closed.

Next, runnable in parallel:

- **RK-03 — extractors `rust-syn` / `cargo` / `docs`** (lane B): the fixture workspace, the
  `repo-graph-rust.nix` determinism check (index twice ⇒ equal hash), the iai bench with an Ir
  ceiling and the dhat leak test. Depends on RK-01 only; fills the `agent-repo-graph` skeleton with
  the `Extractor` impls.
- **RK-06 — `Indexer`, `[repo_graph]` config, `agent repo …` CLI, retention, metrics** (lane C):
  now that RK-02 has landed, unblocks as soon as RK-03 also lands (needs both the store and an
  extractor).

## Decisions (RK-01)

| # | Decision | Why |
|---|---|---|
| D1 | **All pure code lives in `agent_core::repo_graph`** (`key`, `model`, `builder`, the seam), not in `agent-repo-graph`. The impl crate is a CP-01-style skeleton that re-exports the seam and grows `PgRepoGraph` (RK-02) and the extractors (RK-03). | The `agent-testkit` conformance fixtures build graphs through the one `GraphBuilder`, and testkit cannot depend on the impl crate (the campaign precedent). A **deviation** from the 06-increments RK-01 row, which placed the builder in `agent-repo-graph`. |
| D2 | Tenancy is the store handle's, never a per-call value. Reads take `Scope { repo, snapshot }` (Copy); the store is tenant-bound via `with_tenant`, fail-closed on `safe_segment`. | README D4 / `05-consumption.md`: a store view for another tenant cannot be built from a model-supplied value. |
| D3 | RK-01 ships only the **Repos, Snapshots and Reads** seam groups. The Inventory group lands with migration 0002 (RK-10) and the Facts group with 0003 (RK-15). | A trait method with no table, writer or reader is dead surface; the seam grows migration-by-migration. |
| D4 | `NodeKey` is a validated newtype with typed per-segment constructors; `NodeKind` (25), `EdgeKind` (13) and `Lang` are closed enums mirroring the DDL CHECK sets. Go `func` / `interface` map to kinds `fn` / `trait`. | `01-schema.md` grammar table; a key is model-visible, so it is validated at the boundary, once. |
| D5 | `node_id_for` = `sha256(node_key)[..8]` big-endian as `i64`; `name_tokens` = lower-cased snake/camel split, first-seen dedup, ≤ 16 × ≤ 64 B, non-ASCII dropped. | `01-schema.md`. |
| D6 | `GraphBuilder` validates every field, cfg-splits duplicate keys with an `@<sha8>` suffix, fails `finish()` on an unexplained duplicate or an id collision, sorts, assigns ids and computes `graph_hash`. `RepoGraph` has private fields (unforgeable) with a `#[doc(hidden)] with_id_fn` for the collision tests. | `02-extraction.md`: a snapshot that fails validation must never be written. |
| D7 | The builder checks paths **lexically** (`repo_relative`); `confine` needs a real root and is applied by the RK-03 file walk before opening each file. | `02-extraction.md`. |
| D8 | The seam is ~20 methods, all clamping hops / caps / list lengths and returning empty for unknown or hostile keys without echoing them; typed `RepoGraphError { NotFound, Conflict, Invalid, TooLong, Backend }` → `Error::RepoGraph`. | `03-queries.md` method table + hop / cap limits; README threat model. |
| D9 | The `Extractor` contract (`ExtractBudget`, `ExtractReport`) lives in core too — `snapshot_finish` takes the report — but no extractor is implemented in RK-01. | `02-extraction.md`; RK-03 fills it. |
| D10/D11 | `MemRepoGraph` + a conformance harness + fixtures `fixture_v1()` / `fixture_v2()` + `repo_graph_conformance_suite!` live in `agent-testkit`; RK-02 adds the `pg` tier with zero row changes. | The campaign conformance precedent. |

## Decisions (RK-02)

| # | Decision | Why |
|---|---|---|
| E1 | `PgRepoGraph` lives in `crates/agent-repo-graph/src/postgres.rs` + `postgres/sql.rs` + `postgres/tests.rs`, gated `#[cfg(feature = "repo-graph-postgres")]`; `lib.rs` re-exports it. The pure seam stays in `agent_core::repo_graph` (D1). | The `PgCampaigns` / `PgDigests` layout: impl behind the feature, one source of truth for the seam. |
| E2 | Feature `repo-graph-postgres = ["dep:sqlx", "dep:async-trait", "dep:serde_json"]` (all optional), **no `sqlx/macros` / `sqlx/migrate`**; dev-deps `agent-testkit` / `rstest` / `tokio`. | Default build stays `agent-core`-only + DB-free; avoids `sqlx-mysql`→`rsa` (RUSTSEC-2023-0071); reuses the pinned workspace sqlx (no new version). |
| E3 | Struct `PgRepoGraph { pool, tenant, now_ms }` — `Clone`, hand-written `Debug` (tenant only, never pool/DSN), **no sink** (repo-graph emits no events). `with_tenant` fail-closed on `safe_segment` before any statement; `#[doc(hidden)] with_clock`. | Mirrors the two Pg precedents; the injectable clock drives the conformance rows deterministically. |
| E4 | Migration runner copied from `PgDigests`: ledger `_repo_graph_migrations`, `MIGRATION_LOCK_KEY = 0x6167_7265_706f_6772` (`"agrepogr"`, distinct per tier), one-tx `pg_advisory_xact_lock`, `raw_sql` per version. | Exactly-once versioned schema without `sqlx::migrate!`. |
| E5 | Migration 0001 = the `01-schema.md` DDL with **named** UNIQUE constraints (`repos_slug_key`, `graph_snapshots_identity_key`, `graph_nodes_key_key`) and `CREATE TABLE IF NOT EXISTS` throughout; leads with `tenants`. | The identity constraint drives `snapshot_begin` conflicts; named constraints let `map_db` name the rule, not the row. |
| E6 | `map_db` keyed on constraint name + `ErrorKind`: identity unique → `Conflict("duplicate snapshot identity")`; other unique → `Conflict`; FK/NotNull/Check → `Invalid("constraint: …")`; `RowNotFound` → `NotFound`; else `Backend("… sqlstate <code>")`. Never message text or DSN. | The `PgCampaigns` `map_db` shape; fail-closed, never echo. |
| E7 | **Manual row decoding** (no `FromRow`): `col`/`lim`/`truncate_chars` helpers + `row_to_repo`/`row_to_snapshot`/`row_to_noderow`; `NodeKind`/`EdgeKind`/`parse_lang`/`parse_status` from text (unknown ⇒ `Backend`), JSON via `::text` → `serde_json`, time via `EXTRACT(EPOCH…)*1000`, ids straight into the newtypes. | A stored row is untrusted: a corrupt row is a `Backend` fault, never a panic (`corner_decode_corrupt_row_is_backend`). |
| E8 | **`snapshot_write` = one transaction, `UNNEST` bulk, chunked at `WRITE_CHUNK` (10 000).** `FOR UPDATE` re-read (must be `building`), count guards (`MAX_SNAPSHOT_NODES/EDGES`), bodies `ON CONFLICT DO NOTHING`, the distinct-key/same-`node_id` **collision check** ⇒ `Conflict("node id collision")` rolling the whole write back, then versions + edges. `name_tokens` comma-joined into `text[]`; `attrs` bound `text` cast `::jsonb` per row. | `03-queries.md` UNNEST contract; matches `MemRepoGraph`'s all-or-nothing; the content-addressed id means the only store-caught collision is distinct-key vs a stored body. |
| E9 | **Reads clamp in Rust, then the recursive CTEs, each after a scope preflight.** `SELECT 1 FROM graph_snapshots …` ⇒ absent scope is `NotFound` (how cross-repo/cross-tenant reads return `NotFound`); `neighbors`/`blast_radius`/`tests_covering` walk inbound on `dst_id` (min depth, ordered `(depth, key)`), `path_between` outbound on `src_id` with the `ANY(path)` cycle guard; `references` excluded; seeds excluded to match `mem`. | The seam's clamp/never-echo contract + `03-queries.md`; the R3 rows pin the exact depths/clamps. |
| E10 | **`snapshot_diff` computed in Rust** (fetch each snapshot's versions/edges, classify, sort, cap `MAX_DIFF`), not a full-outer-join in SQL; **retention** deletes non-kept `ready` + all `failed` (CASCADE) then sweeps orphan bodies (`NOT EXISTS`); `shape` via `GROUP BY`. | Byte-for-byte parity with the `mem` tier the R3 rows pin; the non-CASCADE version→node FK is why the sweep is explicit. |
| E11 | **The `pg` conformance tier reuses the R3 rows unchanged**: `postgres/tests.rs` `dsn`/`test_pool`/`reset` (TRUNCATE the five graph tables, leave `tenants`)/`tenant_store`/`pg_harness` → `repo_graph_conformance_suite!(pg, …, after = assert_invariants, ignore = …)`; `assert_invariants` checks edge endpoints in-scope + `ready` count parity. | D10/D11: RK-02 adds the `pg` tier with zero row changes; the env var is `AGENT_REPO_GRAPH_TEST_DSN`. |
| E12 | **Pg-only rows (R4)** in `postgres/tests.rs`: migration idempotence, shared-body counts, collision-leaves-building, concurrent duplicate-identity, cross-tenant reads return nothing, orphan-body sweep, connect-bad-dsn-does-not-echo, corrupt-row→`Backend`, CHECK→`Invalid`, diff cap. The **P1 in-gate units** (`with_tenant` refusals, `map_db`, and the pure `sql.rs` clamp/array/chunk rows) are non-`#[ignore]` and run in the gate. | Not expressible against `MemRepoGraph`; the `PgCampaigns` pg-only set translated. |
| E14 | **Docs deviation:** [`08-test-matrix.md`](08-test-matrix.md) reserved **R4** for RK-03 extractors in a parenthetical; RK-02 claims **R4** for its pg-only rows (per the approved plan) and adds a **P1** tier, and the reservation note is reworded so RK-03's extractor tables take fresh ids. As-built notes added to [`01-schema.md`](01-schema.md) / [`03-queries.md`](03-queries.md); the [`06-increments.md`](06-increments.md) RK-02 row gains an as-built row (feature name, E13′). | CLAUDE.md docs rule; keep the matrix's tier ids unambiguous across increments. |
| E13′ | **Beneficial deviation from the plan's E13:** RK-02 adds `nix/checks/repo-graph.nix` (feature-scoped, non-`--ignored`) so the **P1 in-gate unit tables** (`with_tenant` refusals, clamps, `UNNEST` array builders, `map_db`) run in `nix flake check`. The plan said "no new checks entry", but the general `test` check uses default features and pg-integration runs only `--ignored`, so without this the P1 rows would run nowhere in the gate. The pattern of `config-store-sqlite.nix`. | The units are behind the non-default feature; a dedicated hermetic check is the only way to execute them in the gate (the R3/R4 rows stay `#[ignore]` for `.#pg-integration`). |

## Deferred (out of RK-01 scope)

- The Inventory (`features_put`, `summary_put`, `embedding_put`, …) and Facts seam groups —
  RK-10 (migration 0002) and RK-15 (0003).
- `PerTenant` wiring and `[repo_graph]` config — RK-06.
- A `RepoGraphBackend` tenant enumeration — no consumer until RK-07.
- Bench and leak — RK-03 introduces the extractor hot path.

## Gate log

| When | Ref | Result |
|---|---|---|
| 2026-09-29 | `a023635a` | `nix flake check` on the committed ref — **all checks passed** (clippy `-D warnings`, rustfmt, tests, cargo-audit, buf, bench, leak, mt-audit, constants-sync). First full gate for the track. |
| 2026-09-30 | `0d61ff52` | `nix flake check` on the committed ref (RK-02) — **all checks passed**, including the new `repo-graph` check (the P1 in-gate units under `repo-graph-postgres`). `clippy --all-features -D warnings`, `cargo machete` and `cargo deny check bans` clean; no new `sqlx` version, no `rsa`/RUSTSEC-2023-0071. |
| 2026-09-30 | `0d61ff52` | `nix run .#pg-integration` (podman) — **PASS**, all tiers green incl. `repo-graph`: the 46 R3 conformance rows rerun on `PgRepoGraph` (`after = assert_invariants`) + the 11 R4 pg-only rows (57 ignored rows, 0 failed). Postgres torn down on the EXIT trap. |

## Open questions

Tracked in [`06-increments.md`](06-increments.md#open-questions-recorded-not-blocking); none
blocks RK-01.

## 2026-10-17 — Design extension for monitored repositories and lessons

Documentation only; no runtime, schema or Nix change, and no new implementation increment marked
complete. Extended RK-03/06–09 with deterministic extraction/report handling, atomic snapshot
publication, bounded off-path refresh and explicit stale/missing context behavior. Clarified
as-built tenant-bound `Scope` and fixture path selection for the existing Nix source filter.

Added RK-19–21 for separately stored decisions/lessons, explicit human confirmation, evidence
freshness and supersession. An `addressed` review finding is not a confirmed lesson. Model
proposals never modify structural facts. Reserved additive migration 0004 after planned 0002/0003.

Expanded the test matrix with descriptions, expected outcomes and gate/increment ownership;
added a Nix wiring and integration runbook that distinguishes existing commands from future
checks and makes successful skips insufficient for Postgres acceptance. Evaluation compares
baseline, structural graph and confirmed-knowledge arms with indexing cost reported separately.
No measurements of future behavior are claimed.
