# Repo knowledge — status

Legend: ✅ merged · 🟡 in progress · ⬜ not started · ❌ dropped

Design: [`README.md`](README.md) · sequence: [`06-increments.md`](06-increments.md) · journal:
[`PROGRESS.md`](PROGRESS.md) · tests: [`08-test-matrix.md`](08-test-matrix.md) · source:
[self-improvement gap analysis](../../gap-analysis/self-improvement.md).

| # | Increment | Closes | State | PR |
|---|---|---|---|---|
| RK-00 | This track, the self-improvement gap analysis, index links | — | ✅ | #495 |
| RK-01 | `RepoGraphStore` seam, model, key grammar, validation, `GraphBuilder`, `MemRepoGraph` | SI-1 | ✅ | #582 |
| RK-02 | `PgRepoGraph`, migration 0001, `with_tenant`, bulk write, read verbs, live suite | SI-1, SI-3 | ⬜ | — |
| RK-03 | Extractors `rust-syn`, `cargo`, `docs`; fixture workspace; determinism check; bench + leak | SI-2 | ⬜ | — |
| RK-04 | Go mapping from the helper JSON + `_test.go` scan | SI-2 | ⬜ | — |
| RK-05 | SCIP spike report + `extract-scip` extractor (resolved calls / references / tests) | SI-2 | ⬜ | — |
| RK-06 | `Indexer`, `[repo_graph]` config, `agent repo add\|index\|status\|diff`, retention, metrics | SI-1, SI-3 | ⬜ | — |
| RK-07 | Fleet index hook (merge-base, off-path), tenant view, `(tenant, row.id)` cache key | SI-8 | ⬜ | — |
| RK-08 | `repo_graph` tool + `PgAst` engine + registry wiring + component doc | SI-4 | ⬜ | — |
| RK-09 | `RepoKnowledgeCollector`, `ReviewFacts.repo_knowledge`, renderer, `ReviewCfg.repo_knowledge` (off) | SI-5 | ⬜ | — |
| RK-10 | Inventory skeleton + migration 0002 + `agent repo inventory` | SI-6 | ⬜ | — |
| RK-11 | Cited summaries + embeddings + `Similar` / `Feature` questions + `agent repo summarize` | SI-6 | ⬜ | — |
| RK-12 | Implement / Design-mode brief hook + `agent repo brief --goal` | SI-7 | ⬜ | — |
| RK-13 | `RepoGraphService` gRPC (`scoped`), mt-audit, constants, `--serve-repo-graph` | — | ⬜ | — |
| RK-14 | Postgres RLS across Pg tiers (filed under multi-tenancy plane 02) | — | ⬜ | — |
| RK-15 | Profile extractors + migration 0003 (`repo_facts`) + fixture extension | SI-9 | ⬜ | — |
| RK-16 | History extractors: `repo_history`, `co_changes_with`, in-flight, change recipes | SI-10 | ⬜ | — |
| RK-17 | Derived analyses: chokepoints, guard funnels / gaps, near-duplicates, orphans | SI-5 | ⬜ | — |
| RK-18 | LLM extras: conventions, glossary, playbooks, pitfalls, L0 overview; layered briefs | SI-10 | ⬜ | — |

## As-built log

- **2026-09-26 — RK-00 (#495).** Opened the track from
  [`gap-analysis/self-improvement.md`](../../gap-analysis/self-improvement.md). Decisions D1–D10
  recorded in [`README.md`](README.md). No code. The `docs/components/repo-graph.md` stub is
  written in RK-08 with the tool, not here.
- **2026-09-29 — RK-01 (#582).** The pure core landed: the `RepoGraphStore` seam (Repos /
  Snapshots / Reads groups), `NodeKey` grammar + closed kind enums, `node_id_for` / `name_tokens`,
  `GraphBuilder` (validation, cfg-dup suffixing, deterministic `graph_hash`, unforgeable
  `RepoGraph`), the `Extractor` contract, and the `MemRepoGraph` double + conformance suite. **All
  pure code lives in `agent_core::repo_graph`, not `agent-repo-graph`** (deviation D1: testkit
  cannot depend on the impl crate; `agent-repo-graph` is a re-exporting skeleton that RK-02 / RK-03
  fill). Tests R1 (key grammar) + R2 (builder) in `agent-core`, R3 conformance + mem-only rows in
  `agent-testkit` ([`08-test-matrix.md`](08-test-matrix.md)). Gate green on `a023635a`
  ([`PROGRESS.md`](PROGRESS.md)). Unblocks RK-02 (Postgres, lane A) and RK-03 (extractors, lane B),
  runnable in parallel.
