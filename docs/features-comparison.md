# Features Comparison: agent-seddon vs. pi vs. hermes-agent

**Date:** 2026-07-17 (updated 2026-07-18)
**Status:** Analysis / roadmap input.

> **Update (2026-07-18):** Since the original analysis, the plugin registry + P0
> features shipped, and an **MCP client** (stdio + HTTP) and **subagent
> `delegate`** landed. The rows and roadmap below are annotated where that changes
> our coverage.

## Purpose

`agent-seddon` is an experimental, Rust-based coding-agent harness. This document
inventories its capabilities against two mature open-source harnesses, states
honestly what we already have and how complete each piece is, and proposes a
**prioritized roadmap**.

The evaluation is framed by a specific intent: **grow `agent-seddon` into a
full-featured coding agent** — a daily-driver that competes with the reference
harnesses, not merely a research toy. The two yardsticks:

- **[pi](https://github.com/earendil-works/pi)** — a TypeScript monorepo with a
  deliberately *minimal core* (no built-in MCP or subagents) but exceptional
  breadth elsewhere: 40+ LLM providers, a polished differential-rendering TUI,
  session branching, LLM-summarizing compaction, and a first-class
  extension/skill/theme system. Its philosophy: keep the core small, push
  everything else to installable packages.
- **[hermes-agent](https://github.com/NousResearch/hermes-agent)** — a large
  Python "batteries-included" harness: ~94 tools, 27 provider plugins, 8 memory
  backends, MCP client *and* server, subagents + a kanban coordination board,
  19 messaging-platform gateways, 4 UI surfaces, and multiple sandboxing backends.

Together they bracket the design space: pi = disciplined minimalism, hermes =
maximalism. `agent-seddon` today sits below both in breadth but has a clean
trait-seam architecture and a genuinely differentiated observability stack.

---

## TL;DR

We have a correct, well-architected **core loop** with three tools
(`bash`/`read_file`/`write_file`), one OpenAI-compatible provider, sliding-window
context trimming, a layered file-memory, and — our standout — **production-grade
observability** (Prometheus metrics + ClickHouse event/log/usage streaming) that
*neither reference harness ships out of the box*. The gap to a full-featured coding
agent is breadth, not soundness. The four things to fix first: **(1)** a proper
`edit` tool plus `grep`/`find`/`ls` (we can't do surgical code edits today);
**(2)** an **Anthropic-native provider** and streaming; **(3)** an interactive
**TUI with session resume**; **(4)** **summarizing compaction** (today we only drop
old turns, losing information). MCP, subagents, and skills follow once the coding
fundamentals are solid.

---

## The three harnesses at a glance

| | **agent-seddon** (us) | **pi** | **hermes-agent** |
|---|---|---|---|
| Language | Rust | TypeScript | Python (+ TS UIs) |
| Scale | ~2.9k LOC, 8 crates | TS monorepo, 5 packages | ~40k+ core LOC, ~900 test files |
| Philosophy | Trait seams, config-swappable | Minimal core + extensions | Batteries-included |
| Maturity | Early / experimental | Production, polished | Production, sprawling |
| Standout strength | **Observability (Prometheus + ClickHouse)** | Provider breadth + TUI + branching | Tools/providers/surfaces breadth + multi-agent |
| Providers | 1 | 40+ | 27 |
| Tools | 3 | ~8 | ~94 |
| UI surfaces | CLI only | Rich TUI + print/JSON/RPC/SDK | CLI + TUI + web + desktop |

---

## Feature comparison matrix

Coverage rubric (our column): ✅ Full · 🟡 Partial · 🟦 Seam only (trait defined, no
impl) · ❌ Missing · ➖ N/A.

| Feature area | agent-seddon | pi | hermes | Our coverage |
|---|---|---|---|---|
| Agent loop (assemble→call→tools→record) | Yes | Yes | Yes | ✅ |
| Streaming completions | No (buffered) | Yes | Yes | ❌ |
| Parallel tool execution | No (sequential) | Yes | Yes | ❌ |
| Steering / follow-up while running | No | Yes | Yes | ❌ |
| `bash` tool | Yes | Yes | Yes | ✅ |
| `read_file` / `write_file` | Yes | Yes | Yes | ✅ |
| `edit` (surgical/diff edits) | No | Yes | Yes (`patch`) | ❌ |
| `grep` / `find` / `ls` | No | Yes | Yes (`search_files`) | ❌ |
| Web search / fetch | No | Via extension | Yes | ❌ |
| Browser automation | No | No (external) | Yes | ❌ |
| LLM providers | 1 (OpenAI-compat) | 40+ | 27 | 🟡 |
| Anthropic-native provider | No | Yes | Yes | 🟦 |
| Provider capability metadata | Yes (basic) | Yes (rich, cost) | Yes | 🟡 |
| Context assembly | Yes | Yes | Yes | ✅ |
| Compaction | Truncation only | LLM summary | LLM summary | 🟡 |
| Session branching | No | Yes (`/tree`) | Partial | ❌ |
| Working / episodic / semantic memory | Yes (layered) | Sessions only | MEMORY+USER files | ✅ |
| Memory recall | Keyword | ➖ | FTS5 + LLM + vector plugins | 🟡 |
| Distillation (episodic→semantic) | No-op v1 | ➖ | Curator | 🟦 |
| Prometheus metrics | Yes | No | No | ✅ |
| Structured telemetry sink (ClickHouse) | Yes | Adapter interface | Trace upload | ✅ |
| MCP client | Yes (stdio + HTTP) | No (by design) | Yes | ✅ |
| MCP server | No | No | Yes | ❌ |
| Permission / approval gate | Yes (auto/interactive) | No (trust model) | Yes (rich) | 🟡 |
| Path-traversal safety on file tools | Yes | — | — | ✅ |
| Sandboxed execution backends | No | Docs/patterns | 6 backends | ❌ |
| Subagents / delegation | Yes (`delegate`, depth-capped) | Extension | Yes + kanban | ✅ |
| Session persistence / resume | Append-only log; no resume UX | Yes (JSONL + `/resume`) | Yes (SQLite) | 🟡 |
| Interactive TUI | No | Yes | Yes | ❌ |
| Slash commands | No | Yes | Yes | ❌ |
| Skills (SKILL.md) | No | Yes | Yes | ❌ |
| Plugins / extensions | Compile-time seams | Yes (hot-reload TS) | Yes (19 plugin types) | 🟡 |
| Hooks | No | Yes (events) | Yes | ❌ |
| Config system | TOML | JSON | YAML | ✅ |
| User context files (project rules) | Yes (`context.d/`) | Skills/templates | `.hermes/context` | ✅ |
| Multi-platform messaging | No | No | 19 platforms | ➖ |
| Cron / scheduled runs | No | No | Yes | ❌ |
| Test suite | Unit + Nix checks | vitest | ~17k pytest | 🟡 |

---

## Per-area deep dive

Coverage percentages below are rough judgments of "how much of a full-featured
harness's version of this do we have," not precise measurements.

### Agent loop / execution model — **~70%**
Our loop is sound and well-factored: `crates/agent-runtime/src/agent.rs` runs
assemble → complete → policy-gated tool dispatch → record → compact → repeat, with
metrics on every path. What's missing versus pi (`packages/agent/src/agent-loop.ts`)
and hermes (`run_agent.py`) is **streaming** (we buffer the whole completion — the
`LlmProvider` trait even sketches a `stream()` method in DESIGN.md §4.1 but the impl
buffers), **parallel tool execution** (pi runs tool calls concurrently; we run them
one at a time), and **steering / follow-up** (interrupting or queueing work while the
agent runs). *Verdict:* solid foundation; needs streaming + parallelism to feel modern.

### Tools — **~35%**
We ship three: `bash`, `read_file`, `write_file` (`crates/agent-tools/src/lib.rs`,
via `default_tools()`), with real path-traversal protection, output capping (12 KB),
and a 120 s bash timeout. The critical gap for a *coding* agent is the lack of an
**`edit` tool** — today the model must rewrite whole files via `write_file`, which is
token-expensive and error-prone. pi has `read/write/edit/bash/ls/find/grep`; hermes
has `patch` + `search_files` plus ~90 others. Note DESIGN.md §4.2 lists `search` as a
v1 tool, but it is **not implemented** — `default_tools()` returns only three.
*Verdict:* the single biggest functional gap. `edit` + `grep`/`find`/`ls` are table
stakes.

### LLM providers — **~40% breadth, 100% of what a single provider needs**
One impl: `OpenAiCompatProvider` (`crates/agent-providers/src/openai_compat.rs`),
tested against GLM, with reasoning-content handling, configurable base URL/model,
and optional insecure-TLS for dev servers. The trait (`LlmProvider`) is the right
seam and exposes `capabilities()`. But pi supports 40+ providers with cost/token
metadata and OAuth flows; hermes 27. Most urgent: an **Anthropic-native provider**
(DESIGN.md §4.1 lists it as planned; §9 recommends wrapping the `genai` crate for
breadth). *Verdict:* architecture is right, breadth is thin; Anthropic-native is the
priority add.

### Context management / compaction — **~45%**
`SlidingWindow` (`crates/agent-context/src/lib.rs`) assembles `[system, user,
(system-append)]` messages, folding in `context.d/` blocks and recalled memory, and
compacts by **dropping the oldest non-system messages** until under budget (with a
guard against orphaned tool results). This is honest truncation — it is *not*
summarization, and information is simply lost when it triggers. Both references do
**LLM-based summarizing compaction** (pi: `packages/coding-agent/src/core/compaction/`
with configurable `reserveTokens`/`keepRecentTokens` and iterative summaries; hermes:
`agent/context_compressor.py`). pi additionally does **branch summarization** for
`/tree` navigation. *Verdict:* functional but lossy; a summarizing compactor (already
anticipated in DESIGN.md §4.4) is a meaningful quality upgrade.

### Memory — **~55%**
This is an area where our *design* is arguably ahead: a genuine 3-layer model
(`crates/agent-memory/src/lib.rs`) — in-memory working, append-only JSONL episodic
(replayable, never mutated), and markdown-with-frontmatter semantic. Recall is
keyword/recency scoring; `distill()` (episodic→semantic promotion) is an honest
**no-op in v1**. pi has only session storage (no curated semantic layer); hermes has
`MEMORY.md` + `USER.md` plus 8 pluggable memory backends including vector/dialectic
(honcho, mem0, etc.). *Verdict:* strong bones, but recall is naive and the learning
loop (distillation) is unbuilt — both are documented future seams.

### Telemetry / metrics / observability — **~90% (our moat)**
This is where we **lead**. `crates/agent-runtime/src/metrics.rs` exposes 10
Prometheus metrics (API calls, latency histograms, tokens, context size, tool calls,
iterations, runs, run duration, active gauge) over a `/metrics` endpoint with
optional Pushgateway push. `crates/agent-telemetry/` streams a full transaction
history to **ClickHouse** — three tables (`agent_events`, `agent_logs`, `agent_usage`
per `nix/clickhouse/schema.sql`), keyed by per-run `session_id`, via a batched
background writer that drops rows rather than blocking the loop if ClickHouse is
down. Neither pi nor hermes ships this: pi defines a vendor-neutral observability
*interface* (`packages/agent/docs/observability.md`) but leaves the sink to you;
hermes has trajectory/trace upload hooks. *Verdict:* a real differentiator — keep
investing here.

### MCP support — **0%**
Not implemented; not even a seam. hermes is a full **MCP client and server**
(`mcp_serve.py`, `tools/mcp_discovery.py`) with sampling support. pi deliberately
omits MCP (README: "build an extension that adds MCP support"). *Verdict:* missing;
an MCP client would unlock a large tool ecosystem cheaply and fits our `ToolRegistry`
seam naturally.

### Permissions / sandboxing / security — **~40%**
We have a `Policy` seam (`crates/agent-runtime/src/policy.rs`) with `AutoApprove` and
`Interactive` (stdin y/N) impls, plus lexical path-traversal protection and output/
time caps on tools. That's more built-in safety than pi (which has *no* permission
system — it relies on a project-trust model and tells you to containerize). hermes is
far richer: a large approval engine (`tools/approval.py`) with dangerous-command
detection, smart auto-approval, and **6 execution backends** (local/SSH/Docker/
Singularity/Modal/Daytona). *Verdict:* good primitives; no sandboxed execution and no
allowlist policy yet (DESIGN.md §4.5 mentions `AllowList` as planned).

### Subagents / orchestration — **~10% (seam only)**
DESIGN.md §4.5 describes the "boomerang" delegated-subtask pattern (parent spawns
child with isolated context, gets a summary back) but there is no implementation. pi
ships a subagent extension example; hermes has full delegation
(`tools/delegate_tool.py`) with batch/async workers, depth control, and a kanban
board for coordination. *Verdict:* a documented future seam; valuable but not before
coding fundamentals.

### Session management / persistence / resume — **~40%**
Every run gets a UUID `session_id`; the episodic JSONL log makes runs replayable in
principle. But there is **no resume UX** — you cannot `--continue` a prior session or
pick one from a list. pi has JSONL sessions with a tree structure (`id`/`parentId`),
`/resume`, and in-place branching; hermes uses SQLite (WAL + FTS5) with session
splitting/tagging and auto-reset. *Verdict:* the data model supports replay; the
user-facing resume/branch features are missing.

### CLI / TUI / UI surfaces — **~20%**
We are **CLI-only**: `agent [--config PATH] <goal>` (`crates/agent-cli/src/main.rs`),
logging to stderr and printing a final answer, plus the `/metrics` HTTP endpoint. No
interactive session, no streaming display. pi has a sophisticated differential-render
TUI (`packages/tui/`) plus print/JSON/RPC/SDK modes; hermes has CLI + Ink TUI + web
dashboard + Electron desktop. *Verdict:* the biggest *experience* gap — an
interactive TUI is needed for daily-driver use.

### Skills / plugins / extensions / hooks / slash commands — **~15%**
We have compile-time extensibility (add a `Tool` impl, a provider, a context/memory
strategy; wired by TOML via a registry) but **no runtime plugin/skill/hook/
slash-command system**. pi has hot-reloadable TS extensions, SKILL.md skills, prompt
templates, and rich event hooks; hermes has 19 plugin types, built-in + optional
skills, and a curator that auto-maintains them. *Verdict:* our seams are real but
compile-time only; runtime extensibility (starting with skills) is a large gap.

### Configuration — **~85%**
A clean, type-safe single-file TOML (`crates/agent-runtime/src/config.rs`) with
sections for agent/provider/memory/tools/telemetry/metrics/context-files, three-tier
API-key resolution (inline > env > file), and tilde expansion. Comparable in quality
to pi's JSON settings and far smaller than hermes's 1500-line YAML. *Verdict:* strong
and appropriately scoped; grows naturally as features land.

### User context files — **✅ done well**
`context.d/prepend/*.md` and `context.d/append/*.md` are always-injected, numerically
ordered project instructions (`crates/agent-runtime/src/context_files.rs`) — analogous
to hermes's `.hermes/context`. A nice, simple feature we already have.

### Testing — **~50% for our size**
Unit tests across crates (path safety, context assembly, metrics encoding, row
serialization) plus Nix flake checks (clippy `-D warnings`, rustfmt, `cargo test`,
cargo-audit). Proportionate to a 2.9k-LOC codebase, but far from pi's vitest suites
or hermes's ~17k tests. *Verdict:* good hygiene; needs integration/e2e coverage as
features grow.

---

## Where we already lead

These are worth protecting and extending as deliberate differentiators:

1. **Observability.** First-class Prometheus metrics + ClickHouse transaction/log/
   usage streaming, best-effort and non-blocking. Neither reference harness ships a
   turnkey metrics + queryable-history stack. This is a genuine moat for anyone who
   wants to *measure and compare* agent runs.
2. **Clean trait-seam architecture.** Every major component is an `async` trait wired
   by config (DESIGN.md §4–5). Swapping provider/memory/context is a one-line TOML
   edit — ideal for A/B experimentation and for adding the features below without
   touching the loop.
3. **Rust.** Performance, memory safety, single static binary, no runtime/venv.
4. **Reproducible tooling.** Modular Nix flake (dev shell, checks, ClickHouse
   container) gives deterministic builds and CI.

The strategic read: keep pi's *discipline* (small core, swappable impls — which we
already have) while closing the coding fundamentals that both pi and hermes ship.

---

## Prioritized roadmap

Effort key: **S** ≈ hours–1 day · **M** ≈ a few days · **L** ≈ 1–2 weeks.
Items marked *(seam exists)* are already anticipated in DESIGN.md, so they build on
existing intent rather than contradicting it.

### P0 — core coding parity (do first)

| Feature | Current | Target | Effort | Why now |
|---|---|---|---|---|
| `edit` tool | Missing | Structured/diff-based line edits w/ preview | M | Whole-file rewrites are the #1 correctness+cost problem; blocks real coding |
| `grep` / `find` / `ls` tools | Missing (`search` unbuilt) | Three read tools, gitignore-aware | S–M | Agents can't navigate a codebase without these |
| Anthropic-native provider *(seam exists)* | Only OpenAI-compat | First-class Anthropic (tools, thinking, cache) | M | Best coding models; DESIGN.md §4.1 already plans it |
| Streaming completions *(seam exists)* | Buffered | Real `stream()` impl end-to-end | M | Responsiveness; prerequisite for a good TUI |
| Parallel tool execution | Sequential | Concurrent tool calls per turn | M | Matches pi/hermes; big latency win |

### P1 — usability / daily-driver

| Feature | Current | Target | Effort | Why now |
|---|---|---|---|---|
| Interactive TUI | CLI one-shot | Streaming multi-turn TUI | L | Biggest experience gap for daily use |
| Session resume / `--continue` | Log only, no UX | Resume + list past sessions | M | Episodic log already supports replay; just needs UX |
| Summarizing compaction *(seam exists)* | Truncation (lossy) | LLM summary, keep head/tail | M | Stop losing context; DESIGN.md §4.4 anticipates it |
| Slash commands | None | `/model`, `/compact`, `/resume`, etc. | M | Standard control surface; pairs with TUI |

### P2 — extensibility

| Feature | Current | Target | Effort | Why now |
|---|---|---|---|---|
| MCP client | None | Connect stdio/HTTP MCP servers as tools | M | Unlocks a large tool ecosystem via the `ToolRegistry` seam |
| Subagent / delegation *(seam exists)* | Seam only | Boomerang delegated subtasks | L | DESIGN.md §4.5; enables decomposition |
| Skills (SKILL.md) | None | Load + progressive-disclosure skills | M | Portable across harnesses; cheap capability packs |
| Distillation pipeline *(seam exists)* | No-op | Episodic→semantic promotion | M | Activates the memory model we already designed |
| `AllowList` policy *(seam exists)* | auto/interactive | Pattern allowlist | S | DESIGN.md §4.5; safer unattended runs |

### P3 — breadth

| Feature | Current | Target | Effort | Why now |
|---|---|---|---|---|
| Multi-provider via `genai` wrapper *(§9)* | 1 provider | ~26 providers behind our trait | M | DESIGN.md §9 recommends exactly this |
| Web search / fetch tool | None | Built-in web tools | M | Common coding need (docs lookup) |
| Embedding-based recall *(seam exists)* | Keyword | Vector semantic store (Qdrant/LanceDB) | L | Better memory recall; DESIGN.md §9 |
| Sandboxed execution backend | None | Docker backend for bash | L | Safety for untrusted repos; hermes-style |

---

## Philosophy note

pi and hermes represent opposite bets. pi keeps a **small core** and pushes MCP,
subagents, and plan-mode into installable extensions — betting that a lean,
composable base ages better. hermes bundles **everything** — betting that breadth in
core is what users actually want. `agent-seddon` already has pi's structural
discipline (trait seams + config wiring), which means we can pursue hermes-like
breadth *incrementally* without bloating the loop: each roadmap item lands behind an
existing seam. The recommended posture: **keep the core small and swappable, close
the coding fundamentals (P0/P1) that both references ship, and lean into
observability as our differentiator.**
