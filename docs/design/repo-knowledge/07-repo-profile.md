# 07 — Repo profile: deterministic knowledge beyond the code graph

Everything here is an `Extractor` ([`02-extraction.md`](02-extraction.md): same trait, budget,
validation and determinism contract) that writes graph nodes and attrs, rows in `repo_facts`,
rows in `repo_history`, or edges ([`01-schema.md`](01-schema.md), migration 0003). Nothing is
model-derived. The rows are the deterministic half of the top-20 list in the
[self-improvement gap analysis §2.8](../../gap-analysis/self-improvement.md).

## Analyses

| Analysis | Source | Lands as | Increment |
|---|---|---|---|
| Entry points: `[[bin]]`, `fn main`, clap subcommands and flags (`#[command]`, `#[arg]`), `--serve-<seam>` | syn + `Cargo.toml` | `attrs.entry_point` on the fn; `repo_facts kind=cli` (one row per subcommand: name, flags, crate) | RK-15 |
| Served services, RPCs, ports, message types | `*.proto` (`service`, `rpc`, `message`) + `nix/constants.nix` | `proto_service` / `proto_rpc` / `proto_message` nodes with `defined_in`; `attrs.port` from constants; `implements` edge from the tonic impl (`impl <Svc> for …`) to the service | RK-15 |
| Persisted schemas: SQL migrations, ClickHouse `schema.sql`, sqlite inline DDL | `crates/*/migrations/*.sql`, `nix/clickhouse/schema.sql` | `table` nodes with columns in `attrs`; `defined_in`; `references` edges from Rust files whose string literals name the table (`attrs.via = "literal"`) | RK-15 |
| Config keys: struct fields under the config root, `#[serde(default = …)]`, doc line, `deny_unknown_fields` | syn over `profile.config_root` (`crates/agent-runtime/src/config.rs` here) | `config_key` nodes `cfg:<section>.<key>` with `attrs.default`, `attrs.doc`; `config_section` features in the inventory | RK-15 |
| Factory map: config string → impl | syn over `register_builtins` (`crates/agent-runtime/src/registry.rs:469`): string literal → path inside the closure | `attrs.registry_key` on the impl node; `repo_facts kind=factory` (key, seam, impl key, cargo feature) | RK-15 |
| Error types and propagation | syn: enums named `Error` or `*Error`, `impl From<X> for Error`, `thiserror` attributes | `attrs.is_error_type`; `references` edges for each `From` | RK-15 |
| Public API surface | syn `pub` visibility plus `pub use` re-exports | `attrs.api = true`; `repo_facts kind=api_summary` per crate (counts by kind) | RK-15 |
| Complexity per fn | syn: lines, max nesting depth, branch count (`if`, `match`, loops, `?`), parameter count | `attrs.lines`, `attrs.nesting`, `attrs.branches`, `attrs.params`; `repo_facts kind=hotspot_complexity` top 10 per crate | RK-15 |
| Debt: `TODO`, `FIXME`, `XXX`, `deferred`, `follow-up` comments; `#[ignore = ""]`; `#[allow(clippy::…)]`; `unsafe` blocks | syn attrs + a bounded comment scan (≤ 160 B per hit, injection-screened) | `repo_facts kind=debt` (file, line, category, ≤ 160 B text) | RK-15 |
| Metrics, spans, log targets | literal scan of `register_*`, `IntCounterVec::new`, `info_span!`, `tracing::*` first-argument literals | `metric` / `span` nodes with `defined_in`; `capability:observability` feature rows | RK-15 |
| Gates and commands | `nix/checks/*.nix` names, `flake.nix` apps, `CLAUDE.md` fenced commands | `repo_facts kind=gate`, `kind=command` | RK-15 |
| Test doubles and fixtures | syn: types in `profile.testkit_crate`, names matching `Fake*`, `Fault*`, `Mem*`, `*Probe`; `#[fixture]` | `attrs.is_double`; `repo_facts kind=double` | RK-15 |
| Hot spots: commits in 90 days, last touched, unique authors, bus factor, churn slope per file | `git log --numstat` through `RepoBackend`, the `churn.rs` math reused (`crates/agent-review/src/churn.rs:34`) | `repo_history` rows | RK-16 |
| Co-change | the `cochange.rs` math reused over the same log (`crates/agent-review/src/cochange.rs:39`) | `co_changes_with` edges (`weight` = confidence, `attrs.count`) | RK-16 |
| In-flight: commits since the previous snapshot grouped by crate; `STATUS.md` rows 🟡 / ⬜; branches ahead of the default | git log + markdown table parse | `repo_facts kind=inflight` | RK-16 |
| Change recipes: touch sets of commits clustered by rule (`feat(<scope>)`, paths matching `migrations/`, `register_builtins`, `*.proto`, `nix/checks/`) | git log file lists + the rule set in `repos.profile.recipe_rules` | `repo_facts kind=recipe` (kind, ordered file list, ≤ 5 example shas) | RK-16 |
| Chokepoints: top in-degree fns over `calls` + `references`; guard funnels: callers of `confine`, `safe_segment`, `scan_for_injection` (`profile.guards`) | graph SQL | `repo_facts kind=chokepoint`, `kind=guard` | RK-17 |
| Guard gaps: `impl Tool` `call` fns whose call subgraph (≤ 4 hops) never reaches a guard while a parameter is named `path`, `id`, `name`, `rev` | graph SQL + attrs | `repo_facts kind=guard_gap` (the review brief flags a PR that adds one) | RK-17 |
| Near-duplicates: winnowed token shingles (k = 7, w = 4) over fn bodies, Jaccard ≥ 0.7, cross-crate pairs first | syn token streams | `similar_to` edges, `weight` = Jaccard | RK-17 |
| Orphans and dead pub items: pub fns with no `calls` / `references` in-edges and no `api` re-export | graph SQL (meaningful only with SCIP) | `repo_facts kind=orphan` | RK-17 |

## Consumers

- The `repo_graph` tool gains `EntryPoints`, `Schemas`, `ConfigKeys`, `Debt`, `HotSpots`,
  `InFlight`, `Recipe`, `Duplicates`, `Guards` ([`03-queries.md`](03-queries.md)).
- The review collector adds lines for: hot-spot or low-bus-factor file touched; duplicate of
  `<key>` with its Jaccard; a new guard gap; a schema or proto touched without a migration or
  baseline change in the same PR ([`05-consumption.md`](05-consumption.md)).
- The Implement-mode brief adds the matching `playbook` and `recipe`.

All bounded: at most 8 items per category, inside the existing byte caps.

## Threats specific to this doc

| Input | Handling |
|---|---|
| Comment text (`TODO` scan) | ≤ 160 B, `scan_for_injection`, rendered in the brief as a count plus `file:line`, never as text |
| Commit subjects (recipes, pitfalls) | ≤ 200 B, injection-screened, only the sha is rendered in briefs |
| Author identities (bus factor) | counted, never stored by name; `unique_authors` and `bus_factor` are integers |
| String literals that look like table names | matched against the known `table` node set only; no new nodes from literals |
| Recipe rules | rule-based, per-repo configurable, never learned from model output |
