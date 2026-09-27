# Self-improvement gap analysis: what agent-seddon needs to work its own gap list

Date: 2026-09-26 · Checkout: `main` `5ddcda7` · Companion to the [general gap analysis](README.md).

Scope: the **loop** in which agent-seddon takes an outstanding gap (or any feature request) and
closes it with grounded, non-hallucinated knowledge of the repository. The general analysis asks
"what was designed but not built?"; this one asks "what does the agent need in order to *answer
that question itself* and then act on it?". The design that closes the gaps found here is the
[`design/repo-knowledge/`](../design/repo-knowledge/README.md) track.

Status legend, as the `STATUS.md` trackers use it: ✅ built · 🟡 partial · ⬜ designed only · ❌ not designed / absent.

Method: read-only sweeps of the AST seam, the search and review crates, the Postgres tiers, the
tenancy plumbing, the fleet orchestrator and the docs index, then a spot-check of every claim
against the code. Docs were not trusted. Every statement about existing code carries a
`path:line` against `main` `5ddcda7`.

---

## 0. Summary

The loop has four stages. Stage (a), turning a gap into structured requirements plus a repo
brief, is **absent**: the agent takes free-text goals and the Implement mode injects nothing. Stage
(b), a deterministic code graph the model can query, is **partial**: the Go engine builds a real
typed graph but only in memory, with ids that change on every run; the SCIP engine keeps
definitions and drops references, so Rust, the language this repo is written in, has no call or
reference edges at all; nothing is persisted, nothing is keyed by repo or tenant. Stage (c), a
cited, LLM-written inventory of features, capabilities and tests, is **absent**: the only
per-repo prose is `docs/architecture.md`, which is human-written and stale. Stage (d), reusing
(b) and (c) when a PR arrives, is **partial**: the review brief exists and is byte-budgeted, but
every collector recomputes from scratch, the duplicate signal is a literal name search, and the
fleet reviewer cannot call any graph tool.

The Postgres pattern needed for all of this exists (`PgDigests`, PG-07) and the tenant plumbing
exists (S2). What is missing is the schema, the extractors, the inventory and the consumers. The
track closes them as increments RK-01 to RK-18.

---

## 1. The self-improvement loop as capabilities

| Stage | Capability needed | Status |
|---|---|---|
| (a) Gap → requirements | Take a gap id or a goal, produce a structured requirement plus a repo-context brief the model can cite | ❌ |
| (b) Deterministic code graph | A graph built by parsers, not by the model, persisted per `(tenant, repo, commit)`, with stable node keys, queryable by the model in bounded slices | 🟡 (Go in memory only; Rust has no edges; nothing persisted) |
| (c) Cited inventory | Features, capabilities, seams, tools, tests per repo: skeleton derived deterministically from (b), prose written by an LLM but every claim cites a graph key and is rejected otherwise | ❌ |
| (d) PR-time and feature-time reuse | When a PR arrives or a feature is requested, pull the precomputed slice of (b) and (c) from Postgres: touched nodes, callers, covering tests, dependents, similar existing symbols, crate summaries | 🟡 (brief exists; nothing precomputed; duplicate signal is a literal search) |
| (e) Gap → campaign → PRs | A persisted, hierarchical, ACID task tree per `(tenant, repo)` that an LLM decomposes, with the brief from (a)–(c) attached, until every leaf is small enough to execute; a driver that farms leaves to sandboxed workers and tracks their PRs | ❌ (flat in-memory `Todo` only; no hierarchy anywhere) |

Multi-tenancy and multi-repo are cross-cutting: agent-seddon is tenant one, repo one, and the
fleet already reviews many repos for many users from one process, so every table must lead with
the tenant and every read must be scoped.

---

## 2. What exists versus what is missing, per stage

### 2.1 The graph today

| Fact | Evidence | Verdict |
|---|---|---|
| The Go engine builds a typed graph (CHA call edges, implicit interface satisfaction) from a pinned helper's JSON | `crates/agent-ast/src/go.rs:30`, `helpers/go-graph/main.go:140` | ✅ for Go |
| That graph lives in one `RwLock<Option<Arc<Graph>>>` per process; symbol ids are dense positions in the helper's output, so they change whenever the source changes | `crates/agent-ast/src/go.rs:30`, `crates/agent-ast/src/graph.rs:34-52` | ❌ persistence, ❌ stable ids |
| Caps: 20 000 symbols and 100 000 edges per graph | `crates/agent-ast/src/graph.rs:18-19,89,123` | 🟡 (fine for one repo, not for a store) |
| The helper is invoked with `Tests: false`, so Go test functions are never nodes | `helpers/go-graph/main.go:140` | ❌ test nodes |
| One repo per process: the root is found by walking up to `.git` from the cwd | `crates/agent-runtime/src/ast.rs:99-104` | ❌ multi-repo |
| `AstBackend` is the seam (symbols, implementations, interface_of, callers, callees, callchain, blast radius, dependency path); `DispatchAst` routes graph verbs to the first capable engine | `crates/agent-core/src/lib.rs:4909`, `crates/agent-ast/src/lib.rs:46-77` | ✅ seam |

