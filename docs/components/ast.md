# AST / code graph — the `AstBackend` seam

Type-aware, whole-repo **structural** code intelligence. Where [search](search.md)
retrieves *text* and [lsp](lsp.md) answers *position* queries, `AstBackend` answers
questions about *shape*: who calls a function, what it reaches, which concrete types
satisfy an interface (implicit in Go), and the package path between two components.

- **Trait**: `agent_core::AstBackend` (`crates/agent-core/src/lib.rs`)
- **Impl crate**: `agent-ast` (`crates/agent-ast/`)
- **Engines**: `go` — `GoAst`, feature `ast-go` (precise, via the pinned
  `agent-go-graph` helper); `rust` — `RustAst`, feature `ast-rust` (precise, via the
  pinned `charon` MIR extractor); `cpp` — `CppAst`, feature `ast-cpp` (syntactic C/C++,
  in-crate tree-sitter); `scip` — `ScipAst`, feature `ast-scip` (cross-language
  symbols/implementations via `.scip` indexes, incl. `scip-clang` for precise C/C++)
- **Runtime features**: `ast` (Go engine + tools, default), `ast-rust` (Rust engine,
  opt-in), `ast-cpp` (C/C++ engine, opt-in), `ast-scip` (SCIP engine, opt-in),
  `structural-search` (the `ast-grep` tool, default)
- **Tools**: `find_symbol`, `find_implementations`, `find_interface`, `find_callers`,
  `find_callees`, `find_callchain`, `find_changed_callers`, `find_dependency_path`,
  and `structural_search` (ast-grep, orthogonal)
- **gRPC service**: `agent.v1.AstService` — `agent --serve-ast`
  (port `50081` / socket `/tmp/agent-seddon/ast.sock` / metrics `9631`, from
  `nix/constants.nix`)
- **Config**: `[ast] backends / auto_index / helper_timeout_secs`; per-seam
  `[grpc.ast] listen / endpoint`

## The trait

```rust
#[async_trait]
pub trait AstBackend: Send + Sync {
    fn capabilities(&self) -> AstCapabilities;          // verbs + languages served
    async fn status(&self) -> Result<IndexStatus>;      // cheap freshness probe
    async fn reindex(&self, progress: ProgressFn<'_>) -> Result<IndexStatus>;

    async fn find_symbol(&self, q: &SymbolQuery) -> Result<Vec<Symbol>>;
    async fn implementations(&self, iface: &SymbolRef) -> Result<Vec<Symbol>>;
    async fn interface_of(&self, ty: &SymbolRef) -> Result<Vec<Symbol>>;
    async fn callers(&self, target: &SymbolRef, hops: u32) -> Result<AstCallGraph>;
    async fn callees(&self, target: &SymbolRef, hops: u32) -> Result<AstCallGraph>;
    async fn callchain(&self, from: &SymbolRef, to: &SymbolRef, max_paths: u32)
        -> Result<Vec<CallPath>>;
    async fn blast_radius(&self, changed: &[String], hops: u32) -> Result<AstCallGraph>;
    async fn dependency_path(&self, from_pkg: &str, to_pkg: &str) -> Result<Vec<String>>;
}
```

Every verb but `find_symbol` has a **capability-gated default** returning `Error::Ast`
(mirroring `SearchBackend::list_files`), so an engine implements only what it can — a
future SCIP substrate serves symbols/implementations across many languages but not the
call-graph verbs. The dispatcher consults `capabilities()` to route.

## The Go engine (`GoAst`)

`GoAst` runs the pinned **`agent-go-graph`** helper (`helpers/go-graph/`) through the
`Sandbox` seam, parses its JSON into a bounded in-memory graph, and answers the verbs
over it. The helper uses `golang.org/x/tools`:

- `go/packages` + `go/types` → **implicit interface satisfaction**
  (`types.Implements`) — the query nothing else answers cheaply. In Go a type
  satisfies an interface with no `implements` keyword; `find_implementations` finds
  every implementer (value *and* pointer receiver).
