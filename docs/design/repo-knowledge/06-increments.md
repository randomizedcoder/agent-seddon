# 06 — Increments: RK-00 to RK-18

One PR per increment. Each is gated by `nix flake check` (clippy `-D warnings`, rustfmt, tests,
cargo-audit, buf, bench, leak, mt-audit, constants-sync) plus whatever the row adds. Tests are
table-driven `rstest` with the four case classes and `adversarial_` cases for every
untrusted input (repo content, model-supplied keys, tenant strings).

| ID | Increment | Lane | Depends | Adds to `nix flake check` |
|---|---|---|---|---|
| RK-00 | This track, `docs/gap-analysis/self-improvement.md`, index and back-links | — | — | — |
| RK-01 | `RepoGraphStore` seam + value types in `agent-core`; crate `agent-repo-graph`: model, key grammar, validation, `GraphBuilder`, `MemRepoGraph` double in `agent-testkit` | A | RK-00 | unit tests: key round-trip, caps, confinement, `graph_hash` determinism, id-collision failure |
| RK-02 | `PgRepoGraph` (feature `postgres`), migration 0001, `with_tenant`, `UNNEST` bulk write with collision check, every read verb; `#[ignore = "requires a live Postgres (AGENT_REPO_GRAPH_TEST_DSN)"]` suite with `TRUNCATE`; `nix/pg-integration.nix` step | A | RK-01 | clippy all-features; pg suite in `nix run .#integration`; adversarial cross-tenant reads return nothing |
| RK-03 | Extractors `rust-syn`, `cargo`, `docs`; fixture workspace `tests/fixture/ws` (a seam trait, an impl, a cargo feature, rstest cases of every class, a cfg-gated fn, a `mod` with `#[path]`); `nix/checks/repo-graph-rust.nix` (index twice ⇒ equal hash, pinned expected hash); iai bench with an Ir ceiling; dhat leak test | B | RK-01 | `repo-graph-rust`, `bench`, `leak` |
| RK-04 | Go mapping from the helper JSON + `_test.go` scan; `nix/checks/repo-graph-go.nix` over the existing ast-go fixture | B | RK-03 | `repo-graph-go` |
| RK-05 | SCIP spike report (as-built entry: time, RSS, index size, `enclosing_range`, join rate) + `extract-scip` extractor: resolved `calls` / `references` / `tests(via=scip)`, `res_key` join; check modelled on `ast-scip.nix` | B | RK-03 | `repo-graph-scip` when the fixture builds offline, else the `integration` app |
| RK-06 | `Indexer`, `[repo_graph]` config, `agent repo add\|index\|status\|diff\|shape`, retention sweep, metrics `agent_repo_graph_index_seconds`, `_nodes`, `_edges`, `_truncated_total` | C | RK-02, RK-03 | `config-roundtrip`, `cli-help` |
| RK-07 | Fleet hook (merge-base index off the critical path, in-flight dedup by `(tenant, slug, sha)`); `FleetReviewFactory` supplies the tenant view; cache key `(tenant, row.id)` | C | RK-06 | orchestrator tests with `MemRepoGraph` + a fake hook; a test that two tenants with the same `row.id` do not share a context |
| RK-08 | `repo_graph` tool (feature `tool-repo-graph`) + `PgAst` (`[ast] backends = ["pg"]`) + registry lines in `register_builtins` (`crates/agent-runtime/src/registry.rs:469`) + `docs/components/repo-graph.md` | D | RK-06 | tool tests over the fixture; adversarial args (huge hops, unknown key, key with whitespace) |
| RK-09 | `RepoKnowledgeCollector`, `FactFragment::RepoKnowledge`, `ReviewFacts.repo_knowledge`, `render_repo_knowledge`, `ReviewCfg.repo_knowledge` (off), ClickHouse status, `nix/checks/review-repo-knowledge.nix`, measure-gate note in STATUS | D | RK-07, RK-08 | `review-repo-knowledge` |
| RK-10 | Inventory skeleton + migration 0002 + `agent repo inventory` | E | RK-06 | fixture assertions: one `seam`, one `tool`, one `cargo_feature` row with the expected evidence roles |
| RK-11 | LLM summaries with fail-closed citations, sanitize + injection screen, embeddings, `Similar` / `Feature` questions; `agent repo summarize [--dirty-only]` | E | RK-10 | fake-provider tests: unknown citation, injected text, dim mismatch, oversize text all rejected; unchanged `subgraph_hash` makes no call |
| RK-12 | Implement / Design-mode brief hook + `agent repo brief --goal` | E | RK-11 | session tests: injected once, bounded, absent when the store is off |
| RK-13 | `RepoGraphService` gRPC (`scoped`) + mt-audit manifest row + `class_of` + constants + `--serve-repo-graph` | opt | RK-08 | `buf`, `mt-audit`, `constants-sync` |
| RK-14 | Postgres RLS across all Pg tiers; filed under multi-tenancy plane 02, tracked there | opt | — | — |
| RK-15 | Profile extractors ([`07-repo-profile.md`](07-repo-profile.md) rows marked RK-15) + migration 0003 (`repo_facts`, `repo_history`); fixture extended with a `.proto`, a migration, a clap bin, a `TODO`, an `#[ignore]`, a metric registration | B | RK-03 | `repo-graph-rust` assertions extended |
| RK-16 | History extractors: `repo_history` (churn, recency, bus factor, slope; reusing the `churn.rs` math), `co_changes_with` edges (reusing `cochange.rs`), in-flight facts, change recipes; `Indexer` gets `RepoBackend` log access | C | RK-06, RK-15 | fixture repo with a synthetic git history built by `agent-testkit` |
| RK-17 | Derived analyses over the stored graph: chokepoints, guard funnels and guard-gap detection, near-duplicate `similar_to` edges (winnowing), orphans; tool questions and review-brief lines for each | D | RK-09, RK-16 (RK-05 for orphans) | tool and collector tests over the fixture; `review-repo-knowledge` extended |
| RK-18 | LLM extras: `conventions`, `glossary`, `playbook:*`, `pitfalls`, `overview_l0`; L0 / L1 / L2 layering in both briefs | E | RK-11, RK-16 | fake-provider tests: citations validated; a playbook naming a file outside its recipe is rejected |

