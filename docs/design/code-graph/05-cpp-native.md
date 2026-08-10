# Increment 05 — C/C++ engine (syntactic tree-sitter + precise scip-clang)

C/C++ gets the same `find_*` verb surface as Go ([01](STATUS.md)) and Rust
([04](STATUS.md)) — call graph, symbols, type/class hierarchy, `#include` dependency
graph — but split across **two layers**, because the defining C/C++ constraint is the
**compilation database**.

## Why two layers

Precise C/C++ analysis (scip-clang, any libclang tool) needs a `compile_commands.json`
(CMake/Bazel/Meson/`bear`), which many repos lack and the agent can't synthesize. So
unlike Go/Rust — where the precise engine is the default — C/C++ layers precision:

| Layer | Tool | Gives | Needs |
|---|---|---|---|
| **`cpp` engine** (always on) | in-crate `tree-sitter-c`/`tree-sitter-cpp` | call graph (syntactic), symbols, class inheritance, `#include` graph | nothing — parses any tree |
| **`scip-clang`** (when a compile DB exists) | pinned prebuilt `scip-clang` | precise symbols + C++ inheritance/override implementations | `compile_commands.json` + `clang` on PATH |

## Why tree-sitter in-crate (not semcode, not a clang helper)

The user suggested `semcode` (Meta experimental). It's tree-sitter under the hood, but
a poor fit as a bulk-ingest engine: **no bulk call-graph JSON export** (only interactive
`callers`/`calls`/`callchain`), experimental/unreleased, unpackaged (cargo-build with
libclang+protobuf), LanceDB-stateful. So we do the *same tree-sitter parsing* it does,
but **in-crate** (pinned grammar crates) — hermetic, no external process, tailored to
our `Graph`. A precise clang-based call-graph helper (libclang) was rejected: it needs a
compile DB *and* is Clang-version-pinned and heavy to package.

## The `cpp` engine (`crates/agent-ast/src/cpp.rs`)

Mirrors the other engines but **reads the tree in-process** (no `Sandbox`): `walkdir`
enumerates C/C++ files (capped, `confine`d), each is parsed with the matching grammar,
and a **manual recursive tree walk** (not the QueryCursor streaming API — version-stable)
extracts functions/calls/types/inheritance/includes into the shared `Graph` via
`Graph::parse_value` (the same schema the Go helper emits). Name resolution links calls
to every definition of that name (capped `MAX_CALLEES_PER_NAME`); inheritance derived→base
becomes an `implements` edge; `#include`s resolve to repo files by basename suffix for the
dependency graph.

**Honest limitation** (pinned by a `corner_` test + documented in `ast.md`): the call
graph is **syntactic/name-resolved** — macros opaque, C++ overloads / virtual dispatch /
templates / function pointers unresolved. `find_symbol`, inheritance, and includes are
reliable.

## The `scip-clang` layer

A new `"cpp"|"c"|"c++"` arm in `ScipIndexer::builtin` runs `scip-clang --compdb-path
compile_commands.json --index-output-path index.scip.cpp`. **No ingestion changes** —
`kind_from_scip` already folds C++ `Class/Struct/Method`, and `ingest_scip` keys
implementation edges purely off SCIP `Relationship { is_implementation }` (language-
agnostic), so C++ inheritance/overrides answer `find_implementations`/`interface_of` for
free. Pinned as a **prebuilt Linux binary** (Sourcegraph v0.4.0 → `fetchurl` +
`autoPatchelfHook`; NOT a Clang-21 source build). Fail-soft: no compile DB ⇒ skipped.

## Dispatch

`DispatchAst` needs no change: it fans symbol/implementation lookups across `cpp` +
`scip` and merges (precise scip-clang symbols + tree-sitter symbols), and `route_first`
routes the call-graph verbs to `cpp` (the only C/C++ engine that serves them).

## Gates (full seam treatment)

- **Tests**: table-driven `rstest` in `src/cpp.rs` over checked-in C/C++ source
  fixtures (in-crate — no external tool) — static call edges, callchain, C++
  inheritance→implementations, `#include` dependency path, `negative_`/`boundary_`/
  `corner_` (recursion/cycle, name-resolved method calls) + `adversarial_` (binary /
  oversized files skipped, path confinement, bounded fan-out).
- **Bench**: `benches/cpp_ingest.rs` (feature `ast-cpp`) — parse+lower a 50-file C
  corpus; Ir ceiling 54M (measured ~44M) in `nix/checks/bench.nix`.
- **Leak**: a `cpp` ingest scenario in `tests/leak.rs` (dhat), run under
  `--features dhat-heap,ast-cpp`.
- **Nix checks**: `ast-cpp.nix` (craneCheck running the in-crate test suite, fully
  offline) + `ast-scip-cpp.nix` (hermetic — prebuilt `scip-clang` over a self-contained
  C++ fixture + a hand-written `compile_commands.json` + `clang` on PATH, offline;
  asserts a non-empty index carrying the fixture symbols).
- **Metrics/OTel**: none new — `metered::ast` labels the engine `cpp` automatically.
- **No proto/gRPC/CLI change** — the seam is language-neutral.

See [STATUS.md](STATUS.md).