- `go/ssa` + `go/callgraph/cha` → **precise, type-resolved call edges** (an interface
  method call resolves to every concrete implementation), for
  `find_callers`/`callees`/`callchain` and the `find_changed_callers` blast radius.
- the package import graph → `find_dependency_path`.

Unlike the stdlib-only `agent-go-ast` used by the review flow (syntactic, runs on any
tree), `agent-go-graph` loads full type information, so it **needs the target to
type-check** and the **Go toolchain present at runtime** (it shells out via
`go/packages`). It is **fail-soft**: a package that doesn't type-check contributes
whatever resolved plus a diagnostic; a missing helper / timeout / unparseable output
surfaces as `Error::Ast`, never a panic. The graph is built lazily on first query and
cached; `reindex` rebuilds it, and `[ast] auto_index` warms it in the background on
start.

## The Rust engine (`RustAst`)

`RustAst` (feature `ast-rust`, opt-in) is the precise Rust analogue of `GoAst`. It
runs the pinned **`charon`** MIR extractor (`charon cargo --ullbc
--no-dedup-serialized-ast`) through the `Sandbox`, then **lowers** charon's `.llbc`
JSON into the *same* bounded `Graph` the Go helper feeds — so all the traversal verbs
(`callers`/`callees`/`callchain`/`blast_radius`/`dependency_path`) and the
`confine`/bound/cap containment are shared, not reimplemented (`Graph::parse_value`).

