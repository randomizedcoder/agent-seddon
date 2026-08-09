# Code graph — the `AstBackend` seam

Type-aware, whole-repo structural code intelligence as a first-class seam, so the
model (and the review flow) can ask about *shape* — callers, callees, call chains,
interface implementations, package dependency paths — as **fact**, deterministically,
instead of inferring architecture from snippets and getting it wrong.

This track builds out the `AstBackend` seam, `AstService` / `--serve-ast`, and the
`find_*` tools. It **supersedes the *Deferred* section** of
[`../code-review/ast-callgraph.md`](../code-review/ast-callgraph.md) (the dedicated
seam, CHA/RTA edges, interface/implementation edges). The stdlib-only
`agent-go-ast` + `CallGraphCollector` in the review track stay as-is — they are
syntactic and run on non-compiling trees, which the precise engine here cannot.

## Why a new seam (not fold into search)

A `SearchBackend` returns text hits (`path:line`, snippet, score). Code-graph queries
return **nodes + edges** — a different response shape. Folding them into
`SearchService` would distort that seam's contract. The graph is its own seam, one
trait, dispatched over named engines exactly like `[search] backends = [...]`.

`find_references` / `find_definition` are **not** re-implemented here — the
[LSP seam](../../components/lsp.md) (gopls) already serves them per-position. This
track owns what LSP can't do whole-repo: the call graph, implicit-interface
implementations, blast radius, package paths, and a fast index-backed `find_symbol`.

## Engine strategy (phased, multi-engine)

The seam is **language-neutral**; engines are selected by config string:

- **Increment 1 — `go` (deep, native):** `helpers/go-graph` (`agent-go-graph`) uses
  `golang.org/x/tools` — `go/packages` + `go/types.Implements` (implicit interface
  satisfaction) + `go/ssa` + `go/callgraph/cha` (precise typed call edges). Needs the
  target to type-check + the Go toolchain at runtime; fail-soft.
- **Increment 2 — `scip` (breadth):** a SCIP substrate ingesting the output of
  `scip-go` / `scip-typescript` / `scip-python` / `scip-clang` / `rust-analyzer scip`
  → symbols / def / ref / implementations across ~5 languages through **one**
  ingestion path (no per-language Rust). No call edges in SCIP → the call-graph verbs
  route to `go`.
- **Increment 3 — `structural_search` (orthogonal):** an `ast-grep`-backed tool for
  multi-language structural pattern search (`$X.Close()`), via the Sandbox. Not a full
  seam.
- **Increment 4 — `rust` (deep, native):** the precise Rust analogue of `go` — the
  pinned `charon` MIR extractor → a typed, dispatch-resolved Rust call graph + trait
  implementations, lowered into the same shared `Graph`. Needs the target to build
  under charon's toolchain + `charon` at runtime; fail-soft. See
  [`04-rust-native.md`](04-rust-native.md).

## Design specifics

- **Trait / proto / verbs / metrics / security**: see the component doc,
  [`../../components/ast.md`](../../components/ast.md).
- **Dispatch**: `DispatchAst` mirrors `DispatchSearch` — first engine is the default,
  `resolve(name)` for per-request routing, symbol/implementation lookups fan out and
  merge, call-graph verbs route to the first capable engine.
- **Wire**: additive `ast.proto` (`AstService`) — a new service, so `buf breaking`
  passes with **no baseline bump**. New `ast` seam in `nix/constants.nix`
  (port 50081).

See [STATUS.md](STATUS.md) for what has shipped.