## Lanes

```
A  RK-01 ──► RK-02 ─────────────────────────────┐
B  RK-01 ──► RK-03 ──► RK-04 ──► RK-15 ──► RK-05 │
C                      RK-02 + RK-03 ──► RK-06 ──► RK-07 ──► RK-16
D                                        RK-06 ──► RK-08 ──► RK-09 ──► RK-17
E                                        RK-06 ──► RK-10 ──► RK-11 ──► RK-12 ──► RK-18
```

Lane B runs in parallel with A after RK-01. RK-15 rides lane B right after RK-03 because it is
the same extractor pass and cheap. RK-16 waits for the indexer to have git access. RK-17 and
RK-18 are last. First value for agent-seddon as tenant one is RK-03 + RK-06; first model-visible
payoff is RK-08 and RK-09; RK-05 is the largest quality jump and must not block RK-06 to RK-09.

## Open questions (recorded, not blocking)

| Question | Recommendation |
|---|---|
| Should each `#[case]` of an rstest be its own `test` node? | Yes. About 2.5 k extra rows here; it is what makes "which crates lack `adversarial_` cases?" a query. |
| Materialise `co_changes_with` from the review-time `cochange` collector as well? | No. One source (the RK-16 extractor at index time); the collector keeps reading its own window per PR. |
| Flip `helpers/go-graph` to `Tests: true`? | Separate helper PR; until then the `_test.go` scan covers it. |
| Store the SCIP index blob? | No. Only the edges; the index is rebuilt per snapshot. |
