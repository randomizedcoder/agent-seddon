# Increment 04 — `rust` engine (deep, native) via charon

The Go engine ([01](STATUS.md)) gave two things SCIP can't: a **precise, typed call
graph** (`callers`/`callees`/`callchain`/`blast_radius`) and implicit-interface
implementations, both from `go/types` + `go/callgraph`. Rust already had the
*symbol/implementation* half through the SCIP substrate (`rust-analyzer scip`,
[02](STATUS.md)) — but **no call graph**, because SCIP carries no call edges. This
increment closes that gap with a deep native Rust engine, giving Rust the same verb
surface Go has, at the same precision.

## Why charon (and why not the alternatives)

Rust has **no first-party whole-program call-graph tool** the way Go has
`x/tools/go/callgraph`. The options split cleanly:

| Source | Gives | Precision | Verdict |
|---|---|---|---|
| **rustdoc JSON** (nightly) | symbols, trait impls, module graph | precise — **no call edges** | half the picture |
| **`syn` syntactic** | call edges by name | best-effort, no dispatch resolution | imprecise |
| **cargo-call-stack / rust-callgraphs / callgraph.rs** | call graph | LLVM/embedded-only, or brittle `rustc_private` pinned to old nightlies | unmaintained for this use |
| **charon** (AeneasVerif) | **type decls + function bodies (resolved call sites) + trait impls**, as JSON | precise, MIR-level, dispatch resolved | **chosen** |

Charon is a maintained `rustc` driver (used by the Aeneas verification toolchain)
that lowers MIR to ULLBC and dumps a crate's `type_decls`, `fun_decls` (with call
sites in the function bodies), `trait_decls`, and `trait_impls` to a single JSON
`.llbc`. One run yields **every** verb — symbols, implementations, the call graph,
and the module/crate dependency graph — exactly as one `agent-go-graph` run does for
Go. Because it works at the MIR level, a trait-method call resolves to the trait
method reference (and, via the trait impl, to the concrete implementation), so the
call graph is typed, not name-matched.

The user explicitly chose this precise path over a lighter rustdoc/`syn` engine,
accepting its cost (below).

## The cost, stated plainly

Charon **compiles the target crate** with its own bundled nightly toolchain to get
MIR. That means:

- The target must **type-check under charon's toolchain** — like the Go engine needs
  the target to type-check under `go/packages`. A crate that doesn't build yields
  whatever charon resolved plus diagnostics, never an abort (**fail-soft**).
- It is **heavier and slower** than SCIP or the Go helper (a full MIR build), and is
  **nightly-locked** — charon's pin, not our stable toolchain. Pinning charon as a
  flake input (frozen in `flake.lock`) contains the churn: the format and toolchain
  move only when we deliberately bump the input.
- Runtime needs **`charon` + a Rust toolchain on PATH** (documented alongside the Go
  engine's "needs the Go toolchain" caveat).

## Shape (mirrors the `go` engine exactly)

```
charon (flake input, pinned)        MIR extractor → crate.llbc (JSON)
  └─ nix/versions.nix `charon`       on the dev-shell + ast-rust check PATH
crates/agent-ast/src/rust.rs        RustAst (feature ast-rust): run charon via
  Sandbox → lower .llbc → Graph      the Sandbox, lower JSON into the shared Graph
crates/agent-ast/src/graph.rs       Graph::parse_value — shared bounded ingestion
  (refactor)                         core, fed by both engines
DispatchAst (lib.rs, refactor)      route_first: call-graph verbs go to the first
                                     capable engine that RESOLVES the target
```

**Reuse, not reinvent.** `RustAst` lowers charon's `.llbc` into the *same*
intermediate schema (`symbols`/`edges`/`implements`/`imports`/`packages`) the Go
helper emits, then feeds it to `Graph::parse_value` — so all the `confine`/bound/cap
containment and every traversal algorithm (BFS callers/callees, DFS callchain,
package-BFS dependency_path) are shared with the Go engine. The charon JSON is parsed
**defensively** as `serde_json::Value` (no `charon_lib` dependency), so a schema we
don't recognise degrades to fewer edges, never a panic.

## Dispatch: two call-graph engines

Before this increment only `go` served the call-graph verbs, so `DispatchAst` routed
them to the first capable backend. With `rust` also serving them, that's wrong — a
Rust symbol would route to `go` and come back empty. The results **can't be merged**
either: symbol ids are per-engine, so concatenating two graphs would alias unrelated
nodes. The fix (`route_first`) tries every capable engine in config order and returns
the **first non-empty** result: a Rust symbol doesn't exist in the Go engine (empty
graph) and vice versa, so the owning engine wins without any id merging. Ambiguous
same-named symbols across a mixed Go+Rust tree resolve to the first configured engine;
the gRPC per-request selector overrides when that matters.

`find_symbol` / `implementations` / `interface_of` still **fan and merge** across all
engines (dedup by package+receiver+name+kind) — those results are id-independent.

## Config

```toml
[ast]
backends = ["go", "rust", "scip"]   # order = default + call-graph routing priority
```

`rust` is gated by the runtime `ast-rust` feature (off by default — heavy toolchain).

## Gates (full seam treatment, same as `go`)

- **Tests** — table-driven `rstest` over a captured charon `.llbc` fixture
  (`positive_` impls + call edge, `negative_` unknown/empty, `boundary_` hops,
  `corner_` self-recursion/cycle, **`adversarial_`** garbage JSON / path-escape /
  hostile counts), plus fake-`Sandbox` engine tests for the fail-soft branches and a
  real-charon end-to-end test that self-skips when `charon` is absent (like
  `scip_e2e.rs`).
- **Bench** — `benches/ast.rs` gains a Rust ingest + verb bench with an Ir ceiling in
  `nix/checks/bench.nix`.
- **Leak** — `tests/leak.rs` exercises the Rust ingest + query path under dhat.
- **Nix check** — `nix/checks/ast-rust.nix`, hermetic: a tiny Rust crate with a trait
  + two impls + a caller→callee, `charon` on PATH, asserting the implementations and
  the call edge resolve. Offline.
- **Metrics/tracing** — none new: the `metered::ast` decorator is engine-generic and
  labels this backend `rust` automatically.

See [STATUS.md](STATUS.md) for shipped state.
