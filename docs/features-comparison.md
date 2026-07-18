# Features Comparison: agent-seddon vs. pi vs. hermes-agent

**Original analysis:** 2026-07-17 · **Last refreshed:** 2026-07-18 to match the
current code.
**Status:** Capability inventory vs. two reference harnesses + remaining roadmap.

> The original document was written before the plugin registry + P0 work. It has
> since been refreshed: the matrix, the per-area notes, and the roadmap below all
> reflect what is **actually implemented today** — plugin registry, `edit` /
> `grep` / `find` / `ls` tools, an Anthropic-native provider, real streaming and
> parallel tool execution, summarizing compaction, an MCP client (stdio + HTTP)
> and server (`--serve-mcp`), subagent `delegate`, an interactive REPL with
> session resume + slash commands + rustyline history, and skills.

## Purpose

`agent-seddon` is an experimental, Rust-based coding-agent harness. This document
inventories its capabilities against two mature open-source harnesses and states
honestly what we have, how complete it is, and what remains. The framing intent:
**grow `agent-seddon` into a full-featured coding agent** — a daily driver, not a
research toy. The two yardsticks:

- **[pi](https://github.com/earendil-works/pi)** — a TypeScript monorepo with a
  deliberately *minimal core* (no built-in MCP or subagents) but exceptional
  breadth elsewhere: 40+ LLM providers, a polished differential-rendering TUI,
  session branching, LLM-summarizing compaction, and a first-class
  extension/skill/theme system.
- **[hermes-agent](https://github.com/NousResearch/hermes-agent)** — a large
  Python "batteries-included" harness: ~94 tools, 27 provider plugins, 8 memory
  backends, MCP client *and* server, subagents + a kanban coordination board,
  19 messaging-platform gateways, 4 UI surfaces, and multiple sandboxing backends.

pi = disciplined minimalism, hermes = maximalism. `agent-seddon` now covers the
coding fundamentals both ship, still sits below both in raw breadth (providers,
tools, UI surfaces), and has a genuinely differentiated observability stack.

---

## TL;DR

The core loop is sound and the coding fundamentals are in: 7 tools
(`bash`/`read_file`/`write_file`/`edit`/`grep`/`find`/`ls`), two providers
(OpenAI-compatible + Anthropic-native) with real SSE **streaming**, **parallel**
tool execution, **summarizing** *or* truncating compaction, a layered file-memory,
an interactive **REPL** (history, slash commands, session resume), **skills**,
**subagent delegation**, and an **MCP client + server**. Our standout remains
**production-grade observability** (Prometheus metrics + ClickHouse event/log/usage
streaming) that neither reference harness ships out of the box.

What's left is mostly breadth and a couple of stubbed seams: more providers, web/
browser tools, sandboxed execution, a full-screen TUI, embedding-based recall, and
activating the distillation (episodic→semantic) pipeline.

---

## The three harnesses at a glance

| | **agent-seddon** (us) | **pi** | **hermes-agent** |
|---|---|---|---|
| Language | Rust | TypeScript | Python (+ TS UIs) |
| Scale | 9 crates | TS monorepo, 5 packages | ~40k+ core LOC, ~900 test files |
| Philosophy | Trait seams, config-swappable | Minimal core + extensions | Batteries-included |
| Maturity | Experimental, fundamentals complete | Production, polished | Production, sprawling |
| Standout strength | **Observability (Prometheus + ClickHouse)** | Provider breadth + TUI + branching | Tools/providers/surfaces breadth + multi-agent |
| Providers | 2 (OpenAI-compat + Anthropic) | 40+ | 27 |
| Tools | 7 built-in + MCP | ~8 | ~94 |
| UI surfaces | CLI + interactive REPL | Rich TUI + print/JSON/RPC/SDK | CLI + TUI + web + desktop |

---

## Feature comparison matrix

Coverage rubric (our column): ✅ Full · 🟡 Partial · 🟦 Seam only (trait defined, no
impl) · ❌ Missing · ➖ N/A.

| Feature area | agent-seddon | pi | hermes | Our coverage |
|---|---|---|---|---|
| Agent loop (assemble→call→tools→record) | Yes | Yes | Yes | ✅ |
| Streaming completions | Yes (SSE, both providers) | Yes | Yes | ✅ |
| Parallel tool execution | Yes (concurrent per turn) | Yes | Yes | ✅ |
| Steering / follow-up while running | No | Yes | Yes | ❌ |
| Multi-turn session (REPL) | Yes | Yes | Yes | ✅ |
| `bash` tool | Yes | Yes | Yes | ✅ |
| `read_file` / `write_file` | Yes | Yes | Yes | ✅ |
| `edit` (surgical string replace) | Yes | Yes | Yes (`patch`) | ✅ |
| `grep` / `find` / `ls` | Yes (gitignore-aware) | Yes | Yes (`search_files`) | ✅ |
| Web search / fetch | No | Via extension | Yes | ❌ |
| Browser automation | No | No (external) | Yes | ❌ |
| LLM providers | 2 (OpenAI-compat + Anthropic) | 40+ | 27 | 🟡 |
| Provider capability metadata | Yes (basic) | Yes (rich, cost) | Yes | 🟡 |
| Context assembly | Yes | Yes | Yes | ✅ |
| Compaction | Truncation **and** LLM summary | LLM summary | LLM summary | ✅ |
| Session branching | No | Yes (`/tree`) | Partial | ❌ |
| Working / episodic / semantic memory | Yes (layered) | Sessions only | MEMORY+USER files | ✅ |
| Memory recall | Keyword scan | ➖ | FTS5 + LLM + vector plugins | 🟡 |
| Distillation (episodic→semantic) | Seam only (no-op stub) | ➖ | Curator | 🟦 |
| Prometheus metrics | Yes | No | No | ✅ |
| Structured telemetry sink (ClickHouse) | Yes | Adapter interface | Trace upload | ✅ |
| MCP client | Yes (stdio + HTTP) | No (by design) | Yes | ✅ |
| MCP server | Yes (`--serve-mcp`, stdio) | No | Yes | ✅ |
| Distributed components (run seams as services) | Yes (gRPC over TCP/UDS, `--serve-<seam>`) | No | No | ✅ |
| Distributed tracing | Yes (OpenTelemetry/OTLP → ClickStack) | No | Trace upload | ✅ |
| Permission / approval gate | Yes (auto/interactive) | No (trust model) | Yes (rich) | 🟡 |
| Path-traversal safety on file tools | Yes | — | — | ✅ |
| Sandboxed execution backends | No | Docs/patterns | 6 backends | ❌ |
| Subagents / delegation | Yes (`delegate`, depth-capped) | Extension | Yes + kanban | ✅ |
| Session persistence / resume | Yes (JSONL + `--continue`/`--resume`/`/resume`) | Yes (JSONL + `/resume`) | Yes (SQLite) | ✅ |
| Interactive REPL / TUI | REPL (line-based, rustyline) | Rich TUI | Rich TUI | 🟡 |
| Slash commands | Yes | Yes | Yes | ✅ |
| Skills (SKILL.md) | Yes (`/skill:<name>` load) | Yes | Yes | ✅ |
| Plugins / extensions | Compile-time seams + MCP tools + skills | Yes (hot-reload TS) | Yes (19 plugin types) | 🟡 |
| Hooks | No | Yes (events) | Yes | ❌ |
| Config system | TOML | JSON | YAML | ✅ |
| User context files (project rules) | Yes (`context.d/`) | Skills/templates | `.hermes/context` | ✅ |
| Multi-platform messaging | No | No | 19 platforms | ➖ |
| Cron / scheduled runs | No | No | Yes | ❌ |
| Test suite | Unit + integration + Nix checks | vitest | ~17k pytest | 🟡 |

---

## Per-area notes

### Agent loop / execution model — solid
`crates/agent-runtime/src/agent.rs` runs assemble → complete → policy-gated tool
dispatch → record → compact, with metrics on every path, refactored into a
`Session` that keeps its working set across turns (multi-turn REPL) while
`Agent::run` remains a one-shot. Completions **stream** (SSE) with a live echo, and
a turn's parallel-safe tool calls run **concurrently** (`join_all`), results
appended in call order. The remaining gap vs. pi/hermes is **steering / follow-up**
(interrupting or queueing work mid-run).

### Tools — coding fundamentals in
Seven built-ins, all registered through the plugin registry and gated by cargo
features: `bash`, `read_file`, `write_file` (`tool-core`), `edit` (`tool-edit`,
unique/`replace_all` string replace), and `grep`/`find`/`ls` (`tool-search`,
gitignore-aware via ripgrep's `ignore` crate). All share lexical path-traversal
protection and output caps. MCP servers add more tools at runtime as
`mcp_<server>_<tool>`. Remaining: web/browser tools (hermes has ~90 tools total).

### LLM providers — right architecture, thin breadth
Two hand-rolled impls behind the `LlmProvider` trait: `OpenAiCompatProvider`
(GLM/OpenAI/vLLM/Ollama) and a native `AnthropicProvider` (Messages API,
`tool_use`/`tool_result`), both with real SSE `stream`. pi has 40+ providers with
cost metadata and OAuth; hermes 27. Breadth is the gap — a `genai`-style wrapper
(DESIGN.md §9) would close much of it.

### Context management / compaction — both strategies
Two context strategies, selected by `[agent] context`: `SlidingWindow` (drops the
oldest turns — lossy but free) and `SummarizingWindow` (`context-summarizing`,
keeps the head + a recent tail `keep_recent_tokens` and LLM-summarizes the middle,
falling back to truncation on error). Non-destructive w.r.t. the episodic log. pi
additionally does branch summarization for `/tree`.

### Memory — strong bones; recall + distillation still basic
A genuine 3-layer model (`crates/agent-memory/src/file.rs`): in-memory working,
append-only JSONL episodic (never mutated), and markdown semantic. Recall is a
**keyword-count scan** of the semantic directory on each query (no embeddings, no
index). `distill()` (episodic→semantic promotion) is an **honest no-op stub** that
runs at session end but does nothing yet. hermes has 8 memory backends incl.
vector/dialectic. Remaining: activate distillation + an embedding-backed
`SemanticStore` (both are documented seams).

### Telemetry / metrics / observability — our moat
`crates/agent-runtime/src/metrics.rs` exposes 10 Prometheus metrics over a
`/metrics` endpoint (+ optional Pushgateway). `crates/agent-telemetry/` streams a
full transaction history to **ClickHouse** — three tables (`agent_events`,
`agent_logs`, `agent_usage`), keyed by per-run `session_id`, via a batched
background writer that drops rows rather than blocking if ClickHouse is down.
On top of that, **OpenTelemetry tracing**: the loop is instrumented as a span tree
and exported over OTLP; W3C context propagates across gRPC seam boundaries, so a
distributed run reassembles into **one trace** in a ClickStack/HyperDX collector
(see [`tracing.md`](tracing.md)). Neither pi nor hermes ships a turnkey metrics +
queryable-history + distributed-tracing stack.

### Distributed components (gRPC) — a differentiator
Because every seam is a config-selected trait, a component can run as its own
process/container: `agent-proto` (binary protobuf contracts) + `agent-grpc`
(per-seam servers/clients over TCP or unix domain sockets) let the loop dial a
remote provider/memory/tools/context/policy with `= "grpc"`, hosted by
`agent --serve-<seam>` (see [`grpc.md`](grpc.md)). This enables a k8s-style
topology — a model gateway, a shared memory service, sandboxed tool workers —
that neither pi nor hermes offers out of the box.

### MCP — client and server
`crates/agent-mcp` is an MCP **client** (stdio subprocess + streamable HTTP behind
an `McpTransport` trait): it runs `initialize`, discovers tools (`tools/list`), and
registers each into the same `ToolRegistry` as the built-ins. `agent --serve-mcp`
(`crates/agent-cli/src/mcp_server.rs`) is the **server** side — exposes a single
`run` tool so another MCP client can drive the whole agent loop. Matches hermes;
pi deliberately omits MCP.

### Permissions / sandboxing / security — good primitives
A `Policy` seam (`AutoApprove`, `Interactive`) plus lexical path-traversal
protection and output/time caps. More built-in safety than pi (trust model only),
far less than hermes (dangerous-command detection + 6 execution backends).
Remaining: an `AllowList` policy and a sandboxed (Docker) execution backend.

### Subagents / orchestration — implemented
`crates/agent-runtime/src/subagent.rs`: with `[agent] subagents = true`, a
`delegate` tool spawns a child agent from the same components, runs it in isolated
context, and returns only the summary (the boomerang pattern), depth-bounded by
`subagent_max_depth`. hermes goes further with batch/async workers + a kanban board.

### Session management / persistence / resume — done
`crates/agent-runtime/src/session_store.rs` saves each REPL turn's transcript under
`.agent/sessions/<id>.jsonl`; resume via `--continue` (most recent), `--resume ID`,
or `/resume` in the REPL. pi additionally has in-place branching; hermes SQLite +
FTS5.

### CLI / REPL / UI surfaces — REPL, not yet a full TUI
`agent` runs one-shot with a goal or opens an interactive **REPL** with no goal:
multi-turn, live streaming, rustyline history + line editing (piped input falls
back to plain reading), and slash commands. Still line-based, not a full-screen
differential-render TUI like pi/hermes — that's the main remaining experience gap.

### Skills / plugins / extensions / hooks / slash commands — partial
Compile-time extensibility (seams + cargo features + the registry), **plus**
runtime capability without recompiling: MCP tools, `SKILL.md` skills
(`/skills`, `/skill:<name>`), and slash commands. Still missing vs. pi/hermes:
hot-reloadable extensions and lifecycle hooks.

### Configuration — strong
A type-safe single-file TOML (`crates/agent-runtime/src/config.rs`) with sections
for agent / provider / memory / tools / mcp / telemetry / context-files / metrics,
three-tier API-key resolution (inline > env > file), and tilde expansion.

### User context files — done
`context.d/prepend/*.md` and `context.d/append/*.md` are always-injected,
numerically ordered project instructions (`crates/agent-runtime/src/context_files.rs`).

### Testing — proportionate
Unit tests across crates + an MCP client↔server integration test, all run under the
Nix flake checks (clippy `-D warnings`, rustfmt, `cargo test`, cargo-audit,
nix-fmt). Far smaller than pi's vitest suites or hermes's ~17k tests.

---

## Where we already lead

1. **Observability.** First-class Prometheus metrics + ClickHouse transaction/log/
   usage streaming, best-effort and non-blocking. A genuine moat for anyone who
   wants to *measure and compare* agent runs.
2. **Clean trait-seam architecture.** Every major component is an `async` trait
   wired by a config-selected registry, gated by cargo features. Swapping
   provider/memory/context/policy is a one-line TOML edit; third parties can add
   modules in-tree or out-of-tree without forking.
3. **Rust.** Performance, memory safety, single static binary, no runtime/venv.
4. **Reproducible tooling.** Modular Nix flake (dev shell, checks, ClickHouse
   container).

---

## Roadmap

### Shipped since the original analysis
Plugin registry + cargo-feature gating · `edit` / `grep` / `find` / `ls` tools ·
Anthropic-native provider · streaming (both providers) · parallel tool execution ·
summarizing compaction · MCP client (stdio + HTTP) · MCP server (`--serve-mcp`) ·
subagent `delegate` · interactive REPL (rustyline history) · session resume · slash
commands · skills.

### Remaining

Effort key: **S** ≈ hours–1 day · **M** ≈ a few days · **L** ≈ 1–2 weeks.

| Feature | Current | Target | Effort |
|---|---|---|---|
| Distillation pipeline *(seam exists)* | No-op stub | Episodic→semantic promotion via the model | M |
| Embedding-based recall *(seam exists)* | Keyword scan | Vector semantic store (Qdrant/LanceDB) | L |
| More providers | 2 hand-rolled | `genai`-style wrapper for breadth (DESIGN.md §9) | M |
| Web search / fetch tools | None | Built-in web tools | M |
| Sandboxed execution backend | None | Docker backend for `bash` | L |
| `AllowList` policy *(seam exists)* | auto/interactive | Pattern allowlist | S |
| Full-screen TUI | Line-based REPL | Differential-render multi-turn TUI | L |
| Steering / follow-up | None | Interrupt / queue work mid-run | M |
| Session branching | Linear resume | In-place branch tree | M |

---

## Philosophy note

pi keeps a **small core** and pushes MCP, subagents, and plan-mode into installable
extensions; hermes bundles **everything**. `agent-seddon` has pi's structural
discipline (trait seams + config wiring) — so it can pursue hermes-like breadth
*incrementally*, each item landing behind an existing seam. The posture that got us
here and should continue: keep the core small and swappable, close the fundamentals
both references ship, and lean into observability as the differentiator.
