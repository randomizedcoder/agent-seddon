# 06 — AST & call-graph

Status: **design / pre-implementation.**

Go ships first-class AST and call-graph tooling in its standard library and
`golang.org/x/tools`. Extracting the **structure** — packages, types, functions,
and *what calls what* — and summarizing it gives the model the code's hierarchy and
patterns *as fact*, so it doesn't have to infer the architecture from snippets (and
get it wrong). Especially powerful for a reviewer trying to understand the blast
radius of a change.

## Motivation

A diff shows *lines*. A reviewer needs *shape*: which function was changed, who
calls it, what it calls, which package it lives in, whether it's exported. Asking
the model to reconstruct that from a diff is exactly the hallucination risk this
whole design avoids. The Go AST gives it precisely, deterministically, cheaply.

## What it produces

Scoped to the changed set (with a one-hop neighborhood so callers/callees of
changed functions are included):

```rust
pub struct CallGraph {
    pub nodes: Vec<FnNode>,       // id, package, name, exported, file, line, signature_hash
    pub edges: Vec<CallEdge>,     // caller_id -> callee_id
    pub changed_fns: Vec<u32>,    // node ids that the diff touched
}
pub struct PackageShape {         // coarse hierarchy summary
    pub package: String, pub files: u32, pub exported_fns: u32, pub types: u32,
}
```

Two grounded artifacts: the **call graph** (who calls the changed functions, and
what they call — the review's blast-radius map) and a **package shape** summary
(the hierarchy at a glance). Both feed the context; the call graph also lets 08
pick *which* functions are worth a prose summary (the changed ones and their
direct callers).

## Design

A **small Go helper program**, pinned in the flake, invoked by an `AstService`
through the `Sandbox` seam (same reproducible-execution rationale as 05). The
helper uses `go/parser` + `go/ast` (and `golang.org/x/tools/go/callgraph` with
`cha`/`rta` for the edges) and emits **compact JSON** the Rust side parses into the
typed messages. Rust does not parse Go — the language's own tooling does, and the
seam boundary is the JSON.

Scoping keeps it cheap and bounded: build the graph for the packages touched by
the diff, plus one hop of callers/callees, not the whole repo. `signature_hash`
(fnv1a) lets 08 detect a signature change (before/after) without shipping full
signatures around.

Language-extensible the same way as 05: `AstBackend` is a seam; `GoAst` first, a
`RustAst` (via `syn`/`rust-analyzer`) is a later slot-in gated on `RepoLanguage`.

## Failure semantic

**Fail-soft.** A repo that doesn't parse/compile yields a partial graph (whatever
parsed) or `Skipped` with the reason; the bundle is assembled without it. The call
graph is an *enrichment*, never a gate.

## Protobuf

```proto
message FnNode {
  uint32 id        = 1;
  string package   = 2;
  string name      = 3;
  bool   exported  = 4;
  string file      = 5;          // confined
  uint32 line      = 6;
  string signature_hash = 7;     // fnv1a — not the raw signature
}
message CallEdge { uint32 caller_id = 1; uint32 callee_id = 2; }
message PackageShape { string package = 1; uint32 files = 2; uint32 exported_fns = 3; uint32 types = 4; }

message CallGraph {
  repeated FnNode nodes = 1;
  repeated CallEdge edges = 2;
  repeated uint32 changed_fns = 3;
  repeated PackageShape packages = 4;
  uint32 total_ms = 5;
}
```

## gRPC interface

```proto
service AstService {
  rpc Graph (AstRequest) returns (CallGraph);
}
message AstRequest { repeated string changed = 1; uint32 hops = 2; }   // hops clamped
```

`--serve-ast`, new `ast` block in `nix/constants.nix`. Executes the Go helper →
same **serving warning** as 05 (socket permissions are the control). Wire failure
semantic: **fail-soft**.

## Prometheus metrics

| Metric | Type | Labels |
|---|---|---|
| `agent_review_ast_duration_seconds` | histogram | `outcome` |
| `agent_review_ast_nodes` | histogram | — (graph size) |
| `agent_review_ast_edges` | histogram | — |

## Tracing + logs

- Span `review.ast` (`n_changed`, `hops`, `nodes`, `edges`, `duration_ms`).
- Logs: `INFO` "call graph: {nodes} fns / {edges} edges over {n} changed" — counts
  only, never source or signatures.

## Security

- The Go helper runs in the `Sandbox`; its JSON output is untrusted → parsed to
  typed messages with **caps** on node/edge counts (drop-with-count past the cap),
  bounded string fields, and `confine`d file paths.
- `hops` is clamped to a small max so a hostile repo can't induce a whole-graph
  blowup.
- `adversarial_` cases: a package graph with a cycle (must terminate), a crafted
  file path in the helper output pointing outside the repo (dropped), an
  enormous generated file (node cap holds).

## Deferred

- **Interface/implementation edges** (dynamic dispatch precision beyond `cha`) —
  `rta` where a build is available; coarse `cha` otherwise.
- **Cross-package data-flow** — out of scope; the call graph + package shape is the
  right first level of grounding.
- **Rust/other AST backends** — seam is ready; not built first.
