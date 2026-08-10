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

## Increment 04 — `rust` engine (deep, native) via charon — **DONE**

The precise Rust analogue of the Go engine — a typed, dispatch-resolved Rust call
graph + trait implementations. See [`04-rust-native.md`](04-rust-native.md).

- **Extractor**: `charon` (AeneasVerif MIR extractor, v0.1.232) pinned as a **flake
  input** (`flake.nix` → `charonPkg` → `nix/versions.nix` `charon`; not in nixpkgs).
  Bundles its own nightly toolchain + full-MIR sysroot, so it builds a crate fully
  offline. On the dev-shell + `ast-rust` check PATH.
- **Engine** (`agent-ast`, feature `ast-rust`): `RustAst` runs `charon cargo --ullbc
  --no-dedup-serialized-ast` through the `Sandbox`, then **lowers** the `.llbc` JSON
  (`src/rust.rs`) into the *shared* `Graph` (`Graph::parse_value`, factored out of the
  Go path) — so all traversal + containment is reused. Extracts symbols, trait
  implementations (`trait_impls` → Self `TypeDeclId` + `TraitDeclId`), precise static
  call edges (`func.Regular.kind.Fun.Regular`), and generic trait-bound calls resolved
  CHA-style (`func.Regular.kind.Trait`). Parses charon JSON defensively as
  `serde_json::Value` (no `charon_lib` dep). Fail-soft; every verb served.
- **Dispatch**: `DispatchAst` gained `route_first` — call-graph verbs now route to the
  first engine that **resolves the target** (not merely serves the verb), since two
  engines (`go` + `rust`) serve them and per-engine ids can't be merged.
- **Runtime feature** `ast-rust` (off by default — heavy: full MIR build + Rust
  toolchain at runtime); `build_ast` `rust` arm (generous charon timeout); `metered::ast`
  labels it `rust`. `[ast] backends = ["rust", …]`.
- **Tests / gates**: table-driven `rstest` over a **real** checked-in charon fixture
  (`tests/fixtures/greeter.ullbc.json`) — implementations, static + CHA edges, receiver
  disambiguation, the documented `dyn`-dispatch gap (`corner_`), `adversarial_`
  (garbage/path-escape/hostile), and fake-`Sandbox` fail-soft. Hermetic
  `nix/checks/ast-rust.nix` (real `charon` on a fixture crate, offline).
  `benches/rust_ingest.rs` (iai, Ir ceiling 27M; lowering measured ~21.9M) +
  `tests/leak.rs` Rust-ingest scenario (dhat).

### Implementation notes / deviations

- **`--no-dedup-serialized-ast` is load-bearing.** Without it charon serializes types
  as hashcons `Deduplicated` indices with no in-file table, so a trait impl's Self type
  can't be resolved. The flag inlines types (`Adt.id.Adt` = `TypeDeclId`).
- **`dyn Trait` calls are the one gap.** charon renders them as `func.Dynamic` vtable
  projections with no cheap trait id; static + generic trait dispatch resolve, pure
  `dyn` chains don't. Pinned by a `corner_` test + documented in `ast.md`.
- **No proto/CLI/gRPC changes** — the seam is language-neutral; a new engine needs no
  wire change (`GrpcAst` / `AstService` serve it unchanged).
- **No new metrics** — the `metered::ast` decorator is engine-generic (backend label).

## Increment 05 — C/C++ engine (syntactic tree-sitter + precise scip-clang) — **DONE**

C/C++ gets the full verb surface, split across two layers because precise C/C++ needs a
`compile_commands.json` many repos lack. See [`05-cpp-native.md`](05-cpp-native.md).

- **`cpp` engine** (`agent-ast`, feature `ast-cpp`): `CppAst` parses the tree
  **in-process** with the pinned `tree-sitter-c`/`tree-sitter-cpp` grammars — no
  external tool, no build, no compile DB — via a **manual recursive tree walk**
  (version-stable, not the QueryCursor streaming API). Extracts function/method symbols,
  name-resolved call edges, C++ class inheritance → `implements`, and the `#include`
  graph, lowering into the **shared `Graph`** (`Graph::parse_value`). Reads files
  directly (no `Sandbox`); `walkdir`-enumerated, capped, `confine`d, fail-soft. The call
  graph is **syntactic** (macros/overloads/virtual-dispatch/fn-pointers unresolved) —
  the documented limitation, pinned by a `corner_` test.
- **`scip-clang` layer** (existing `scip` engine): a `"cpp"|"c"|"c++"` arm in
  `ScipIndexer::builtin` runs the pinned prebuilt `scip-clang` over a
  `compile_commands.json` → precise symbols + C++ inheritance/override `is_implementation`
  relations flow into `find_implementations`/`interface_of` with **no ingestion change**
  (language-agnostic). Pinned as a prebuilt Linux binary (v0.4.0, `fetchurl` +
  `autoPatchelfHook`; needs `clang` on PATH). Fail-soft when no compile DB is present.
- **Dispatch**: no change — `DispatchAst` fans symbols across `cpp` + `scip` and merges;
  `route_first` routes call-graph verbs to `cpp`.
- **Runtime feature** `ast-cpp` (off by default); `build_ast` `cpp` arm (no `Sandbox`);
  `metered::ast` labels it `cpp`. `[ast] backends = ["cpp", …]`,
  `scip_langs = ["cpp"]`.
- **Tests / gates**: table-driven `rstest` over C/C++ fixtures (all four classes +
  `adversarial_`); hermetic `nix/checks/ast-cpp.nix` (in-crate test suite) +
  `ast-scip-cpp.nix` (real `scip-clang` on a C++ fixture, offline). `benches/cpp_ingest.rs`
  (iai, Ir ceiling 54M; parse+lower measured ~44M) + `tests/leak.rs` cpp scenario (dhat).

### Implementation notes / deviations

- **Manual tree walk, not tree-sitter `Query`.** tree-sitter 0.25's `QueryCursor` uses
  a streaming-iterator API; a plain recursive descent over `Node` (depth-capped) is
  version-stable and gives natural enter/leave scoping for the caller/receiver context.
- **`semcode` rejected as an engine** (the user's suggestion): no bulk call-graph JSON
  export, experimental/unreleased, unpackaged, LanceDB-stateful. In-crate tree-sitter
  does the same parsing without the baggage.
- **`scip-clang` is a prebuilt-binary pin** (not a source build): its release binary is
  glibc-dynamic, so `autoPatchelfHook` fixes the interpreter/rpath in the store.
