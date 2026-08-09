# Code graph — status

## Increment 01 — Go engine + seam + tools + gRPC — **DONE**

The `AstBackend` seam ships with the type-aware Go engine, the full `find_*` tool
surface, and the `AstService` gRPC seam.

- **Seam** (`agent-core`): `AstBackend` trait + value types (`Symbol`, `SymbolRef`,
  `SymbolKind`, `AstCallGraph`, `CallPath`, `AstCapabilities`, `AstVerb`);
  capability-gated verb defaults return `Error::Ast`.
- **Helper** (`helpers/go-graph`, `agent-go-graph`): `x/tools` — `go/packages` +
  `go/types.Implements` (implicit interface satisfaction) + `go/ssa` +
  `go/callgraph/cha` (precise typed call edges) + the package import graph. Bounded,
  fail-soft. Pinned by `buildGoModule` (vendored `x/tools`, real `vendorHash`) in
  `nix/default.nix`; on the dev-shell + check PATH.
- **Engine** (`agent-ast`): `GoAst` runs the helper through the `Sandbox`, parses its
  untrusted JSON into a bounded/confined indexed `Graph`, answers every verb;
  `DispatchAst` composes named engines.
- **Tools** (`agent-tools`, `tool-ast-graph`): `find_symbol`, `find_implementations`,
  `find_interface`, `find_callers`, `find_callees`, `find_callchain`,
  `find_changed_callers`, `find_dependency_path`.
- **Wire**: additive `ast.proto` (`AstService`, 11 RPCs) — `buf` passes with no
  baseline bump; `server/ast.rs` + `client/ast.rs` (`GrpcAst`); `Seam::Ast` +
  `--serve-ast` in the CLI SEAMS table; `ast` block in `nix/constants.nix`
  (port 50081 / metrics 9631).
- **Runtime**: `[ast]` config (`backends` / `auto_index` / `helper_timeout_secs`),
  `build_ast` + `spawn_freshness` in the builder, `Agent::with_ast` / `ast()`,
  `agent` feature default-on.
- **Observability**: `metered::ast` decorator → `agent_ast_query_seconds` /
  `agent_ast_result_nodes` / `agent_ast_errors_total` (labels `backend`,`verb`) +
  `ast.<verb>` spans (counts only).
- **Tests / gates**: table-driven `rstest` (all four classes + `adversarial_`) over
  the graph core + fake-sandbox engine tests; hermetic `nix/checks/ast-go.nix`
  (fixture Go module → 2 implicit implementers + a precise call edge);
  `benches/ast.rs` (iai, Ir ceilings; setup excluded so verbs measure ~2.6–3.5M Ir,
  parse ~34.7M) in `nix/checks/bench.nix`; `tests/leak.rs` (dhat) in
  `nix/checks/leak.nix`.

### Implementation notes / deviations

- **Builder-wired, not a `Registry` factory.** The `go` engine needs the shared
  `Sandbox` (not carried by `FactoryCtx`), so `build_ast` is wired in
  `agent-runtime/src/builder.rs` like `bash`, not via `Registry::ast`. The `grpc`
  client is built there too.
- **Bench setup moved out of the measured region.** The first cut measured ~38M Ir
  for every verb because the shared `built()` setup (parse + tempdir) dominated;
  moving it to an iai `setup` hook revealed the true verb cost (~2.6–3.5M) and set
  honest ceilings.
- **`find_references` / `find_definition` intentionally omitted** — served by the LSP
  seam; not duplicated.

## Increment 02 — SCIP substrate — **DONE**

Cross-language symbols / implementations via the SCIP index format.

- **Engine** (`agent-ast`, feature `ast-scip`): `ScipAst` runs a per-language indexer
  (`ScipIndexer::builtin`: `scip-go`, `rust-analyzer scip`, `scip-typescript`,
  `scip-python`) through the `Sandbox`, reads the `.scip` output, and folds it into a
  shared **`SymbolModel`** (`src/model.rs`) via `ingest_scip` — each
  `SymbolInformation` → a symbol (defined in its document, positioned from a
  Definition-role occurrence), each `Relationship { is_implementation }` → an
  implementation edge. Function-local symbols dropped; document paths `confine`d;
  strings bounded; symbol cap. `find_symbol` / `implementations` / `interface_of`
  only — **call-graph verbs stay capability-gated** (route to `go`).
- **Decode**: the `scip` crate (v0.9) + `protobuf` (3.7, scip's codec).
- **Dispatch**: `DispatchAst` fans symbol/implementation lookups across `go` + `scip`
  and merges; `[ast] backends = ["go", "scip"]`, `[ast] scip_langs = ["go", …]`.
- **Runtime feature** `ast-scip` (off by default — heavier deps/slower indexers);
  builder `scip` arm; `metered::ast` labels it `scip`.
- **Tests / gates**: `rstest` ingestion tests over an in-memory `Index` (implements /
  interface-of / def-line / adversarial path-escape / empty) + a real-output
  end-to-end test (`tests/scip_e2e.rs`, self-skips without `scip-go`). Hermetic
  `nix/checks/ast-scip.nix` runs `scip-go` on a fixture Go module and asserts both
  implicit implementers of `Greeter` resolve. `scip-go` pinned in `nix/versions.nix`
  + on the dev-shell PATH.

## Increment 03 — `structural_search` (ast-grep) — **DONE**

Multi-language structural (AST-pattern) search.

- **Tool** (`agent-tools`, feature `tool-structural-search`): `StructuralSearchTool`
  runs `ast-grep run --pattern … --lang … --json` through the `Sandbox` and parses the
  JSON matches into `path:line<TAB>text`. The model-supplied pattern/lang/paths are
  **shell-quoted** (an adversarial test pins that an injection stays inside the arg);
  `lang` is allowlisted; output capped.
- **Runtime feature** `structural-search` (default-on); builder-wired with the shared
  `Sandbox`; `structural_search` in the config tools list. `ast-grep` pinned in
  `nix/versions.nix` + on the dev-shell PATH (needs `ast-grep` on PATH at runtime).
- **Not a seam** — stateless structural grep, like the live `grep` tool.
