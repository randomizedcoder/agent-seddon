# Repository knowledge — implementation plan

Execution plan for the [design of record](README.md), including the decisions/lessons extension
merged in PR #636. Original baseline: `ed0ab95d` (2026-10-17). RK-00–02 are merged;
RK-03 now has local implementation on `d648c894`, with acceptance in STATUS/PROGRESS.
RK-04–21 remain planned, including their proposed checks and commands.

[Status tracker](STATUS.md) · [Increment map](06-increments.md) · [Test matrix](08-test-matrix.md) ·
[Progress journal](PROGRESS.md) · [Integration and evaluation runbook](10-integration-and-evaluation.md)

## How to use this plan

Take the next increment whose prerequisites are merged, implement its steps, collect its
acceptance evidence, and update its row in STATUS in the same PR. One PR per increment remains
the default. Record deviations and measured results in PROGRESS; retain the existing RK IDs.
STATUS is the single implementation tracker. This plan owns execution steps; numbered design
chapters own API/schema contracts, the test matrix owns case descriptions/expected outcomes,
and the integration runbook owns commands and measurement thresholds.

Do not infer that a proposed interface, test target, Nix check or migration exists from its
appearance here. For completed work, use the as-built entries and linked historical evidence.

## Delivery sequence

| Milestone | Delivery order | Acceptance outcome |
|---|---|---|
| Foundation | RK-00 → RK-01 → RK-02 (merged) | Validated core graph and tenant-bound memory/Postgres storage |
| Structural review context | RK-03 → RK-06 → RK-07 and RK-08 → RK-09 | Immutable-SHA indexing, monitored-repo refresh, bounded queries and cited review context |
| Inventory and session context | RK-10 → RK-11 → RK-12; RK-15 after RK-10 | Inventory, cited summaries, Implement/Design briefs and deterministic profile |
| Confirmed decisions and lessons | RK-19 → RK-20 → RK-21, with prerequisites below | Human-confirmed, current knowledge in briefs and paired evaluation |
| Additional analysis | RK-04/RK-05 after RK-03; RK-16 → RK-17/RK-18 with prerequisites below | Go/SCIP support, history, derived analyses and layered summaries |
| Optional infrastructure | RK-13 after RK-08; RK-14 in the multi-tenancy track | Remote service and cross-tier RLS, separately accepted |

Recommended serial order after the foundation: **03, 06, 07, 08, 09, 10, 15, 11, 12, 19, 20, 21**.
RK-07 and RK-08 can be developed independently after RK-06; RK-11 and RK-15 can be developed
independently after RK-10. Go and SCIP do not block that path. Additional analyses and optional
infrastructure remain tracked work, not prerequisites for confirmed lessons.

Migration delivery is strictly **0001/RK-02 → 0002/RK-10 → 0003/RK-15 → 0004/RK-19**.
RK-15 therefore depends on RK-10 as well as RK-03; do not ship migration 0003 ahead of 0002.
RK-19 needs RK-15's schema/profile, not RK-16's history extraction. RK-17's orphan analysis
requires RK-05; do not infer absence of callers from an unresolved Rust graph.

## Shared implementation and acceptance rules

- Keep the pure graph model and interfaces in `agent-core`, doubles/conformance in
  `agent-testkit`, and Postgres/extractors in `agent-repo-graph`. Runtime composes these with
  review, tools and CLI. Use tenant-bound handles and `Scope { repo, snapshot }` consistently.