Rust has no first-party whole-program call-graph tool (unlike Go's `x/tools`), so this
is built on charon (the AeneasVerif verification project's `rustc` driver), which
dumps a crate's `type_decls`, `fun_decls` (function bodies with resolved call sites),
`trait_decls`, and `trait_impls` to a single JSON. One run yields every verb:

- `trait_impls` → **implementations** / **interface_of** (`impl_trait.id` = the trait,
  the inlined Self type's `TypeDeclId` = the implementing type — including generic and
  blanket impls).
- a `Call` terminator's `func.Regular.kind.Fun.Regular` → a **precise static call
  edge** (the compiler already resolved dispatch).
- a generic trait-bound call (`func.Regular.kind.Trait = [trait_ref, idx]`) → resolved
  **CHA-style** to every implementer's method via the `trait_impls` method table.

Because charon works at the MIR level, the call graph is typed, not name-matched.
charon's JSON is parsed **defensively** as `serde_json::Value` (no `charon_lib`
dependency), so a schema we don't recognise degrades to fewer edges, never a panic.

**The one honest gap.** A pure `dyn Trait` call is rendered by charon as a vtable
projection (`func.Dynamic`) with no cheap trait id, so its edge is **not** resolved —
static and generic (monomorphizable) trait dispatch is; `dyn`-only call chains are the
documented limitation (a covered corner test pins this).

**Cost, like `GoAst`'s "needs the Go toolchain".** charon does a full **MIR build** of
the target with its own bundled nightly toolchain: the target must type-check under it
(else a partial graph + diagnostics — **fail-soft**), it is heavier/slower than SCIP or
the Go helper, and it needs **`charon` on PATH at runtime**. The charon pin is a flake
input frozen in `flake.lock`, so its nightly + wire format move only on a deliberate
bump. Enable with `[ast] backends = ["rust", …]` and raise `[ast] helper_timeout_secs`.

## The C/C++ engine (`CppAst`)

`CppAst` (feature `ast-cpp`, opt-in) is the odd one out: it parses the tree
**in-process** with the pinned `tree-sitter-c` / `tree-sitter-cpp` grammars — **no
external tool, no build, no `compile_commands.json`** — and lowers into the same shared
`Graph`. It walks the source tree (`walkdir`, capped, `confine`d), and per file extracts:

- `function_definition` → **function / method symbols** (`static` ⇒ not exported;
  method receiver = the enclosing `class`/`struct`).
- `call_expression` → **call edges** (caller = enclosing function; callee resolved by
  name), for `find_callers`/`callees`/`callchain`/`find_changed_callers`.
- `class_specifier` base clauses → **inheritance** → `implements` edges, so
  `find_implementations(Base)` returns subclasses and `find_interface(Derived)` the bases.
- `preproc_include` → the **`#include` graph** for `find_dependency_path`.

**The honest limitation.** Because it is purely syntactic, the call graph is
**name-resolved**: a call to `foo` links to *every* function named `foo` (capped),
macros are opaque, and C++ overloads / virtual dispatch / templates / function pointers
are not resolved. `find_symbol`, class inheritance, and the `#include` graph are
reliable; the *precise* C/C++ symbols/implementations come from the `scip-clang` layer
below (where a compile DB exists). This is the same tree-sitter parsing tools like
`semcode` do, but in-crate — no external process at runtime. Fail-soft: an
unreadable/oversized/non-UTF-8 file is skipped, never a panic. Enable with
`[ast] backends = ["cpp", …]`.

**Precise C/C++ via `scip-clang`.** Add `"cpp"` to `[ast] scip_langs` (with the `scip`
backend + `ast-scip` build) to run the pinned **`scip-clang`** — a precise Clang-based
SCIP indexer — over a `compile_commands.json`. Its symbols + C++ inheritance/override
`is_implementation` relations flow into `find_implementations`/`interface_of` through
the SCIP substrate unchanged, and `DispatchAst` merges them with the tree-sitter
symbols. `scip-clang` **needs a `compile_commands.json`** (CMake/Bazel/Meson/`bear`) and
`clang` on PATH (for its resource dir); absent a compile DB it fail-soft-skips, leaving
the tree-sitter `cpp` engine as the always-available layer.

## The SCIP engine (`ScipAst`, breadth to many languages)

`ScipAst` (feature `ast-scip`, opt-in) serves **symbols / implementations across
languages** through the SCIP index format. It runs a per-language indexer via the
`Sandbox` — `scip-go`, `rust-analyzer scip`, `scip-clang` (C/C++), `scip-typescript`,
`scip-python` (`ScipIndexer::builtin`) — reads the produced `.scip` protobuf, folds it into the
shared `SymbolModel`: each `SymbolInformation` becomes a symbol; each SCIP
`Relationship { is_implementation }` becomes an implementation edge (which is how it
answers `find_implementations` for interfaces, even implicit ones, in any indexed
language). SCIP carries **no call edges**, so `callers`/`callees`/… stay gated and
route to `go`. Configure with `[ast] backends = ["go", "scip"]` and
`[ast] scip_langs = ["go", "rust", …]`. Fail-soft: a missing/failed indexer or
unreadable output contributes nothing.

## `structural_search` (ast-grep)

An orthogonal tool (feature `structural-search`, default-on) — **not** a seam. It runs
`ast-grep` through the `Sandbox` to match a tree-sitter pattern (`$X.Close()`,
`if err != nil { $$$ }`) across Go/Rust/TS/Python/…, returning `path:line<TAB>match`.
The model-supplied `pattern`/`lang`/`paths` are **shell-quoted** (an injection stays
inside the argument), `lang` is allowlisted, output is capped. Needs `ast-grep` on
PATH at runtime.

## Security (the model is untrusted)

- The helper runs in the `Sandbox`; its JSON is untrusted → parsed defensively with
  **caps** on every list (symbols/edges/implements/imports), **bounded** strings, and
  file paths run through `confine()` — a symbol whose path escapes the repo is dropped
  with its edges/relations.
- Model-supplied symbol/package names are **matched**, never interpolated into the
  shell (the helper command is static, `agent-go-graph --root .`).
- Traversal `hops` and `max_paths` are **clamped** (`MAX_HOPS = 8`) so a hostile repo
  can't induce a whole-graph walk; results are capped (`MAX_RESULT`).

## Metrics & tracing

Recorded by the `metered::ast` decorator (one per configured engine, labelled
`backend` = `go`/`rust`/`cpp`/`scip`/`grpc`):

| Metric | Type | Labels |
|---|---|---|
| `agent_ast_query_seconds` | histogram | `backend`, `verb` |
| `agent_ast_result_nodes` | histogram | `backend`, `verb` |
| `agent_ast_errors_total` | counter | `backend`, `verb` |

Each verb emits an `ast.<verb>` span carrying `backend`, `hops`/`n_changed` where
relevant, and the result `nodes` count — **counts only, never source or signatures**.
gRPC handlers parent their span on the caller's W3C trace context.

## Running as a distributed service

```sh
agent --serve-ast                 # host AstService on the generated port/socket
grpcurl -plaintext 127.0.0.1:50081 list agent.v1.AstService
```

A `[ast] backends = ["grpc"]` client (`GrpcAst`) dials it — the same trait served by
`AstServiceSvc` and consumed by `GrpcAst`, so the engine can run on one host and the
loop on another. See [grpc.md](../grpc.md).

## Adding your own engine

1. Implement `AstBackend` in a sibling crate (or in `agent-ast` behind a feature).
   Implement only the verbs you can serve; leave the rest to the capability-gated
   defaults and advertise the served set in `capabilities()`.
2. Wire it in `agent-runtime/src/ast.rs::build_ast` (it composes named engines into
   one `DispatchAst`, which fans symbol/implementation lookups across engines and
   routes each call-graph verb to the first engine that actually **resolves the
   target** — a Rust symbol isn't in the Go engine and vice versa, and per-engine
   symbol ids mean the graphs can't be merged, so first-non-empty wins).
3. Add the backend name to `[ast] backends`.

See [extending.md](../extending.md).

## Testing

- Pure graph algorithms are table-driven `rstest` cases in
  `crates/agent-ast/src/graph.rs` (`positive_`/`negative_`/`boundary_`/`corner_` +
  mandatory `adversarial_`: path escape, hostile counts, cyclic termination, `hops`
  clamp). Engine fail-soft branches use a fake `Sandbox` in `go.rs`.
- Rust `.llbc` lowering is table-driven `rstest` in `src/rust.rs` over a **real**
  checked-in charon fixture (`tests/fixtures/greeter.ullbc.json`): implementations,
  static + CHA call edges, receiver disambiguation, the `dyn`-dispatch gap
  (`corner_`), plus `adversarial_` (garbage JSON, path-escape drop, hostile decls) and
  fake-`Sandbox` engine fail-soft (127 / timeout / nonzero-with-index).
- SCIP ingestion is table-driven `rstest` over an in-memory `Index`
  (`src/model.rs`), plus a real-output end-to-end test (`tests/scip_e2e.rs`,
  self-skips without `scip-go`).
- `structural_search` has fake-`Sandbox` tests incl. `adversarial_` shell-injection
  quoting.
- End-to-end the pinned helpers are gated by hermetic checks: `nix/checks/ast-go.nix`
  (Go engine — interface, two implicit implementers, a call chain),
  `nix/checks/ast-rust.nix` (Rust engine — `charon` on a fixture crate → 2 trait impls
  + a precise static call edge), and `nix/checks/ast-scip.nix` (`scip-go` → ingestion
  → implementation query); all offline.
- `benches/ast.rs` (parse + query verbs) and `benches/rust_ingest.rs` (the charon
  `.llbc` lowering, feature `ast-rust`) — iai-callgrind, Ir ceilings in
  `nix/checks/bench.nix`; `tests/leak.rs` (dhat, in `nix/checks/leak.nix`) gates the
  parse/ingest/query hot paths (incl. the Rust ingest under `ast-rust`).
