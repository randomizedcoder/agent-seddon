# Repo knowledge: a persisted, deterministic code graph + a cited inventory (design of record)

> **Status:** implementation under way, opened 2026-09-26 from the
> [self-improvement gap analysis](../../gap-analysis/self-improvement.md). The track is landing
> increment by increment: [`STATUS.md`](STATUS.md) is the tracker, [`06-increments.md`](06-increments.md)
> the increment map, [`IMPLEMENTATION.md`](IMPLEMENTATION.md) the execution plan,
> [`PROGRESS.md`](PROGRESS.md) the working journal (why the as-built code
> differs from these docs), and [`08-test-matrix.md`](08-test-matrix.md) the test tables. Every
> design claim below carries a `path:line` against `main` `5ddcda7` so it can be re-verified.

## Why this exists

agent-seddon reviews PRs and implements features against repositories it has never seen whole.
Today its knowledge of a repo is rebuilt from scratch on every review, lives in one process, and
for Rust has no parser-derived structure at all. The self-improvement gap analysis found ten gaps:

| Gap | What is missing | Evidence |
|---|---|---|
| SI-1 | A persisted graph with stable node keys | Go graph is in-memory, dense unstable ids: `crates/agent-ast/src/go.rs:30`, `crates/agent-ast/src/graph.rs:34-52` |
| SI-2 | Rust structure: SCIP ingestion keeps definitions and drops references | `crates/agent-ast/src/model.rs:223-273` |
| SI-3 | A `repos` table and a `(tenant, repo, commit)` identity | config store is a blob store: `crates/agent-config-store/migrations/0001_config_store.sql:10-14` |
| SI-4 | Bounded structural questions the model can ask | `AstBackend` verbs are symbol-centric: `crates/agent-core/src/lib.rs:4909` |
| SI-5 | Precomputed review facts; a real duplicate signal | `nearby` is a literal search: `crates/agent-review/src/nearby.rs:1-10` |
| SI-6 | A cited inventory of features, seams, tools, tests | nothing per repo exists |
| SI-7 | Repo context for Implement / Design mode; gap intake | only Review injects: `crates/agent-runtime/src/agent/session.rs:339-352` |
| SI-8 | Fleet indexing ahead of the PR; tenant in the cache key | cache by `row.id`: `crates/agent-runtime/src/fleet_review.rs:179-180` |
| SI-9 | A repo profile (entry points, schemas, config keys, factory map, debt, metrics) | none |
| SI-10 | History-derived knowledge (hot spots, in-flight work, change recipes) | `churn.rs` / `cochange.rs` compute per PR and discard |

## The shape in one picture

```
 checkout @ sha ──► extractors: rust-syn + cargo · docs · go-graph · scip · profile · history
                        │   [deterministic, versioned, budgeted, validated, sorted, hashed]
                        ▼
                 Postgres  repos / graph_snapshots / graph_nodes / graph_node_versions /
                           graph_edges / repo_facts / repo_history
                        │                                   │
        inventory skeleton (deterministic)      PgAst : AstBackend · repo_graph tool · SQL CTEs
                        │
        LLM summaries (cited, sanitized) + embeddings ──► repo_features / repo_summaries /
                        │                                  repo_embeddings
                        ▼
   fleet: RepoKnowledgeCollector → review brief   ·   loop: Implement/Design brief   ·
   `agent repo brief --goal`
```

The structural graph is never written by a model. In RK-11/18 the model writes only
`repo_summaries`, with citations validated against the selected snapshot. The proposed RK-19–21
extension adds separate decision/lesson candidates; only explicit human confirmation makes them
eligible for guidance. These candidates never modify structural graph edges.

## Decisions

**D1 — Node identity is a repo-scoped natural key.** Each node has a `node_key TEXT` built from
its language, kind and qualified path (`rust:fn:agent_core::security::confine`), and a
`node_id BIGINT` equal to the first 8 bytes of `sha256(node_key)`. Node bodies are inserted once
per `(tenant, repo)` and shared across snapshots; the facts that change per commit (file, lines,
signature hash, body hash, attrs) live in `graph_node_versions`; edges are snapshot-scoped over
the stable ids. A hash collision on write fails the snapshot rather than merging two symbols.
Grammar and DDL in [`01-schema.md`](01-schema.md).

