# Increment 04 — Graph document, anchor-slot executor, control-plane services

Goal: the pieces from increments 01–03 stop being three hand-wired config blocks
and become **nodes in a user-configurable graph**: a declarative document routes
the turn's context along typed edges, background branches hang off delivery, the
gate is a loop-until node — and the document format + schema registry are shaped
so the Flutter portal can render a drag-and-drop editor from data alone.

## The document

Human-readable form is **textproto** (the model-router 03 decision: prost-reflect
over the descriptor set already emitted for gRPC reflection; scenario files
selectable by flag):

```textproto
# config/cognition/default.textproto
version: 1
node { key: "generate"   value { type: "generate"     type_version: 1
        params { fields { key: "provider" value { string_value: "task-router" } } } } }
node { key: "gate"       value { type: "critic_gate"  type_version: 1
        params { fields { key: "critic"     value { string_value: "glm" } }
                 fields { key: "max_rounds" value { number_value: 2 } } } } }
node { key: "summarize"  value { type: "distill_summary" type_version: 1 } }
node { key: "facts"      value { type: "distill_facts"   type_version: 1 } }
node { key: "compact"    value { type: "compact_assemble" type_version: 1
        params { fields { key: "relevance" value { string_value: "llm" } } } } }

edge { from: "anchor.response"  to: "generate"  kind: MAIN }
edge { from: "generate"         to: "gate"      kind: MAIN }        # gate loops internally
edge { from: "anchor.delivery"  to: "summarize" kind: BACKGROUND }
edge { from: "anchor.delivery"  to: "facts"     kind: BACKGROUND }
edge { from: "anchor.compaction" to: "compact"  kind: MAIN }
edge { from: "glm"              to: "gate"      kind: CAPABILITY }  # judge attachment
```