### 2.2 Rust coverage

| Fact | Evidence | Verdict |
|---|---|---|
| The SCIP ingester keeps only definition occurrences and `is_implementation` relationships; reference occurrences are dropped | `crates/agent-ast/src/model.rs:223-273` | ❌ Rust calls / references |
| `rust-analyzer scip` is the Rust indexer, run through the Sandbox | `crates/agent-ast/src/scip.rs:42-43` | 🟡 (heavy, optional, no references) |
| `[ast] backends = ["rust"]` is silently skipped with a warning; there is no native Rust engine | `crates/agent-runtime/src/ast.rs:77` | ❌ |
| Review signatures for Rust come from regexes, not a parser | `crates/agent-review/src/signatures.rs` | 🟡 |
| `syn`, `toml`, `ignore` and `sha2` are already in `Cargo.lock`; `toml = "0.8"` and `ignore = "0.4"` are workspace deps; no crate depends on `syn` directly | `Cargo.toml:91,114`, `Cargo.lock` | 🟡 (cheap to add) |

### 2.3 Storage and tenancy

| Fact | Evidence | Verdict |
|---|---|---|
| There is no `repos` table anywhere; the config store is a blob store (`cards`, `tenants`) | `crates/agent-config-store/migrations/0001_config_store.sql:10-14` | ❌ |
| The pattern to copy exists: `PgDigests` embeds `MIGRATIONS: &[(i64, &str)]`, takes `pg_advisory_xact_lock`, records a per-crate ledger, offers `connect` / `connect_lazy` / `from_pool`, and gates its live tests on a DSN env var | `crates/agent-digest/src/postgres.rs:44,69,86,96,113,119` | ✅ pattern |
| sqlx 0.8 is used without `macros` or `migrate` (RUSTSEC-2023-0071 posture) | `Cargo.toml:144` | ✅ constraint |
| Tenant is a `String` from the verified principal: `current_tenant()` / `scoped_tenant()` (S2) | `crates/agent-core/src/identity.rs:279,298` | ✅ |
| Stores get the tenant through a `with_tenant(&str)` view wrapped in `PerTenant<S>` | `crates/agent-runtime/src/tenant.rs:71` | ✅ |
| DSNs are resolved from `env:` / `file:` references only | `crates/agent-runtime/src/store_backend.rs:26` | ✅ |
| Fleet review contexts are cached by `row.id` alone, not `(tenant, row.id)` | `crates/agent-runtime/src/fleet_review.rs:161,179-180` | 🟡 (general analysis §10 P1) |

### 2.4 Inventory

| Fact | Evidence | Verdict |
|---|---|---|
| No per-repo structured record of features, seams, tools or tests exists; `docs/architecture.md` is human-written and behind the code | `docs/README.md:6,16`, general analysis §9 | ❌ |
| The digest tier summarises **sessions**, not repos, and is unwired (PG-08) | `crates/agent-digest/src/lib.rs:51`, general analysis §10 | ❌ for repos |
| `agent-graph` is the cognition graph (thoughts, not code) | `docs/design/cognition-graph/` | not applicable |
| The safety template for LLM-written text already exists: `sanitize` (16 KiB cap) plus `scan_for_injection` | `crates/agent-digest/src/lib.rs:43,51`, `crates/agent-core/src/security.rs:97` | ✅ template |
| An `Embedder` seam and a 256-dim local embedder exist; similarity is brute-force cosine in Rust; no pgvector | `crates/agent-core/src/lib.rs:1526`, `crates/agent-embed/src/local.rs:41-49`, `crates/agent-search/src/vector.rs:2` | ✅ reusable |

### 2.5 PR consumption