**D2 — Incrementality v1 is "full re-extract, dedup on write".** `syn` over this workspace takes
seconds; SCIP has no incremental mode anyway. The snapshot diff is computed after the write from
`graph_node_versions`, not maintained during extraction.

**D3 — Rust extraction is `syn` + `Cargo.toml` first, SCIP second.** The in-process extractor
gives items, impls, tests with their case class, modules, cfg gating and crate dependencies with
no toolchain in the sandbox. `rust-analyzer scip` is added later (behind a feature, after a
measured spike) to contribute resolved `calls` and `references`, joined to syn nodes by item line
ranges on the source side and by a normalised path on the target side.
Details in [`02-extraction.md`](02-extraction.md).

**D4 — Tenancy is app-side, RLS-ready, RLS deferred.** Every table leads with `tenant` in its
primary key and carries a foreign key to `tenants`; every read helper takes a
`Scope { repo, snapshot }` on a tenant-bound handle (as built in RK-01/02); the Postgres impl
exposes `with_tenant` and the planned runtime integration wraps it in `PerTenant` exactly like the other Pg tiers (`crates/agent-runtime/src/tenant.rs:71`).
Row-level security is a cross-tier increment filed under multi-tenancy, not here.

**D5 — No LLM in the structural graph.** Extractors are parsers and git. Inventory models write
`repo_summaries`; each summary's `citations[]` is validated against the snapshot fail-closed and
the text passes the digest tier's `sanitize` + `scan_for_injection` template
(`crates/agent-digest/src/lib.rs:51`, `crates/agent-core/src/security.rs:97`).
Details in [`04-inventory.md`](04-inventory.md).

**D6 — Embeddings are `REAL[]` with a `dim` column; cosine is computed in Rust after a bounded
fetch.** No pgvector; the `Embedder` seam and the existing brute-force cosine are reused
(`crates/agent-core/src/lib.rs:1526`, `crates/agent-search/src/vector.rs:2`).

**D7 — Consumption is push-first.** Graph slices go into the review brief, which is the
fleet-grounding rule (`docs/design/fleet-grounding/README.md:95-98`); the fleet reviewer's tool set
stays as it is (`crates/agent-runtime/src/agent.rs:3153`). The `repo_graph` tool serves the
interactive loop. Details in [`05-consumption.md`](05-consumption.md) and
[`03-queries.md`](03-queries.md).

**D8 — No `repo_gaps` table.** A gap is a goal. `agent repo brief --goal "<text>"` assembles the
context; a later `agent gap plan <id>` is a thin alias that reads the gap-analysis section as the
goal. A tracker would be a second source of truth beside `STATUS.md`.

**D9 — The repo profile is the same pipeline.** Entry points, protobuf and SQL schemas, config
keys, the factory map, error types, API surface, complexity, debt, metrics, gates, test doubles,
churn, co-change, in-flight work and change recipes are all extra deterministic extractors
writing node kinds, attrs, or rows in `repo_facts` / `repo_history`. Never LLM-derived.
Details in [`07-repo-profile.md`](07-repo-profile.md).

**D10 — Delivery is layered.** L0 is a "do not" list plus an overview of at most 2 KB. L1 is
per-crate and per-feature cards. L2 is on-demand slices through the tool. The review brief and the
Implement-mode brief are L0 + L1 selections under a byte budget; the tool serves L2. Every item
carries a citeable key.

## Recommendation summary

| Gap | Decision | Doc | Increment |
|---|---|---|---|
| SI-1 | Natural-key nodes, shared bodies, snapshot-scoped edges, Pg store | 01 | RK-01, RK-02, RK-06 |
| SI-2 | `syn` + Cargo extractor; SCIP references after a spike | 02 | RK-03, RK-05 |
| SI-3 | `repos` + `graph_snapshots`, tenant-led PKs, `agent repo add` required | 01, 05 | RK-02, RK-06 |
| SI-4 | `RepoGraphStore` seam, `PgAst`, one `repo_graph` tool with a `question` enum | 03 | RK-08 |
| SI-5 | `RepoKnowledgeCollector` (off by default, measure-gated); winnowed near-duplicates | 05, 07 | RK-09, RK-17 |
| SI-6 | Deterministic skeleton + cited, sanitized summaries + embeddings | 04 | RK-10, RK-11 |
| SI-7 | Implement / Design brief hook + `agent repo brief --goal` | 05 | RK-12 |
| SI-8 | Merge-base index off the critical path; `(tenant, row.id)` cache key | 05 | RK-07 |
| SI-9 | Profile extractors + `repo_facts` | 07 | RK-15 |
| SI-10 | History extractors + `repo_history` + `co_changes_with`; LLM extras | 07, 04 | RK-16, RK-18 |

