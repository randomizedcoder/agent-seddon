# Repo knowledge — progress journal

Status board: [`STATUS.md`](STATUS.md). Sequence: [`06-increments.md`](06-increments.md).
Test matrix: [`08-test-matrix.md`](08-test-matrix.md). Design: [`README.md`](README.md).

This file is the working journal: the decisions taken as each increment lands, the gate rows
(each citing the committed hash the gate ran on), and what is deferred. It is where a reader
learns *why* the as-built code differs from the design docs.

## Now

**RK-01 — `RepoGraphStore` seam, key grammar, `GraphBuilder`, `MemRepoGraph`.** In progress.
Lane A. On merge, RK-02 (Postgres, lane A) and RK-03 (extractors, lane B) become runnable in
parallel.

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

## Open questions

Tracked in [`06-increments.md`](06-increments.md#open-questions-recorded-not-blocking); none
blocks RK-01.
