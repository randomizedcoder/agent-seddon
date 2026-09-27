# 02 — Extraction: deterministic extractors, budgets, validation, determinism

Every extractor is a parser or `git`. None calls a model. Output is sorted, versioned, budgeted
and validated before it reaches the store.

## The `Extractor` trait

```rust
pub trait Extractor: Send + Sync {
    fn name(&self) -> &'static str;          // "rust-syn" | "cargo" | "docs" | "go-graph" | "scip-rust" | …
    fn version(&self) -> &'static str;       // bumped on any output-affecting change
    fn extract(
        &self,
        root: &Path,
        out: &mut GraphBuilder,
        budget: &ExtractBudget,
    ) -> Result<ExtractReport>;
}

pub struct ExtractBudget {
    pub max_files: usize,        // 50_000
    pub max_file_bytes: u64,     // 2 MiB
    pub max_nodes: usize,        // 250_000
    pub max_edges: usize,        // 2_000_000
    pub deadline: Instant,
}

pub struct ExtractReport {
    pub files: usize,
    pub skipped: usize,          // over budget, not confined, binary
    pub parse_errors: usize,
    pub truncated: bool,         // a cap was hit
    pub dropped_edges: usize,    // dangling after validation
    pub diagnostics: Vec<String>,// ≤ 32 entries, ≤ 200 B each, file paths only, never content
}
```

`GraphBuilder` accumulates nodes and edges keyed by `node_key`, dedups, assigns `node_id` by
`sha256(node_key)[..8]`, checks for id collisions (fail), and at `finish()` sorts by key and
computes `graph_hash` over the sorted `(node_key, kind, sig_hash, body_hash)` list plus the
sorted edge list. `extractor_version` for a snapshot is the sorted `name@version` list joined
with `,`.

## Walk

`ignore::WalkBuilder` (`ignore = "0.4"` is already a workspace dep, `Cargo.toml:114`) with
`follow_links(false)`, `.gitignore` honoured, output sorted by path. Always skipped: `target/`,
`.git/`, `.agent-seddon/`, `node_modules/`, `vendor/`. Every path passes
`agent_core::confine` (`crates/agent-core/src/security.rs:186`) against the root before it is
opened; a path that fails is counted in `skipped`, never reported by name in the brief.

## `rust-syn`

Runs in-process under `spawn_blocking` with the budget deadline. Dependencies: `syn` with
`full`, `visit` and `extra-traits`; `proc-macro2` with `span-locations`. Per file at most
2 MiB; larger files produce a `file` node only.

Item visitor with a module-path stack and a `cfg` stack:

| Construct | Node | Notes |
|---|---|---|
| `ItemFn` | `fn`, or `test` when `#[test]`, `#[tokio::test]`, `#[rstest]` is present | `attrs.async`, `attrs.unsafe`, `attrs.params`, `attrs.ret` (token text, ≤ 256 B) |
| `#[rstest]` fn with `#[case::<name>(…)]` | one `test` node per named case, key `…::<fn>::<case>` | `attrs.test_class` from the case prefix (`positive_`, `negative_`, `corner_`, `boundary_`, `adversarial_`, else `other`); the parent fn is also a `test` node with `attrs.cases = N` |
| `#[ignore]` / `#[ignore = "…"]` | on the test node | `attrs.ignored = true`, `attrs.ignore_reason` ≤ 160 B |
| `ItemStruct`, `ItemEnum`, `ItemTrait`, `ItemType`, `ItemConst`, `ItemStatic`, `ItemMacro` (macro_rules) | matching kind | `attrs.fields` / `attrs.variants` counts; trait `attrs.async_trait` when the attribute is present |
| `ItemImpl` | `impl` + `impl_for` edge to the self type + `implements` edge to the trait when present + one `method` node per fn | trait path normalised to the last two segments; unresolved trait path recorded as `attrs.trait_path` |
| `ItemMod` inline | `module` + `contains` | |
| `ItemMod` `mod x;` | resolved to `x.rs`, `x/mod.rs`, or `#[path = "…"]` (confined) | unresolved ⇒ `attrs.unresolved = true` |
| `ItemUse` | `imports` edges | target resolved to a workspace key when the leading segment is a workspace crate or `crate` / `super` / `self`; else `attrs.resolved = false` with the external path |
| `#[cfg(feature = "f")]`, `#[cfg(all(…))]`, `#[cfg(any(…))]` | `gated_by` edges, one per feature named | `not(…)` is recorded in `attrs.cfg` but produces no edge |
| Doc comments | `attrs.doc` | first paragraph, ≤ 512 B, dropped when `scan_for_injection` (`crates/agent-core/src/security.rs:97`) returns `Some` |

Hashes: `sig_hash` is sha256 over the item's signature tokens with doc comments and attributes
stripped (so a comment edit does not change it); `body_hash` is sha256 over the whole item's
token stream with comments stripped. `exported` is true for `pub` and `pub(crate)` items at
module level.

Test targets (`tests` edges, `attrs.via = "name"`): a test named `<x>_…` or `…_<x>` where `<x>`
is a fn or method name in the same crate; the enclosing `#[cfg(test)] mod tests` binds the
search to its parent module first. Noisy by design; RK-05 replaces these edges with resolved ones.

## `cargo`