Serialization discipline (the survey's hard-won rules):

- **Flat node map, stable human-chosen ids** (ComfyUI API format; deterministic
  order → clean diffs). **Zero visual data** in the document; layout is a sidecar
  (`default.layout.json`) the executor ignores and the GUI round-trips (BPMN-DI).
- **`type_version` per node + registry-side migrations** (n8n); schemas are
  **never embedded in instances** (the Flowise/Langflow stranding failure).
- **Typed edge kinds**: `MAIN` (data flow on the turn path), `BACKGROUND`
  (fire-and-forget hand-off to the distiller worker; delivery never waits),
  `CAPABILITY` (attachment — which judge/summarizer/store a node uses; n8n's
  `ai_languageModel`-style edges).
- Params reference upstreams/providers/stores **by name** — never secrets, never
  endpoints; those stay in `[route]`/`[provider]`/`[digest]` config.

## Anchor-slot executor

`run_loop` stays the host (Option E). Three anchors, each running the configured
sub-graph deterministically:

- **`anchor.response`** — between `provider.complete` and acceptance: the
  `generate → critic_gate` chain (increment 01's provider, re-expressed; the
  `consensus` provider remains as the graph-less fallback and the node reuses its
  engine verbatim).
- **`anchor.delivery`** — the accepted final answer: mints `agreed_seq`, then
  fans `BACKGROUND` edges out to the distiller queue (increment 02's worker; one
  queue, jobs tagged by node id).
- **`anchor.compaction`** — the `context.compact` call: `compact_assemble`
  (increment 03's strategy).

Executor rules: `MAIN` sub-graphs are acyclic **between** nodes (validated at
load — cycles rejected); loops exist only *inside* loop-until nodes
(`critic_gate`), each with a bounded round cap; a node error on the `MAIN` path
fails soft to the anchor's built-in behavior (deliver ungated / summarizing
window), never the turn; `BACKGROUND` failures are counted, never surfaced
mid-turn. Node result is one closed enum (`Continue(output) | Loop(feedback) |
Fanout(jobs) | Fallback(reason)`), the Mastra/graph-flow lesson: a tiny
vocabulary beats thirty node classes.

Node registry: node types are registry factories (`register_builtins` pattern,
feature-gated), constructed per-graph at build time. v1 node set: `generate`,
`critic_gate`, `distill_summary`, `distill_facts`, `objective`,
`compact_assemble`; increment 05 adds the fork/join set — `split`, `join`,
`merge` ([`05-parallel-branches.md`](05-parallel-branches.md)). New types =
implement trait + factory line + schema — the existing seam-impl recipe.

Load validation (fail closed, at startup): unknown node type / `type_version`,
dangling edge endpoint, `MAIN` cycle, `BACKGROUND` edge from a non-delivery
anchor, params failing the node's schema, unresolvable capability reference —
each a typed error naming the node id.

## Protos + services (additive → no buf baseline bump)

`proto/agent/v1/graph.proto`:

```proto
message CognitionGraph {
  uint32 version = 1;
  map<string, Node> nodes = 2;
  repeated Edge edges = 3;
}
message Node { string type = 1; uint32 type_version = 2; google.protobuf.Struct params = 3; }
message Edge {
  string from = 1; string to = 2;
  enum Kind { KIND_UNSPECIFIED = 0; MAIN = 1; BACKGROUND = 2; CAPABILITY = 3; }
  Kind kind = 3;
}
message NodeTypeSchema {
  string type = 1; uint32 type_version = 2; string title = 3; string doc = 4;
  repeated Port inputs = 5; repeated Port outputs = 6;
  string params_json_schema = 7;   // JSON Schema + UI hints (n8n properties[] shape)
}
message Port { string name = 1; string kind = 2; }   // nominal type strings, ComfyUI-style

service GraphService {
  rpc Get(GetGraphRequest) returns (CognitionGraph);
  rpc Put(PutGraphRequest) returns (PutGraphResponse);       // validates before accept
  rpc Validate(CognitionGraph) returns (ValidateResponse);   // typed per-node errors
  rpc DescribeNodeTypes(DescribeNodeTypesRequest) returns (DescribeNodeTypesResponse);
}
```

`DescribeNodeTypes` is the **`/object_info` analog**: the Flutter editor renders
node bodies, typed ports, and config forms entirely from this response — no
Dart-side knowledge of node types, so custom nodes need zero frontend code.
That plus the layout sidecar makes canvas projection mechanical.

`proto/agent/v1/digest.proto`:

```proto
message Digest { /* mirror of core Digest, §02 */ }
service DigestService {
  rpc Put(PutDigestRequest) returns (PutDigestResponse);
  rpc Query(QueryDigestsRequest) returns (QueryDigestsResponse);
}
```

Both follow the full new-service recipe: convert.rs pairs (proto→core fallible,
server-side `safe_segment` + numeric clamps — a `grpc` store is untrusted),
server + client, `nix/constants.nix` ports + `gen-constants`, `"grpc"` factory
lines, SEAMS rows (`--serve-graph`, `--serve-digest`), TCP+UDS round-trip tests,
reflection, `Metered<T>`, component docs.

Startup: `agent --cognition-graph <file.textproto>` (scenario files, mirroring
`--model-router-config`); absent flag + absent `[graph]` config = the three
increments behave exactly as their TOML config wired them (the graph is a
*re-expression*, not a prerequisite).

## Example graphs (shipped as scenario files + integration tests)

Three graded examples under `config/cognition/`, each a working document AND an
integration-test fixture (a `nix run .#graph-examples`-style check loads,
validates, and dry-runs each against scripted providers — the graphs *are* the
integration tests of the whole feature):

1. **`simple.textproto`** — gate only: `generate → critic_gate` on the response
   anchor. The smallest useful graph; validates the anchor plumbing.
2. **`intermediate.textproto`** — gate + background distillation: adds the two
   `BACKGROUND` edges (`distill_summary`, `distill_facts`) off delivery and
   `compact_assemble` on the compaction anchor — the full increments 01–03
   behavior, expressed as a document.
3. **`advanced.textproto`** — the flow as originally described plus increment
   05: a `split` into safety-lens and performance-lens implementation branches
   (the safety branch carrying its own gate loop), `join policy: all`, a
   `synthesize` merge recording the loser to the alternatives ledger, the
   merged result through the final consensus gate, background distillation on
   delivery, instant compaction assembling summaries + facts + open
   alternatives.

## GUI (portal track, out of scope here — kept unbroken)

What this increment guarantees the future editor: stable node ids + named typed
ports (projects 1:1 into any canvas library), `DescribeNodeTypes` for
auto-rendered forms, `Validate` for live feedback while dragging, layout sidecar
round-trip, `Put` guarded by full validation. The editor itself lands in
[`portal/`](../portal/README.md).

## Tests / verification

Fixtures: a **graph-document corpus** (`testdata` module, README §Harness
obligations) — the default graph, scenario variants, and **one invalid document
per typed load-error class** — shared by the local `Validate` tables, the gRPC
round-trips, and the load/dispatch bench; the `DigestService` round-trips seed
`agent_digest::testdata` through the wire.

- Document: parse/validate tables — `positive_` the default graph; `negative_`
  each typed load error (one corpus file each); `corner_` empty graph (=
  built-in behavior), node with no edges; `boundary_` version 0/max, param at
  schema bounds; `adversarial_` cyclic MAIN graph, dangling ids, hostile params
  (path-like provider names rejected by `safe_segment` discipline, huge Struct
  capped), textproto bombs (size cap before parse).
- Executor: anchor behavior parity — graph-configured gate produces byte-identical
  requests to increment 01's provider (`ScriptedProvider`); BACKGROUND edges
  never delay delivery; MAIN node error → anchor fallback.
- Services: round-trip TCP+UDS; `Validate` returns the same errors as local
  load; `DescribeNodeTypes` covers every registered type (an
  `every_node_type_has_a_schema` test, the `every_seam_has_a_table_row` pattern).
- iai bench: graph load+validate and per-anchor dispatch under Ir ceilings; dhat
  leak over load→run→drop.