- Follow [Nix and feature wiring](10-integration-and-evaluation.md#build-and-feature-wiring):
  `flake.nix` stays a thin orchestrator; register checks through `nix/checks/default.nix` and
  apps through `nix/default.nix`. Reuse pinned tools and Crane artifacts. No default features
  change. Proposed `extract-rust`, `repo-graph`, `tool-repo-graph`, `repo-knowledge` and optional
  `extract-scip` must be explicitly enabled in their owning checks and forwarded by consumers.
- Use the fixture workspace at `crates/agent-repo-graph/tests/fixtures/ws`; confirm source,
  Markdown, JSON and SQL fixtures survive Nix filtering. Use local Git A/B/C plus a bare origin,
  fixed timestamps, fake forge and scripted providers for composition tests.
- Tests use table-driven `rstest` with positive, negative, boundary and corner cases, plus
  adversarial cases for untrusted inputs. The canonical [test matrix](08-test-matrix.md)
  includes descriptions and expected outcomes. Preserve case IDs; extend it in the owning
  implementation PR where an older increment currently has scenario-only coverage.
- Gate each implementation with `nix flake check` plus its feature-scoped checks and required
  integration suites. Run Cargo through `nix develop`. Record exact revision, features,
  commands, execution counts, results and artifact locations; skips/unavailable are not passes.
- “Done” means deliverables and case coverage exist, required acceptance evidence is recorded,
  docs describe the as-built behavior, and the PR is merged. A compiled feature with outstanding
  validation remains “implemented awaiting validation”; merging alone does not prove acceptance.

## Foundation already delivered

### RK-00

**Design track.** No prerequisite. Merged in #495: design, gap analysis and index links.
Retain the original decisions and history. Acceptance is the existing linked design track;
this implementation-plan update is documentation maintenance, not another completed increment.

### RK-01

**Core graph and memory store.** Depends on RK-00. Merged in #582: graph interface, keys,
model, builder, extraction contract and memory store. Preserve the as-built placement in
`agent_core::repo_graph` and `agent-testkit`. Historical acceptance: R1/R2/R3 and memory-only
cases; gate revision `a023635a`. New store methods must retain existing conformance behavior.

### RK-02

**Postgres graph store.** Depends on RK-01. Merged in #585: migration 0001, tenant-bound
store, bulk write, reads, diff and retention behind `repo-graph-postgres`. Historical acceptance:
P1 plus shared R3 and Postgres R4; gate revision `0d61ff52`, live graph suite 57/57.
These results are historical, not a fresh validation of this documentation change.

## Structural review context

### RK-03

**Rust, Cargo and docs extraction.** Depends on RK-01.

Local implementation is present on `d648c894`; see [status](STATUS.md) and the
[as-built decisions/evidence](PROGRESS.md#2026-10-18--rk-03-extraction). Acceptance is not a merge.

1. Implement the three extractors in `agent-repo-graph` behind `extract-rust`, using the existing
   core contract, builder and [extraction rules](02-extraction.md). Parse repository inputs
   without executing build scripts, macros, repo code or dependency downloads.
2. Add the two-crate fixture with traits/impls, features, cfg collisions, path modules, docs,
   test classes and malformed/hostile inputs. Preserve deterministic keys/order/hash and
   explicit parse-error, truncation and unsupported-coverage reporting.
3. Enforce confinement and file/count/byte/deadline budgets. A timed-out parser retains its
   bounded worker slot until completion; discard late output. Keep heuristic test links labelled.
4. Add `repo-graph-rust` through its new Nix check, extend iai bench/Ir ceiling and dhat leak
   coverage, and explicitly enable extraction features in those checks.

**Accept:** E1, including late-worker behavior, passes; extracting twice produces the pinned
hash; no outside-root reads or code execution; bench/leak gates pass. Record actual performance
measurements without treating design estimates as measured results. Unlocks RK-06, RK-04/05.

### RK-06

**Indexer, atomic publication and CLI.** Depends on RK-02 and RK-03.

1. Extend the core interface, memory and Pg stores with `snapshot_publish`, `snapshot_report`,
   operator-only `snapshot_recover` and scoped `read_view` per [consumption](05-consumption.md).
   Preserve begin/write/finish APIs. Publish under one transaction and identity advisory lock;
   duplicate publishers reuse the ready identity. Retention cannot interrupt a consistent read.
2. Add versioned report metadata and legacy-array decoding. Legacy completeness is unknown;
   deploy dual readers before envelope writers. Include normalized output-affecting config and
   budget hash in identity. Recover legacy committed building rows only after explicit operator
   intervention; never take over by age. Record recovery through the existing design contract.
3. Build the indexer over immutable SHA checkouts, validation, publication and retention. Add
   `[repo_graph]`, registry/tenant resolution and CLI add/index/status/diff/shape, including
   operator recovery access. Report index, truncation and cleanup failures distinctly.
4. Forward the proposed runtime/CLI `repo-graph` feature; add `repo-graph-index`, extend P1,
   config/CLI/feature checks and the metrics specified in the increment map.
5. Extend `pg-integration` with strict required mode and suite result artifacts before claiming
   live acceptance. Add the indexer portion of I2; fleet/review assertions arrive in RK-07/09.

**Accept:** I1 including metadata compatibility, existing store conformance, and applicable I2
pass in memory/Pg: identity reuse, races, rollback on disconnect, retention, dirty-root refusal
and report parity. Strict live mode fails on missing runtime/DSN/suite or zero executed tests.

### RK-07

**Monitored-repository indexing.** Depends on RK-06.

1. Wire the fleet factory to tenant-bound graph/indexer views and cache by `(tenant, row.id)`.
   Resolve tenant/forge/repo registration; a missing registration skips graph work while the
   existing review continues.
2. Schedule default-head indexing at enrollment/poll changes and PR merge-base indexing off
   the review path. Pin SHAs; deduplicate tenant/repo/SHA/extractor identity; use 16 waiting jobs
   and one active extraction per indexer, retrying full-queue skips on the next poll.
3. Clean up checkouts/jobs on deletion/cancellation/shutdown and prevent late publication.
   Extend `repo-graph-index` and live I2 through fake forge polling over real local Git.

**Accept:** F1 and applicable I2 pass, including identical row IDs across tenants, bounded queue,
repeated triggers and cancellation. Reviews do not wait for indexing or share foreign context.

### RK-08

**Graph tool and PgAst.** Depends on RK-06.

1. Implement the supported [query surface](03-queries.md), `PgAst` under the existing AST
   interface, and the `repo_graph` tool. Wire tools/runtime/CLI `tool-repo-graph` forwarding,
   registry factories and configuration; add the planned component documentation.
2. Apply existing query caps, snapshot identity checks and consistent read views. Return
   successful empty, unavailable, error, incomplete and unsupported outcomes distinctly.
   Bound the serialized tool envelope to 16 KiB and drop whole cited entries when truncating.
3. Add R5 in `review-repo-knowledge`, with CLI/config/feature coverage. Questions requiring
   later inventory/profile/history/SCIP work remain explicitly unavailable until implemented.

**Accept:** R5 and PgAst composition pass for cycles, hostile arguments, tenant isolation,
source citations and disabled registration. Existing AST backends continue to work.

### RK-09

**Review graph context and initial evaluation.** Depends on RK-07 and RK-08.

1. Add the collector, fact fragment, `ReviewFacts` field, renderer and default-off review
   configuration. Use the pinned PR merge-base snapshot; allow a labelled first-parent ancestor
   at distance at most 50, never an unrelated latest snapshot.
2. Keep graph context within 4 KiB with citations, completeness and supported-coverage labels;
   preserve the ordinary review on missing graph or store failure. Add status/telemetry without
   source text, keys or PR URLs in metric labels.
3. Complete C1 and end-to-end I2 in hermetic and strict Pg suites. Add the opt-in
   `repo-knowledge-eval` app and baseline/graph evaluation per the runbook (Q1).

**Accept:** C1/I2 pass, including disabled equivalence and outage fallback. Record paired A/B
results, cold indexing separately from warm review costs, and limitations. An unavailable
quality run is pending evaluation, not a fabricated pass or permission to enable by default.

## Inventory and session context

### RK-10

**Inventory skeleton.** Depends on RK-06.

1. Add migration 0002, Inventory interface methods, memory/Pg parity and deterministic
   [feature/evidence derivation](04-inventory.md), including stable subgraph hashes.
2. Add `agent repo inventory` and feature-scoped fixture/store coverage. Defer profile-derived
   categories until RK-15 produces their evidence; do not fabricate them.

**Accept:** seam/tool/cargo-feature fixtures have expected evidence roles; empty, missing,
foreign-scope, cap and repeat-input cases pass. Test fresh/upgrade/idempotent migration and
inventory reads in strict Pg; record cases in the matrix and checks with nonzero counts.

### RK-11

**Cited summaries and embeddings.** Depends on RK-10.

1. Extend inventory storage/queries and runtime composition with bounded evidence slices,
   cheap-provider summaries, citation validation/retry, injection screening and embeddings.
   Add `Similar`/`Feature` questions and `agent repo summarize [--dirty-only]`.
2. Skip unchanged subgraph/content hashes; retain previous summaries after rejected output.
   Validate embedding dimensions/model identity. Keep generated explanations distinct from
   confirmed knowledge. Extend scripted-provider, store and query checks.

**Accept:** unknown/missing citations, injection, oversize output, dimension mismatch, retry
exhaustion and unchanged-input no-call cases pass; stable fixture rankings and tenant isolation
are asserted. Add table cases and strict Pg summary/embedding round trips.

### RK-12

**Implement/Design context.** Depends on RK-11.

1. Add the session brief hook and `agent repo brief --goal`, selecting bounded cited inventory
   and graph context per [consumption](05-consumption.md). Inject once at the specified session
   stage within the 6 KiB total budget.
2. Extend CLI/config and scripted session checks for available, disabled, missing, oversized,
   stale and malicious context; reserve the later knowledge sub-budget inside the same total.

**Accept:** the correct snapshot's citations reach the session exactly once; disabled/unavailable
paths preserve existing behavior; boundary truncation keeps complete entries and valid UTF-8.
Record table cases and real-store/runtime composition evidence.

### RK-15

**Deterministic profile.** Depends on RK-03 and RK-10 (migration ordering).

1. Add migration 0003 (`repo_facts`, `repo_history`) and Facts interface/storage in core,
   memory and Pg. Implement the RK-15 rows in [repository profile](07-repo-profile.md), including
   config, entry points, schemas, factories, API/debt/metrics/gates and doubles.
2. Extend extraction fixtures and `repo-graph-rust`; connect profile-backed inventory and
   bounded query/brief categories where consumers exist. Keep heuristic provenance explicit.

**Accept:** deterministic fixture facts and evidence match, hostile text/paths are screened,
caps and missing inputs are covered. Strict Pg applies 0001→0002→0003 and reruns idempotently;
Facts store conformance passes. No history extraction is claimed until RK-16.

## Confirmed decisions and lessons

### RK-19

**Knowledge store and lifecycle.** Depends on RK-10 and RK-15.

1. Add `RepoKnowledgeStore` and types to core, memory/conformance to testkit, Pg implementation
   and migration 0004 to repo-graph. Follow [decisions and lessons](09-decisions-and-lessons.md)
   for immutable content/evidence, links, append-only events, deduplication and lifecycle.
2. Enforce tenant/repo constraints, typed limits, CAS revisions, atomic supersession and
   retained evidence history. Freshness depends on relevant source hashes, not commit age.
3. Forward proposed `repo-knowledge` features, add the `repo-knowledge` Nix check, shared K1
   conformance, applicable K2 and live K5 migration/race coverage.

**Accept:** memory/Pg agree; duplicates do not resurrect rejected records; invalid/foreign
transitions fail; only one concurrent transition wins. Fresh and 0001 databases upgrade through
0004 idempotently. Snapshot retention preserves inspectable knowledge history.

### RK-20

**Operator workflow and optional candidate generation.** Depends on RK-19, RK-09 and RK-11.

1. Add CLI propose/list/show/confirm/reject/supersede and evidence resolution, with trusted
   operator identity, revision checks and strict proposal decoding. Reject model-provided
   tenant/actor/state overrides. Require valid supporting evidence for confirmation.
2. Add the optional review-to-candidate adapter using existing providers/sanitization. Posting,
   merging and `addressed` feedback never confirm knowledge. Models can propose only.
3. Wire default-off knowledge configuration and CLI/config/feature checks; implement K2/K3
   and the live K5 lifecycle path, including unavailable feedback and changed source evidence.

**Accept:** explicit operator actions produce the expected state/events; missing evidence blocks
confirmation; malicious proposals fail closed; proposal failure does not break reviews.
Strict Pg demonstrates persistence and CAS behavior across handle/runtime restarts.

### RK-21

**Confirmed knowledge in briefs and evaluation.** Depends on RK-20 and RK-12.

1. Select only confirmed/current records: direct links then two-hop relevance, stable-ID ties,
   at most five items and 2 KiB within the existing 4/6 KiB brief totals. Preserve historical
   inspection while excluding stale/unverifiable advice from prompts.
2. Implement K4 composition and remaining K5 runtime/restart/retention scenarios; extend
   `repo-knowledge` and strict Pg acceptance.
3. Extend Q1 evaluation to baseline/graph/lessons with a frozen reviewed knowledge corpus,
   held-out PRs, equal total budgets, rotated arm order and three repetitions.

**Accept:** prompt evidence matches target code/doc hashes and tenant, truncation preserves
citations, and superseded/rejected/stale records are absent. Publish all three arms' quality,
tokens, latency, uncertainty and failures. Apply runbook thresholds before proposing a separate
default-config change; failed thresholds leave the feature opt-in.

## Additional analysis

### RK-04

**Go extraction.** Depends on RK-03. Map bounded helper JSON and `_test.go` scans into the
shared builder; reuse the existing AST Go fixture and sandboxed helper execution. Add
`repo-graph-go` and table cases for valid mapping, malformed output, empty tests, caps and hostile
paths. **Accept:** deterministic keys/edges and provenance, safe helper failure, and feature-scoped
gate pass. Changing the helper's `Tests` setting remains a separate helper change.

### RK-05

**SCIP extraction.** Depends on RK-03. First record spike time/RSS/index size, enclosing-range
behavior and join rate. Implement optional `extract-scip`, syn joins and resolved
calls/references/tests only if the [extraction spike criteria](02-extraction.md) hold. Use the
existing sandbox/tooling; store derived edges rather than index blobs. Add cases for joins,
unknown descriptors, missing ranges, oversized/malformed input and sandbox failure.
**Accept:** spike evidence and correct resolved edges; `repo-graph-scip` when offline-capable,
otherwise an explicitly executed integration suite. A rejected spike is recorded as deferred,
with its reason, rather than marking an unshipped extractor complete.

### RK-16

**History and recipes.** Depends on RK-06 and RK-15. Give the indexer bounded `RepoBackend`
log access; reuse churn/co-change math for history, in-flight facts and deterministic recipes.
Extend storage, queries and supported consumers using fixed synthetic Git history. Count author
identities without storing names; screen subjects and cap recipe evidence.
**Accept:** expected churn/co-change/recipe rows for fixed commits; empty/shallow histories,
limits, hostile subjects and repeat determinism have table coverage. Strict Pg round trips and
runtime composition pass; history collection does not alter selected snapshot identity.

### RK-17

**Derived analyses.** Depends on RK-09 and RK-16; complete orphan analysis also requires RK-05.
Implement the profile chapter's chokepoints, guard funnels/gaps, near-duplicates and orphans;
add stored facts, tool questions and bounded review lines. Extend `review-repo-knowledge` with
known positive/negative graphs, cycles, cap boundaries and adversarial inputs.
**Accept:** expected analyses with citations and heuristic/coverage labels; unresolved graphs
never imply verified absence of guards/callers. Record partial delivery until SCIP-dependent
orphan coverage is implemented and validated.

### RK-18

**Generated explanations and layered briefs.** Depends on RK-11, RK-16 and RK-12 (both brief
consumers). Extend cited summaries with conventions, glossary, playbooks, pitfalls and L0 overview;
wire L0/L1/L2 layers into review and Implement/Design briefs. Keep these labelled model-written,
separate from confirmed lessons. Extend provider/renderer checks and summary persistence tests.
**Accept:** citation/injection/size/retry cases pass; out-of-recipe file paths are rejected;
unchanged evidence avoids model calls and both briefs respect existing budgets.

## Optional infrastructure

### RK-13

**Remote graph service.** Depends on RK-08. Add scoped `RepoGraphService`, server/client wiring,
`--serve-repo-graph`, mt-audit classification/manifest, generated constants and component docs.
**Accept:** buf, constants-sync and mt-audit plus scoped RPC conformance, malformed-input and
cross-tenant cases. Wire changes follow existing governance; this service is not required for
local Postgres graph or knowledge consumers.

### RK-14

**Cross-tier Postgres RLS.** External dependency and acceptance owner:
[multi-tenancy plane 02](../multi-tenancy/README.md). Coordinate roles, tenant context and pool
reuse across Pg tiers there; link its implementation PR and evidence back into STATUS.
**Accept:** that track's RLS tests cover graph and, when present, knowledge tables, including
wrong/missing tenant, pooled-connection reuse and privileged migration behavior. Keep the row
not started until external evidence establishes implementation; no RLS claim follows from
application-side tenant checks.

## Acceptance tiers and evidence

| Tier | Required evidence | Owning increments |
|---|---|---|
| Hermetic | Named table cases with descriptions/expected outcomes; feature-scoped checks, deterministic hashes, bench/leak where applicable; local Git + scripted providers for composition | Every implementing increment; E1/I1/F1/R5/C1/K1–K4 and later scenario tables |
| Live Postgres | Strict required-mode suite JSON, nonzero executed counts, sanitized logs, revision/features/pins; migrations, publication races, disconnect/restart, retention, tenant isolation and rendered context | RK-06 onward for changed Pg paths; I2 and K5 expand as their consumers land |
| Paired quality | Frozen corpus/config, raw paired artifacts, adjudication, limitations; cold indexing separated from warm p50/p95 review, tokens/calls and evidence correctness | RK-09 A/B; RK-21 A/B/C; Q1 |

Use the [integration runbook](10-integration-and-evaluation.md) for exact current and future
commands, setup, cleanup, suite inventories and failure codes. Current optional Pg skip behavior
must not be recorded as acceptance. Before RK-06's strict runner exists, inspect nonzero executed
counts explicitly. Future commands are not executable acceptance evidence until their owner lands.

For considering a default change, the runbook requires deterministic/strict Pg success, no
cross-tenant or invalid citations, no false-positive increase, at least one new valid finding or
at least 10% lower median tokens without losing valid findings, and warm p95 latency no more
than 10% worse. Report measured failures honestly; implementation completion and quality approval
are separate tracker entries. No implementation PR silently enables the feature by default.

For this documentation-only update, run doc-links, doc-orphans, Markdown table/anchor validation
and `git diff --check`; verify RK-00–21 each has one plan section and one tracker row. Do not
run or report future implementation suites as though they exist.