The `toml` crate (`toml = "0.8"`, `Cargo.toml:91`), no `cargo metadata` subprocess. Workspace
`members` (globs expanded over the confined walk) → `crate` nodes with `attrs.version`,
`attrs.edition`, `attrs.lib`, `attrs.bins[]`; `[features]` → `feature` nodes plus a `contains`
edge from the crate; `path` and `workspace = true` dependencies → `depends_on` with `attrs.dev`,
`attrs.optional`, `attrs.features[]`; external dependencies are counted in
`crate.attrs.external_deps` and produce no node.

## `docs`

`docs/**/*.md`, `README.md`, `CLAUDE.md`, `DESIGN.md` → `doc` nodes with `attrs.title` (first
`#` heading, ≤ 256 B) and `attrs.headings` (count). Markdown links and backtick paths that
resolve to `crates/<x>` or to an existing repo path → `documents` edges to the crate or `file`
node. Links to nothing are counted in `attrs.broken_links` (general gap analysis §9).

## `go-graph`

Runs the pinned `agent-go-graph` helper through the Sandbox exactly as `GoAst` does
(`crates/agent-ast/src/go.rs:112`), then maps the helper's JSON directly into keyed nodes and
edges instead of going through `agent_ast::Graph` (whose ids are positional,
`crates/agent-ast/src/graph.rs:34-52`). Packages, funcs, methods, types, interfaces,
`implements` and CHA `calls` edges map one-to-one. The helper is invoked with `Tests: false`
(`helpers/go-graph/main.go:140`), so `_test.go` files get a syntactic scan for `func Test…` and
`func Benchmark…` → `go:test` nodes with `tests` edges by name. Flipping the helper to
`Tests: true` is a separate helper PR (open question in `06-increments.md`).

## `scip-rust` (feature `extract-scip`, RK-05)

Runs `rust-analyzer scip . --output …` through the Sandbox as `ScipAst` does
(`crates/agent-ast/src/scip.rs:42-43`), then ingests **all** occurrences, not only definitions
(`crates/agent-ast/src/model.rs:223-273` keeps definitions only):

1. Filter to symbols whose package is a workspace crate.
2. Collapse each SCIP descriptor chain to a resolution key `crate::mod::Type::method` and store
   it on the matching syn node as `attrs.res_key`; the match is by `(file, definition line)`
   against the per-file interval table built from syn item ranges.
3. For each reference occurrence: source = the enclosing syn item by `(file, line)` from the
   same interval table; target = the node whose `res_key` matches. Edge kind is `calls` when the
   target is a `fn` or `method`, else `references`.
4. Occurrences inside a `test` node produce `tests` edges with `attrs.via = "scip"`, replacing
   the name-heuristic edges for that test.
5. Ambiguity (one `res_key`, several nodes after cfg splits) fans out to at most 4 targets with
   `attrs.ambiguous = true`.

**Spike first, in the RK-05 as-built entry.** Measure on this workspace: wall time, peak RSS,
index size, whether `enclosing_range` and impl descriptors are emitted by the pinned
rust-analyzer, and the join rate (occurrences matched to a syn node). The extractor ships only if
the join rate is at least 95 %; otherwise the as-built entry records what blocks it.

## Comparison

| | A `rust-syn` + `cargo` | B `rust-analyzer scip` | C tree-sitter |
|---|---|---|---|
| Hermetic, no toolchain in sandbox | yes | no (needs a resolvable workspace) | yes |
| Runtime on this workspace | ≤ 5 s | 2–6 min | ≤ 5 s |
| Memory | tens of MB | 1.5–4 GB | tens of MB |
| Resolved calls / references | no | yes | no |
| Tests with case class | yes | no (needs syn anyway) | partial |
| Decision | v1 | v1.5, feature-gated, after spike | not taken |

Where each runs: `rust-syn`, `cargo`, `docs` in-process; `go-graph`, `scip-rust` through the
Sandbox with the existing timeouts. In the fleet sandbox SCIP needs a workspace that resolves
offline (vendored) or a network-allowed sandbox; that is a per-repo flag, default off.

## Determinism contract

- Output is sorted by `node_key` and by `(kind, src_key, dst_key)`.
- `extractor_version` is part of snapshot identity (`UNIQUE (tenant, repo_id, commit_sha,
  extractor_version)`), so a toolchain or extractor bump produces a new snapshot, not a silent
  drift.
- The hermetic check `nix/checks/repo-graph-rust.nix` indexes the fixture workspace twice in two
  fresh temp dirs and asserts equal `graph_hash`; a second assertion pins the fixture's expected
  hash so an unintended output change fails the gate and the diff records the bump.

## Validation

Generalises the posture of `Graph::parse` (`crates/agent-ast/src/graph.rs:75-123`): caps on
symbols and edges, bounds on every string, closed kind sets. Applied at `GraphBuilder::finish()`:

| Check | On failure |
|---|---|
| Node or edge cap | `truncated = true`; the snapshot is `ready` with `reason = "truncated"` unless `strict` is set, then `failed` |
| Key > 512 B, name > 256 B, attrs > 4 KiB, edge attrs > 1 KiB | node or edge dropped, counted |
| Key contains whitespace or non-ASCII | dropped, counted |
| Path not confined | dropped, counted, path never echoed |
| Edge endpoint missing | edge dropped, `dropped_edges += 1` |
| Kind not in the closed set | dropped, counted (programming error, also a unit test) |
| `node_id` collision between distinct keys | snapshot `failed`, `reason = "id collision"` |
