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
| Tokenizer | `Tokenizer` (+ `Prices`) | `[tokenizer] backend` | `Registry::tokenizer` | [tokenizer](components/tokenizer.md) |
| Repo (git) | `RepoBackend` | `[git] backend` | `Registry::repo` | [git](components/git.md) |
| Verifier | `Verifier` | `[verifier] backend` | `Registry::verifier` | [verifier](components/verifier.md) |
| Forge | `Forge` | `[forge] backend` | `Registry::forge` | [forge](components/forge.md) |
| Web search | `WebSearch` | `[web_search] backend` | `Registry::web_search` | [web-search](components/web-search.md) |

A registry seam is uniform: a config string selects a named factory, and out-of-tree
code can register its own on the `Registry` passed to `build_agent_with` without
forking — MCP transports included (the runtime `Registry` owns the `TransportRegistry`).

### Beyond the registry — composed and optional seams

`agent-core` defines **36 seam traits** in total (the authoritative list is
`crates/agent-core/src/lib.rs`, one `// Seam:` banner each). The scorecard above is the
config-string-selected subset; the rest are wired two other ways:

- **Composed in the builder** (they compose other seams rather than being picked by a
  single backend string — see [extending.md](extending.md)): `LlmPool` (`[pool]`,
  [pool](components/pool.md)), `TaskClassifier` (`[mode]`, [mode](components/mode.md)),
  `DimensionStore` (`[dimensions]`, [dimensions](components/dimensions.md)).
- **Optional capabilities**, config-gated and attached via `Agent::with_*`:
  `Sandbox` ([sandbox](components/sandbox.md)), `Pty` ([pty](components/pty.md)),
  `LspBackend` ([lsp](components/lsp.md)), `WebBackend` ([web-fetch](components/web-fetch.md)),
  `TaskTracker` ([tasks](components/tasks.md)), `Embedder` ([embedder](components/embedder.md)),
  `PromptStore` ([prompt](components/prompt.md)), `MetricsProxy` ([metrics-proxy](components/metrics-proxy.md)),
  `ReferenceResolver` ([reference](components/reference.md)), `SessionStore` ([session](components/session.md)),
  `Scanner` ([scanner](components/scanner.md)), `CacheStrategy` ([prompt-cache](components/prompt-cache.md)),
  `ReviewCollector` (code-review flow), `OutputSchema` ([structured-output](components/structured-output.md)),
  `Hook` ([hooks](components/hooks.md)), `Scheduler` ([scheduler](components/scheduler.md)).
- **Session / identity infrastructure**: `SessionRegistry`, `SessionSource`,
  `SessionSourceRegistry` ([session](components/session.md)), plus the supporting
  `Prices` (paired with the tokenizer) and the memory layer traits `EpisodicStore` /
  `SemanticStore`.

Every seam — however wired — is still just an `agent-core` trait, and any of them can be
hosted over gRPC (`agent --serve-<seam>`) or dialed with `= "grpc"`.

## Crate map

The 37 crates, by role. Dependency direction is unchanged from DESIGN.md §7: everything
depends on `agent-core`; `agent-runtime` depends on the (feature-gated) impl crates;
`agent-cli` depends on `agent-runtime`; `agent-core` depends on nothing internal.

**Core & runtime**
- `agent-core` — the seam traits + shared vocabulary (`Message`/`ToolCall`/…), identity
  newtypes, and the audited path/injection validators. No impls.
- `agent-runtime` — the agent loop, the plugin `Registry` + `builder`, `Policy`, the
  `metered` decorators, sessions/`SessionManager`, and `config`. The biggest crate.
- `agent-cli` — the `agent` binary: CLI/REPL, `--serve-<seam>`, MCP + metrics servers.

**Seam impl crates** (one seam-family each)
- `agent-providers` (`LlmProvider`/`LlmPool`: openai-compat, anthropic, router, pool) ·
  `agent-tools` (`Tool` built-ins + seam-dialing tools) · `agent-context`
  (`ContextStrategy`) · `agent-memory` (`Episodic`/`Semantic`/`Dimension` stores) ·
  `agent-search` (`SearchBackend`: tantivy + vector) · `agent-git` (`RepoBackend`) ·
  `agent-tokenizer` (`Tokenizer`/`Prices`) · `agent-mcp` (MCP client + transports) ·
  `agent-review` (`ReviewCollector`) · `agent-verifier` (`Verifier`) · `agent-mode`
  (`TaskClassifier`) · `agent-forge` (`Forge`) · `agent-lsp` (`LspBackend`) ·
  `agent-sandbox` (`Sandbox`) · `agent-pty` (`Pty`) · `agent-scanner` (`Scanner`) ·
  `agent-web` (`WebBackend`) · `agent-web-search` (`WebSearch`) · `agent-embed`
  (`Embedder`) · `agent-session` (`SessionStore`/`SessionRegistry`) · `agent-reference`
  (`ReferenceResolver`) · `agent-tasks` (`TaskTracker`) · `agent-prompt` (`PromptStore`)
  · `agent-cache` (`CacheStrategy`) · `agent-validate` (`OutputSchema`) · `agent-scheduler`
  (`Scheduler`) · `agent-metrics-proxy` (`MetricsProxy`) · `agent-export` (session export).

**Infrastructure & cross-cutting**
- `agent-grpc` (per-seam gRPC servers/clients over TCP/UDS) · `agent-proto` (protobuf
  wire contracts + core↔pb conversions) · `agent-metrics` (the Prometheus registry) ·
  `agent-telemetry` (OTLP + ClickHouse sinks) · `agent-retry` (the one retry/backoff
  library).

**Dev**
- `agent-testkit` — shared test doubles (a dev-dependency only).

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
  config and hosted by `agent --serve-<seam>`. Every served seam enables **gRPC
  reflection**, so it can be introspected and called with JSON via `grpcurl`; the
  wire contract is linted + breaking-change-gated by **`buf`** in `nix flake check`
  (codegen stays on `tonic-build`).
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