## Threat model

Repository content is attacker-controlled. So are model-supplied tool arguments. The store spans
tenants. Each item names its mitigation.

| Threat | Mitigation |
|---|---|
| Hostile paths in a checkout (symlinks out of the root, `..`, huge names) | Every walked path passes `confine` (`crates/agent-core/src/security.rs:186`); `follow_links(false)`; key and name length caps |
| Hostile identifiers and doc comments become prompt text | `attrs.doc` is capped at 512 B and dropped when `scan_for_injection` fires; keys are ASCII-validated; nothing from a comment is rendered raw |
| Pathological files stall `syn` | Per-file byte cap (2 MiB), cooperative deadline, bounded worker admission and discarded late output, node / edge caps, `truncated` flag |
| Recursive CTEs blow up | `SET LOCAL statement_timeout = '3s'`, hop caps (≤ 4 neighbours, ≤ 6 path), row `LIMIT`s, `references` excluded from path queries |
| Cross-tenant reads | Tenant-bound handle plus `Scope { repo, snapshot }` on every read helper; tenant-led PKs; adversarial tests that assert a wrong tenant sees nothing |
| Model-chosen slugs, keys, hops, limits | `safe_segment` on slugs and keys (`crates/agent-core/src/identity.rs:26`); hops and limits clamped; unknown keys are an empty result, not an error that echoes the key |
| Summaries that cite nodes which do not exist | Citations validated against the snapshot; one retry listing the unknown keys; then the previous summary is kept and the failure recorded |
| Row growth | Retention of N ready snapshots per repo (default 10); a body sweep deletes nodes unreferenced by any retained snapshot |
| The Go helper or `rust-analyzer` misbehaves | Both run through the Sandbox with the existing timeouts; output is parsed with the same caps as `Graph::parse` (`crates/agent-ast/src/graph.rs:75-123`) |

## Non-goals

- Commit, PR and author **nodes**. ClickHouse already has the event stream; history lands as
  per-file rows and `co_changes_with` edges, not as a second commit store.
- tree-sitter or any language beyond Rust and Go in v1. The extractor trait is language-neutral;
  more languages are more extractors.
- Postgres row-level security (cross-tier, multi-tenancy plane 02).
- A portal UI over the graph.
- An editable gap tracker (D8).

## Relationship to other tracks

- [`code-graph/`](../code-graph/README.md): keeps the `AstBackend` seam and the `go` / `scip`
  engines; this track adds `PgAst` behind the same seam.
- [`fleet-grounding/`](../fleet-grounding/README.md): Increment 3 (per-repo index keyed by
  `row.id` + `head_sha`) is subsumed by RK-07.
- [`config/`](../config/README.md) PG-01..PG-11: the migration runner, DSN resolution and live-test
  pattern are copied, not re-invented.
- [`security-hardening/`](../security-hardening/README.md): S2 supplies the tenant from the
  principal; this track consumes it and adds a `scoped` service in RK-13.
- [`review-analysis-depth/`](../review-analysis-depth/README.md): the collector added in RK-09 is
  one more `FactCollector`.
- [`multi-tenancy/`](../multi-tenancy/README.md): RLS across Pg tiers (RK-14) is filed there.
- [`campaigns/`](../campaigns/README.md): the work tracker this track's D8 declined to be. It
  consumes `repos` (RK-02, its hard prerequisite), the brief (RK-12) and `node_key`s (RK-08) to
  decompose a gap into a tree of tasks and PRs.

## Build order

