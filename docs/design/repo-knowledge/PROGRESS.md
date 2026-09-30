# Repo knowledge — progress journal

Status board: [`STATUS.md`](STATUS.md). Sequence: [`06-increments.md`](06-increments.md).
Test matrix: [`08-test-matrix.md`](08-test-matrix.md). Design: [`README.md`](README.md).

This file is the working journal: the decisions taken as each increment lands, the gate rows
(each citing the committed hash the gate ran on), and what is deferred. It is where a reader
learns *why* the as-built code differs from the design docs.

## Now

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
