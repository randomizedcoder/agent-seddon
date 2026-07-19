# Architecture & abstraction boundaries

A contributor-facing map of *where the seams are* and *what a change touches*. Read
this first, then follow a link into the per-component doc for detail. For the design
rationale behind each seam read [`DESIGN.md`](../DESIGN.md); for the shared "how do I
add one" mechanics read [`extending.md`](extending.md).

## The shape: one crate per seam

Every replaceable component is an `async` trait in `agent-core`; its
implementations live in a sibling crate gated by a cargo feature; `agent-runtime`
wires them together through a factory [`Registry`](../crates/agent-runtime/src/registry.rs)
selected by TOML config. Nothing depends "sideways" — the graph is acyclic:

```
agent-core            (the seams: traits + shared message types, no impls)
   ▲
   ├── agent-providers      (LlmProvider: anthropic, openai-compat)
   ├── agent-tools          (Tool: bash, read/write, edit, grep/find/ls, search)
   ├── agent-search         (SearchBackend: tantivy full-text index)
   ├── agent-memory         (EpisodicStore + SemanticStore: file)
   ├── agent-context        (ContextStrategy: sliding-window, summarizing)
   ├── agent-mcp            (McpTransport: stdio, http — its own registry)
   ├── agent-proto          (protobuf/gRPC wire contracts + core↔proto convert + trace)
   ├── agent-grpc           (per-seam gRPC servers + clients over TCP/UDS; on agent-proto)
   └── agent-telemetry      (CompositeMemory decorator → ClickHouse; OTLP trace export)
        ▲
        └── agent-runtime   (Registry + builder + the agent loop; wires it all)
             ▲
             └── agent-cli  (one-shot / REPL / --serve-mcp presentation layers)

agent-testkit               (dev-only test doubles; depended on by dev-deps)
```

The loop itself (`agent-runtime/src/agent.rs`) is the one place that orchestrates
the seams, and it only ever talks to the traits — never a concrete provider, tool,
or store.

## Seam scorecard

| Seam | Trait (`agent-core`) | Selected by | Wired via | Detail |
|------|----------------------|-------------|-----------|--------|
| Provider | `LlmProvider` | `[agent] provider` | `Registry::provider` | [providers](components/providers.md) |
| Tool | `Tool` (+ `ToolRegistry`) | `[tools] enabled` | `Registry::tool` | [tools](components/tools.md) |
| Context strategy | `ContextStrategy` | `[agent] context` | `Registry::context` | [context](components/context.md) |
| Policy | `Policy` | `[agent] policy` | `Registry::policy` | [policy](components/policy.md) |
| Memory (whole store) | `MemoryStore` | `[memory] backend` | `Registry::memory` | [memory](components/memory.md) |
| Memory — episodic | `EpisodicStore` | `[memory] backend` | `Registry::episodic` | [memory](components/memory.md) |
| Memory — semantic | `SemanticStore` | `[memory] semantic` | `Registry::semantic` | [memory](components/memory.md) |
| MCP transport | `McpTransport` | `[[mcp.servers]] kind` | `Registry::transport` | [mcp](components/mcp.md) |
| Search | `SearchBackend` | `[search] backends` | `Registry::search` | [search](components/search.md) |

Every seam is uniform: a config string selects a named factory from a registry, and
out-of-tree code can register its own factory on the `Registry` passed to
`build_agent_with` without forking — MCP transports included (the runtime `Registry`
owns the `TransportRegistry`).

## Components

High-level summaries; each links to its detailed doc.

- **[Providers](components/providers.md)** — the model behind a uniform
  request/response. Ships `openai-compat` + `anthropic`; `complete` required,
  `stream` optional.
- **[Tools](components/tools.md)** — named capabilities the model invokes.
  `bash`/file/`edit`/search built-ins, MCP tools, and `delegate` all share one
  `ToolRegistry`.
- **[Memory](components/memory.md)** — layered: a `MemoryStore` facade over
  independently-swappable `EpisodicStore` + `SemanticStore`. Real, opt-in
  distillation.
- **[Context strategies](components/context.md)** — assemble + compact the working
  window. `sliding-window` and (model-backed) `summarizing-window`.
- **[Policy](components/policy.md)** — the tool-approval gate. `auto-approve`,
  `interactive`.
- **[MCP](components/mcp.md)** — external tools as first-class tools, plus the
  transport seam and the `--serve-mcp` server.
- **[gRPC seams](grpc.md)** — the protobuf wire contracts (`agent-proto`) and
  per-seam gRPC servers/clients (`agent-grpc`) that let each seam run as a separate
  process/container over **TCP or unix domain sockets**, selected by `= "grpc"`
  config and hosted by `agent --serve-<seam>`.
- **[Tracing](tracing.md)** — the loop instrumented as a span tree, exported over
  OTLP and (across gRPC hops) reassembled into one distributed trace in a
  ClickStack/HyperDX collector; runbook + demo.
- **[Runtime](components/runtime.md)** — the registry, builder, loop, config, and
  cross-cutting pieces (subagents, skills, context files, metrics, telemetry,
  tracing).
- **[Testing](components/testing.md)** — `agent-testkit` shared doubles.

## What editing X touches

The blast radius of a change tells you where the real coupling is.

- **Add a provider / tool / context strategy / policy / memory backend / MCP
  transport** — the isolated case. New impl in the owning crate (1 file) + one
  registration line. The loop, the other seams, and the CLI are untouched. This is
  the whole point of the design.

- **Change the shared message currency** (`Message`, `ToolCall`, `Observation`,
  `ToolSchema`, `CompletionRequest/Response`, `CompletionChunk` — all in
  [`agent-core/src/lib.rs`](../crates/agent-core/src/lib.rs)) — the wide case.
  These types are the lingua franca between *every* seam, so a change ripples
  through every provider, every tool, and every context strategy. Treat them as a
  deliberately stable API; extend additively (serde-defaulted fields, as
  `MemoryEvent` does) rather than reshaping.

- **Change the loop shape** (iteration semantics, tool dispatch) —
  `agent-runtime/src/agent.rs` only. It's intentionally the *single* orchestrator;
  impl crates don't move. If the public `Agent`/`Session` API changes, the CLI
  presentation layers (`agent-cli`) follow, but nothing else.

- **Change config** — `agent-runtime/src/config.rs` owns the TOML schema. New
  fields are `#[serde(default)]` so old config files keep parsing.

## Extension entry points

The mechanics are the same for every seam — see [`extending.md`](extending.md) for
the shared workflow, and each component doc for its "Adding your own" specifics.
In-tree adds a feature-gated line to `register_builtins`; out-of-tree registers
factories on a `Registry` and calls `build_agent_with` (no fork). Test new impls
with [`agent-testkit`](components/testing.md).
