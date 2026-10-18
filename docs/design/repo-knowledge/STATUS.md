# Repo knowledge — implementation status

[Implementation plan](IMPLEMENTATION.md) · [Design](README.md) ·
[Increment map](06-increments.md) · [Test matrix](08-test-matrix.md) ·
[Integration/evaluation](10-integration-and-evaluation.md) · [Journal](PROGRESS.md)

## Current baseline and next work

Last reconciled **2026-10-18**, implementation `db9d9948` based on `d648c894` (includes plan PR #638).
**3 of 22 increments are merged: RK-00–02.** RK-00 is documentation; RK-01/02 are the
implemented graph foundation. The 19 remaining increments include optional and externally
owned work; this count is not an estimate of effort or feature readiness.

**Active: [RK-03 — extraction](IMPLEMENTATION.md#rk-03).** Implementation started
2026-10-18 on `d648c894`; its prerequisite is merged. Published as draft
[PR #642](https://github.com/randomizedcoder/agent-seddon/pull/642), branch `rk-03-extraction`,
after the requested local handoff; full-gate acceptance remains outstanding.
RK-06 follows once extraction lands. Deliver structural review context before confirmed lessons;
Go/SCIP do not block that path. Migration order is 0002/RK-10 → 0003/RK-15 → 0004/RK-19.

RK-03 extractors, fixtures and Nix wiring are published behind `extract-rust`;
acceptance is recorded in its journal entry. Indexing and graph/knowledge consumers remain
planned and disabled by default. Historical RK-01/02 acceptance below is preserved separately.

## State and update rules

| State | Meaning |
|---|---|
| Not started | No implementation is underway; listed prerequisites may still be pending |
| In progress | Implementation has started; link the working PR and remaining acceptance work |
| Blocked | Work cannot continue; name the concrete blocker, dependency and unblock action |
| Implemented awaiting validation | Deliverables exist, but required checks/integration/evaluation evidence is incomplete |
| Merged | Deliverables and required acceptance are evidenced and the PR has merged |
| Deferred | Explicitly postponed; record the reason and reconsideration condition |

Update a row when work starts, validation completes or a PR merges. Evidence must identify the
revision, command/features, result, nonzero execution counts where relevant, and artifact link.
Record pass, fail, skipped, unavailable or not run explicitly; skipped/unavailable never means
pass. Keep detailed logs and deviations in PROGRESS, linked from the row. Do not promote a row
solely because code compiled or a PR merged with validation outstanding.

## Increment tracker

Each ID links to its implementation steps and acceptance criteria. Prerequisites are delivery
constraints, not claims that an unstarted row is actively blocked. The test matrix owns detailed
case descriptions and expected outcomes; add missing scenario tables in the owning increment.

| ID / plan | Deliverable | Prerequisites / blockers | State | Acceptance evidence / remaining work | PR / commit |
|---|---|---|---|---|---|
| [RK-00](IMPLEMENTATION.md#rk-00) | Design track | — | Merged | Existing design/index links | #495 |
| [RK-01](IMPLEMENTATION.md#rk-01) | Core graph and memory store | RK-00 | Merged | R1/R2/R3; gate `a023635a` | #582 |
| [RK-02](IMPLEMENTATION.md#rk-02) | Postgres graph store | RK-01 | Merged | P1/R3/R4; gate `0d61ff52`; live 57/57 | #585 |
| [RK-03](IMPLEMENTATION.md#rk-03) | Rust/Cargo/docs extraction | RK-01 (met) | Implemented awaiting validation | E1 33/33, heap 1/1, Ir/lint, both graph Nix checks and 203 existing graph cases pass; full gate 75/76: `forge-ghapp` failure reproduced on untouched `d648c894`. Upstream #640 adds test synchronization; merged-tree gate remains required before acceptance/merge; [journal](PROGRESS.md#2026-10-18--rk-03-extraction) | [Draft #642](https://github.com/randomizedcoder/agent-seddon/pull/642), `db9d9948` |
| [RK-04](IMPLEMENTATION.md#rk-04) | Go extraction (additional) | RK-03 | Not started | Pending: Go mapping/helper fixture cases | — |
| [RK-05](IMPLEMENTATION.md#rk-05) | SCIP spike and extraction (additional) | RK-03 | Not started | Pending: spike, resolved-edge/offline or integration evidence | — |
| [RK-06](IMPLEMENTATION.md#rk-06) | Indexer, atomic publication and CLI | RK-02, RK-03 | Not started | Pending: I1, applicable I2, strict Pg runner | — |
| [RK-07](IMPLEMENTATION.md#rk-07) | Fleet refresh and tenant cache | RK-06 | Not started | Pending: F1, fleet I2 | — |
| [RK-08](IMPLEMENTATION.md#rk-08) | Graph tool and PgAst | RK-06 | Not started | Pending: R5, AST/config/feature composition | — |
| [RK-09](IMPLEMENTATION.md#rk-09) | Review collector and A/B evaluation | RK-07, RK-08 | Not started | Pending: C1, full I2, Q1 A/B | — |
| [RK-10](IMPLEMENTATION.md#rk-10) | Inventory skeleton; migration 0002 | RK-06 | Not started | Pending: inventory fixtures, store/migration cases | — |
| [RK-11](IMPLEMENTATION.md#rk-11) | Cited summaries and embeddings | RK-10 | Not started | Pending: provider/citation/embedding/store cases | — |
| [RK-12](IMPLEMENTATION.md#rk-12) | Implement/Design brief hook | RK-11 | Not started | Pending: session/CLI/brief composition | — |
| [RK-13](IMPLEMENTATION.md#rk-13) | Remote graph service (optional) | RK-08 | Not started | Pending: scoped RPC, buf, mt-audit, constants | — |
| [RK-14](IMPLEMENTATION.md#rk-14) | Cross-tier RLS (external, optional) | Multi-tenancy plane 02 | Not started | Pending: external RLS implementation and evidence | — |
| [RK-15](IMPLEMENTATION.md#rk-15) | Repository profile; migration 0003 | RK-03, RK-10 (migration order) | Not started | Pending: profile fixtures, Facts conformance, Pg upgrade | — |
| [RK-16](IMPLEMENTATION.md#rk-16) | History and recipes (additional) | RK-06, RK-15 | Not started | Pending: synthetic history, persistence/composition | — |
| [RK-17](IMPLEMENTATION.md#rk-17) | Derived analyses (additional) | RK-09, RK-16; RK-05 for orphans | Not started | Pending: analysis/collector fixtures and coverage labels | — |
| [RK-18](IMPLEMENTATION.md#rk-18) | LLM extras and layered briefs (additional) | RK-11, RK-16, RK-12 | Not started | Pending: cited extras, recipe validation, both briefs | — |
| [RK-19](IMPLEMENTATION.md#rk-19) | Knowledge store; migration 0004 | RK-10, RK-15 | Not started | Pending: K1, applicable K2/K5, migration/races | — |
| [RK-20](IMPLEMENTATION.md#rk-20) | Operator workflow and candidate adapter | RK-19, RK-09, RK-11 | Not started | Pending: K2/K3, live K5 lifecycle | — |
| [RK-21](IMPLEMENTATION.md#rk-21) | Confirmed knowledge briefs and A/B/C evaluation | RK-20, RK-12 | Not started | Pending: K4, remaining K5, Q1 A/B/C | — |

## Milestone acceptance

- [x] **Foundation:** RK-00–02 merged with historical evidence in the as-built log below.
- [ ] **Structural review context:** RK-03/06/07/08/09 merged; E1/I1/F1/R5/C1 and full I2
  accepted; baseline/graph evaluation report linked.
- [ ] **Inventory and session context:** RK-10/11/12/15 merged; migrations 0002/0003,
  cited inventory and bounded session composition accepted.
- [ ] **Confirmed decisions and lessons:** RK-19/20/21 merged; migration 0004, K1–K5 and
  three-arm evaluation report accepted. Operator confirmation and evidence freshness enforced.
- [ ] **Additional analysis:** RK-04/05/16/17/18 accepted, including SCIP-dependent orphan
  analysis. Any deferred increment is explicitly recorded rather than silently counted complete.
- [ ] **Optional remote service:** RK-13 accepted with protocol/tenancy governance evidence.
- [ ] **Optional RLS:** RK-14 has linked acceptance from the multi-tenancy track.

## Evaluation and rollout decisions

Implementation delivery and the decision to change defaults are tracked separately. A completed
report may show that quality thresholds failed; preserve that result and keep the feature opt-in.

| Decision / report | State | Required evidence | Result / artifact |
|---|---|---|---|
| RK-09 baseline versus graph | Not run | Q1 paired A/B, held-out findings, cold index and warm review costs | — |
| RK-21 baseline versus graph versus lessons | Not run | Q1 paired A/B/C with frozen confirmed knowledge | — |
| Default enablement | Not proposed | All runbook correctness/quality/latency criteria; separate reviewed config PR | Features remain off |

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
- **2026-09-30 — RK-02 (#585).** The graph is durable: `PgRepoGraph`, the Postgres impl of the
  seam, behind the non-default `repo-graph-postgres` feature (`dep:sqlx`/`async-trait`/`serde_json`,
  no `sqlx/macros`/`migrate` so `sqlx-mysql`→`rsa` RUSTSEC-2023-0071 never enters the tree).
  Migration 0001 (named UNIQUE constraints so the typed error names the rule), a versioned runner
  copied from `PgDigests` (ledger `_repo_graph_migrations`, distinct advisory lock `"agrepogr"`),
  `with_tenant` fail-closed on `safe_segment` before any statement, a one-transaction `UNNEST` bulk
  write with the node-id collision check, every read verb as the `03-queries.md` recursive CTEs
  (clamped in Rust, scope preflight ⇒ `NotFound` cross-tenant/repo), diff / retention (explicit
  orphan-body sweep for the non-CASCADE version→node FK) / shape. Store rows are untrusted: a corrupt
  row is a `Backend` fault, never a panic; `map_db` keys on constraint name + `ErrorKind`, never the
  message or DSN. Tiers **P1** (in-gate units via `nix/checks/repo-graph.nix`), **R3-pg** (the 46
  conformance rows rerun unchanged with `assert_invariants` after each), **R4** (11 pg-only rows) —
  tables in [`08-test-matrix.md`](08-test-matrix.md). Gate green on `0d61ff52`; `nix run
  .#pg-integration` PASS, repo-graph leg 57/57 (46 R3 + 11 R4) ([`PROGRESS.md`](PROGRESS.md)).
  Unblocks lane C: RK-06 (indexer) once RK-03 (extractors) also lands.