| Fact | Evidence | Verdict |
|---|---|---|
| The review brief is assembled from `FactCollector`s into `ReviewFacts` and rendered under a byte budget | `crates/agent-review/src/collector.rs:14-118`, `crates/agent-review/src/lib.rs:205-209`, `crates/agent-core/src/lib.rs:5929` | ✅ |
| `CollectCtx` carries no `AstBackend`; the call-graph collector is Go-only via the Sandbox | `crates/agent-review/src/collector.rs:14`, `crates/agent-runtime/src/builder.rs:78` | 🟡 |
| The only "is this a duplicate?" signal is `nearby`: a literal search for each newly declared name | `crates/agent-review/src/nearby.rs:1-10`, `crates/agent-runtime/src/builder.rs:114` | 🟡 |
| Each collector is gated by a `ReviewCfg` bool and status-logged to ClickHouse | `crates/agent-runtime/src/config.rs:1401,1454,1518` | ✅ pattern |
| The fleet prepends the brief to the goal; the reviewer's tool set excludes `search` and `find_*` | `crates/agent-review-fleet/src/orchestrator.rs:459,708-713`, `crates/agent-runtime/src/agent.rs:3153` | 🟡 (push-only by design) |
| Fleet-grounding Increment 3 already proposes a per-repo index keyed by `row.id` + `head_sha`, measure-gated | `docs/design/fleet-grounding/README.md:95-98` | ⬜ (subsumed by the track) |

### 2.6 Feature and gap intake

| Fact | Evidence | Verdict |
|---|---|---|
| Scheduler jobs and sessions take free-text goals only | `crates/agent-core/src/lib.rs:3832,3878` | ❌ |
| `TaskMode::Implement` and `Design` exist; the review-mode block in `Session::send_inner` is the hook shape, but Implement injects nothing | `crates/agent-core/src/lib.rs:5310`, `crates/agent-runtime/src/agent/session.rs:339-352` | ❌ |
| No gap tracker, no gap-to-goal mapping | `docs/gap-analysis/` is prose only | ❌ |
| The only plan store is a flat, in-memory `Todo { content, status, priority }` behind `TaskTracker`; no table, struct or migration has a parent pointer, materialized path, `ltree` or closure table | `crates/agent-core/src/lib.rs:1055,1074`, `crates/agent-tasks/src/memory.rs`; the one parent-linked structure is the session checkpoint DAG, `crates/agent-session/src/file.rs:19` | ❌ hierarchy |
| Sub-agents are serial, depth-capped and off by default; scheduler jobs are flat free-text goals | `crates/agent-runtime/src/subagent.rs:39`, `crates/agent-scheduler/src/store.rs:243` | ❌ farming out |
| The pieces a work tree needs exist separately: owner-token claims with TTL reclaim (scheduler), `FOR UPDATE` compare-and-swap (config store), schema-validated LLM JSON with repairs, worktree + push + `create_pr` seams | `crates/agent-scheduler/src/store.rs:20-31`, `crates/agent-config-store/src/postgres.rs:287-302`, `crates/agent-runtime/src/structured.rs:36`, `crates/agent-core/src/lib.rs:4007,5284,5292` | ✅ parts |

### 2.7 Synergies with the general analysis

- **§5.1 test audit**: test nodes in the graph carry the case-class prefix
  (`positive_` / `negative_` / `corner_` / `boundary_` / `adversarial_`), so "which crates lack
  adversarial cases?" becomes a `GROUP BY`, not a grep.
- **§6 LLM awareness**: graph slices pushed into the brief mean the fleet reviewer sees callers and
  tests without being granted `find_*`.
- **§9 docs**: `doc` nodes and `documents` edges make "which doc describes this crate?" and
  "which docs cite a path that no longer exists?" answerable from the same store.

### 2.8 What an LLM needs to work in a new repo

This is the requirements source for the track. The list is what a model needs in front of it to
modify or review code in an unfamiliar repository. Each row is scored against today's code.