Use the [implementation plan](IMPLEMENTATION.md#delivery-sequence) for delivery order and
acceptance, the [increment map](06-increments.md) for lane ownership, and [STATUS](STATUS.md)
for progress. One PR per increment remains the default; `nix flake check` gates implementation.

| Delivery path | Prerequisites and order |
|---|---|
| Structural context | RK-03 → RK-06 → RK-07/RK-08 → RK-09; RK-02 already merged |
| Inventory and profile | RK-10 after RK-06; RK-11 → RK-12 after RK-10; RK-15 after RK-03 and RK-10 |
| Confirmed knowledge | RK-19 after RK-10/RK-15; RK-20 after RK-19/RK-09/RK-11; RK-21 after RK-20/RK-12 |
| Additional analyses | RK-04/RK-05 after RK-03; RK-16 after RK-06/RK-15; RK-17 after RK-09/RK-16 (RK-05 for orphans); RK-18 after RK-11/RK-16/RK-12 |
| Optional infrastructure | RK-13 after RK-08; RK-14 owned by multi-tenancy plane 02 |

First value is RK-03 + RK-06 (`agent repo index / status / diff`). First model-visible payoff
is RK-08/RK-09. Go/SCIP do not block those consumers. Migration order is 0002/RK-10 before
0003/RK-15 before 0004/RK-19; sharing an extractor lane does not override that dependency.

## Risks

| Risk | Mitigation |
|---|---|
| SCIP cost (minutes, gigabytes) and a sandbox that cannot resolve the cargo workspace offline | Spike first (RK-05 as-built); SCIP optional per repo; vendored or network-allowed sandbox flag |
| SCIP descriptor and `enclosing_range` assumptions do not hold | Join by syn line ranges, not by SCIP's own ranges; the spike measures the join rate |
| cfg-split duplicate keys (669 `#[cfg(feature)]` attrs in this workspace) make churny diffs | `@<sha8(cfg tokens)>` suffix only on collision; `attrs.dup` records the rest |
| Name-heuristic `tests` edges are noisy until SCIP lands | `attrs.via = "name"` is rendered as such; SCIP replaces them with `via = "scip"` |
| Hostile repo content (see threat model) | caps, confinement, injection screen, never render raw |
| App-side tenancy only | `Scope` on every read; adversarial cross-tenant tests in every store increment |
| Determinism drift across toolchain pins | `extractor_version` is part of snapshot identity; the hermetic check indexes a fixture twice and compares `graph_hash` |
| Inventory token drift | `subgraph_hash` gating; `agent repo summarize --dirty-only` |
| Slug mismatch between the loop and the fleet | `[repo_graph].repo` is explicit; `repo add` is required before `index`; the fleet passes `row.repo` |
| Comment and TODO scans are attacker text | bounded to 160 B, injection-screened, rendered as counts plus `file:line`, never as text in the brief |
| Shingle index size for near-duplicates | ~9 k functions × a few winnowed shingles each; computed at index time, stored as edges only |
| Recipe classifier drifts from a repo's conventions | Rule-based and per-repo configurable (`repos.profile.recipe_rules`), never learned |

## Extension: monitored repositories and durable lessons (2026-10-17)

**Design only.** RK-01/02 are implemented; the indexer, extraction and review consumers remain
pending. The review fleet already polls PRs, persists drafts and tracks feedback; enabling that
fleet does not yet populate this graph. Complete RK-03 → RK-06 → RK-07/08 → RK-09 first.
The end-to-end publication and refresh contract is in [05 — Consumption](05-consumption.md).

Then add human-confirmed decisions and lessons as RK-19–21, alongside the existing inventory:
[09 — Decisions and lessons](09-decisions-and-lessons.md). Source-backed explanations complement
parser-derived structure and conversational memory. Review `addressed` state and PR merge status
are not confirmation. Candidate generation is optional; only confirmed, current evidence can
enter a brief. No automatic graph writes by a model and no new database dependency.

[10 — Integration and evaluation](10-integration-and-evaluation.md) owns Nix wiring, executable
runbooks, required-versus-skipped Postgres acceptance, and paired review measurement. The
[test matrix](08-test-matrix.md) maps the extensions to named case classes and increments.
Existing non-goals remain: no commit/PR/author graph nodes or portal graph UI. Evidence references
may identify a PR without turning this graph into a duplicate forge event store.
