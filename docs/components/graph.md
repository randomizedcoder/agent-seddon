# Cognition graph (`[graph]`, `--cognition-graph`)

The declarative node graph (cognition-graph increment 04,
[design](../design/cognition-graph/04-graph-config.md)) that re-expresses the
consensus gate (01), background distillation (02), and instant compaction (03)
as **user-configurable wiring**: a flat node map + typed edges (`main` /
`background` / `capability`), hung off three fixed anchor slots in the run loop
— `anchor.response`, `anchor.delivery`, `anchor.compaction`. The document is
data (textproto); layout is a GUI-owned sidecar; params reference providers and
stores **by name only** — endpoints and keys stay in config.

```toml
[graph]
store = "file"                    # "" (off) | "file" | "grpc" (central editor service)
file = ".agent/graph.textproto"
```

`agent --cognition-graph config/cognition/intermediate.textproto "<goal>"` is
the scenario-file shorthand; graded examples (each also an integration-test
fixture) live in [`config/cognition/`](../../config/cognition/README.md).

## How it executes (Option E: compile → the same engines)

At build time a non-empty document is **compiled onto the config the factories
read**: a `critic_gate` node becomes the `[consensus]` provider (critic from
the `critic` param or a capability edge), `distill_summary`/`distill_facts`
background nodes decide **which distiller kinds run** (the document is the
wiring authority for its anchors) and their token budgets,
`compact_assemble`/`objective` select and parameterize the `instant-window`
strategy. The graph drives the *same engines* the TOML blocks drive — the
runtime guarantees (gate fail-open, `try_send` background, compaction
fail-soft chain) are inherited, not re-implemented. The generic per-turn
dataflow interpreter is the recorded deferral (design Option C).

Fail closed / fail soft, precisely: an **unloadable or invalid document is a
startup error** naming the typed issues; a **valid document with a shape this
executor cannot express** (a split — increment 05, an unsupported type on an
anchor) logs a warning and that fragment falls back to the anchor's built-in
behavior. An **empty document** = the built-in graph-less behavior.

## Validation (fail closed, collected)

Every failure class is a typed `GraphIssue` naming the node: `bad_version`,
`too_large` (nodes/edges/params/document caps — the size cap applies **before**
parsing), `bad_node_id` (safe segment, ≤64 chars, reserved `anchor.*`
namespace), `unknown_node_type`, `unknown_type_version`, `bad_params` (closed
key set per type; resource names `safe_segment`-validated; hostile numbers
rejected), `dangling_edge`, `background_not_from_delivery`,
`bad_capability_ref`, `main_cycle` (Kahn over node→node `main` edges).
Validation collects everything for the editor; storage (`Put`) and execution
reject wholesale on the first issue.

## The service (`--serve-graph`, port 50082)

`GraphService` — `Get` / `Put` (validate-then-accept; invalid ⇒
`INVALID_ARGUMENT` + the typed issues, never partially stored) / `Validate`
(issues as data, for live editor feedback) / `DescribeNodeTypes` — the
`/object_info` analog: title/doc, typed ports, and a params **JSON Schema per
node type, derived from the same table the validator runs against** (they
cannot drift). The Flutter portal renders palette + forms entirely from this
response. `Get` of a broken stored document is `FAILED_PRECONDITION` (server
state, caller falls back), not a fabricated empty graph.

## Node types (v1)

`generate`, `critic_gate` (loop-until), `distill_summary`, `distill_facts`,
`objective`, `compact_assemble` — each registered in
`agent-graph/src/schema.rs` with ports + a `ParamSpec` table. Increment 05 adds
`split`/`join`/`merge`. Adding a type = implement + one table entry
(`every_node_type_has_a_schema` keeps the palette honest).

## Testing / harness

`agent_graph::testdata` is the deterministic corpus: the graded example
documents **plus one invalid document per typed issue class**, shared by the
validation tables, the gRPC round-trips (10, TCP+UDS), and the
`examples.rs` fixture test (the shipped `config/cognition/*.textproto` files
must parse to their corpus twins — files and tests cannot drift). Bench
`graph_load` (parse → decode → validate, Ir ceiling in
`nix/checks/bench.nix`); dhat leak test over the load cycle
(`nix/checks/leak.nix`).