| # | Need | Deterministic? | Today | Evidence |
|---|---|---|---|---|
| 1 | Architecture map: layers, what depends on what | yes (Cargo graph) | ❌ | `docs/architecture.md` is human-written and stale |
| 2 | Crate / package map: purpose of each unit in one line | prose | 🟡 | `docs/components/*.md`, uneven |
| 3 | Entry points and ports: binaries, subcommands, `--serve-*`, listeners | yes | ❌ | `nix/constants.nix` has ports; nothing joins them to code |
| 4 | Seams, impls and the config-string → impl map | yes (`register_builtins`) | 🟡 | `crates/agent-runtime/src/registry.rs:469`; `docs/extending.md` prose |
| 5 | Layering rules: what may depend on what | yes | ❌ | not recorded |
| 6 | Call graph and blast radius | yes | 🟡 | Go only, in memory, `crates/agent-ast/src/go.rs:30` |
| 7 | Data model and persisted schemas: tables, protos, columns | yes | ❌ | migrations and `.proto` files exist; nothing indexes them |
| 8 | Config surface: every key, default, section | yes (`serde` structs) | ❌ | `crates/agent-runtime/src/config.rs`, no index |
| 9 | Conventions: error type, async runtime, test harness, lint policy | mostly | 🟡 | `CLAUDE.md` prose |
| 10 | Build, test and gate commands | yes | 🟡 | `CLAUDE.md`, `nix/checks/` |
| 11 | Test map: which tests cover which code, test doubles, fixtures | yes | ❌ | none |
| 12 | Hot spots: churn, recency, bus factor, co-change | yes (git) | 🟡 | `churn.rs` / `cochange.rs` compute per PR, discard after |
| 13 | In-flight work: recent commits by area, open increments | yes | 🟡 | `STATUS.md` files, not queryable |
| 14 | Known debt: `TODO`, `FIXME`, `#[ignore]`, `#[allow]` | yes | ❌ | none |
| 15 | Trust boundaries and guard funnels: which functions every untrusted input must pass | yes (graph) | ❌ | `confine` / `safe_segment` / `scan_for_injection` are known only from `CLAUDE.md` |
| 16 | Error types and how they propagate | yes | ❌ | none |
| 17 | Public API surface per crate | yes | ❌ | none |
| 18 | Metrics, spans, feature flags | yes (literal scan) | ❌ | `agent-metrics` registers them; no index |
| 19 | Change recipes: "to add a seam impl, touch these files" | yes (git touch sets) | 🟡 | `docs/extending.md` prose only |
| 20 | Glossary of repo terms | prose over deterministic candidates | ❌ | none |

Score today: **0 ✅ · 8 🟡 · 12 ❌**. Every 🟡 is prose that must be read whole or a computation
that is thrown away after one review.

**Delivery-shape requirement.** The same knowledge is useless if it arrives as 200 KB. The track
must deliver in layers: a "do not" list and an overview of at most 2 KB first (L0), per-crate and
per-feature cards next (L1), and on-demand slices through a tool last (L2). Every item must carry a
key the model can cite verbatim (`rust:fn:agent_core::security::confine`), so a reviewer's claim
can be checked against the store rather than believed.

---

## 3. Gap list

| # | Gap | Closes in |
|---|---|---|
| SI-1 | No persisted, stable-keyed code graph; nothing survives the process | RK-01, RK-02, RK-06 |
| SI-2 | Rust has no parser-derived graph; SCIP drops references | RK-03 (syn + Cargo), RK-05 (SCIP references) |
| SI-3 | No `repos` table, no `(tenant, repo, commit)` identity for anything | RK-02 |
| SI-4 | The model cannot ask structural questions ("shape of this repo", "tests covering X") in bounded slices | RK-08 |
| SI-5 | Review recomputes every fact; the only duplicate signal is a literal name search | RK-09, RK-17 |
| SI-6 | No cited inventory of features, seams, tools and tests per repo | RK-10, RK-11 |
| SI-7 | Implement / Design mode receives no repo context; gaps have no intake | RK-12 |
| SI-8 | The fleet indexes nothing ahead of a PR; cache key ignores the tenant | RK-07 |
| SI-9 | No repo profile: entry points, schemas, config keys, factory map, error types, API surface, debt, metrics | RK-15 |
| SI-10 | No history-derived knowledge: hot spots, in-flight work, change recipes | RK-16, RK-18 |
| SI-11 | No campaign planning: no hierarchical task tree, no "small enough?" decomposition loop, no worker driver that turns leaves into PRs | [`design/campaigns/`](../design/campaigns/README.md) CP-01..CP-06 |

---

## 4. Relationship to the general analysis

- General §6 (LLM awareness of the AST tools) is addressed by pushing slices into the brief and,
  in the interactive loop, by one `repo_graph` tool with a `question` discriminator.
- General §7 (repo knowledge graph) is the seed of the track; its status line now points here.
- General §10 P2 (LLM awareness) and P3 (repo knowledge graph) are both closed by
  [`design/repo-knowledge/`](../design/repo-knowledge/README.md); STATUS lives there.
- The `code-graph` track keeps the `AstBackend` seam and engines; the track adds a Postgres-backed
  engine (`PgAst`) behind the same seam rather than a second seam for the same verbs.
- Stage (e) and SI-11 are closed by [`design/campaigns/`](../design/campaigns/README.md): the
  work tracker that consumes the repo-knowledge brief and turns a gap into a tree of tasks and
  PRs. The gap docs remain the source of gaps; a campaign's `source_ref` points back here.
