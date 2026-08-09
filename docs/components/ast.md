# AST / code graph — the `AstBackend` seam

Type-aware, whole-repo **structural** code intelligence. Where [search](search.md)
retrieves *text* and [lsp](lsp.md) answers *position* queries, `AstBackend` answers
questions about *shape*: who calls a function, what it reaches, which concrete types
satisfy an interface (implicit in Go), and the package path between two components.

- **Trait**: `agent_core::AstBackend` (`crates/agent-core/src/lib.rs`)
- **Impl crate**: `agent-ast` (`crates/agent-ast/`)
- **Engines**: `go` — `GoAst`, feature `ast-go` (precise, via the pinned
  `agent-go-graph` helper); `scip` — `ScipAst`, feature `ast-scip` (cross-language
  symbols/implementations via `.scip` indexes)
- **Runtime features**: `ast` (Go engine + tools, default), `ast-scip` (SCIP engine,
  opt-in), `structural-search` (the `ast-grep` tool, default)
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

## The SCIP engine (`ScipAst`, breadth to many languages)

`ScipAst` (feature `ast-scip`, opt-in) serves **symbols / implementations across
languages** through the SCIP index format. It runs a per-language indexer via the
`Sandbox` — `scip-go`, `rust-analyzer scip`, `scip-typescript`, `scip-python`
(`ScipIndexer::builtin`) — reads the produced `.scip` protobuf, and folds it into the
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
`backend` = `go`/`grpc`):

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
   routes call-graph verbs to the first engine that serves them).
3. Add the backend name to `[ast] backends`.

See [extending.md](../extending.md).

## Testing

- Pure graph algorithms are table-driven `rstest` cases in
  `crates/agent-ast/src/graph.rs` (`positive_`/`negative_`/`boundary_`/`corner_` +
  mandatory `adversarial_`: path escape, hostile counts, cyclic termination, `hops`
  clamp). Engine fail-soft branches use a fake `Sandbox` in `go.rs`.
- SCIP ingestion is table-driven `rstest` over an in-memory `Index`
  (`src/model.rs`), plus a real-output end-to-end test (`tests/scip_e2e.rs`,
  self-skips without `scip-go`).
- `structural_search` has fake-`Sandbox` tests incl. `adversarial_` shell-injection
  quoting.
- End-to-end the pinned helpers are gated by hermetic checks: `nix/checks/ast-go.nix`
  (Go engine — interface, two implicit implementers, a call chain) and
  `nix/checks/ast-scip.nix` (`scip-go` → ingestion → implementation query); both
  offline.
- `benches/ast.rs` (iai-callgrind, Ir ceilings in `nix/checks/bench.nix`) and
  `tests/leak.rs` (dhat, in `nix/checks/leak.nix`) gate the parse + query hot paths.
