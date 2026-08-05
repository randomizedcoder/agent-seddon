# Features Comparison: agent-seddon vs. pi vs. hermes-agent vs. opencode vs. codex

**Original analysis:** 2026-07-17 · **Last refreshed:** 2026-07-22, after the
30-spec parity programme and the gRPC distribution work (7 → 22 served seams) ·
**codex added as a fourth peer** this round (the codex-driven depth pass, parity
specs 31–50).
**Status:** Capability inventory vs. four reference harnesses + remaining roadmap.

> The original document was written before the plugin registry + P0 work. It has
> since been refreshed: the matrix, the per-area notes, and the roadmap below all
> reflect what is **actually implemented today** — plugin registry, `edit` /
> `grep` / `find` / `ls` tools, **full-text code search** (tantivy) and
> **multi-branch git** tools (9 `git_*`), an Anthropic-native provider, real
> streaming and parallel tool execution, summarizing compaction, an MCP client
> (stdio + HTTP) and server (`--serve-mcp`), subagent `delegate`, an interactive
> REPL with session resume + slash commands + rustyline history, and skills. This
> pass also adds **opencode** as a fourth comparison column and a dedicated
> **coding-fundamentals deep dive** (editing / skills / tool calling). A later
> pass adds **codex** (OpenAI's ~130-crate Rust agent) as a fifth column and the
> **codex-driven depth** round (parity specs 31–50); its cells are source-audited
> against the read-only clone at `codex/codex-rs/`.

## Purpose

`agent-seddon` is an experimental, Rust-based coding-agent harness. This document
inventories its capabilities against four mature open-source harnesses and states
honestly what we have, how complete it is, and what remains. The framing intent:
**grow `agent-seddon` into a full-featured coding agent** — a daily driver, not a
research toy. The four yardsticks:

- **[pi](https://github.com/earendil-works/pi)** — a TypeScript monorepo with a
  deliberately *minimal core* (no built-in MCP or subagents) but exceptional
  breadth elsewhere: 40+ LLM providers, a polished differential-rendering TUI,
  session branching, LLM-summarizing compaction, and a first-class
  extension/skill/theme system.
- **[hermes-agent](https://github.com/NousResearch/hermes-agent)** — a large
  Python "batteries-included" harness: ~94 tools, 27 provider plugins, 8 memory
  backends, MCP client *and* server, subagents + a kanban coordination board,
  19 messaging-platform gateways, 4 UI surfaces, and multiple sandboxing backends.
- **[opencode](https://github.com/anomalyco/opencode)** — a TypeScript/Bun
  monorepo (~34 packages) built on the **Effect** effect system. The most direct
  peer on *coding fundamentals*: it ships **both** a surgical `edit` and an
  `apply_patch` (unified-diff) tool, line-ending/BOM-safe editing with stale-file
  detection, a **model-invocable `skill` tool**, an LSP subsystem, MCP, 13+
  providers, build/plan/general agents, a permission system, and TUI + desktop +
  web UIs.
- **[codex](https://github.com/openai/codex)** — the Rust maximalist: OpenAI's
  ~130-crate Cargo workspace, **Responses-API-only** on the wire, a per-OS sandbox
  trio (macOS **seatbelt** / Linux **landlock + seccomp** / **bwrap** / Windows
  **AppContainer**-style restricted token), a multi-agent **orchestration graph**
  (`agent-graph-store`, spawn/wait/close), **code-mode** (the model writes code
  that calls tools), a hooks engine (11 lifecycle points), a Starlark `execpolicy`
  command-safety DSL, and a **SQLite-backed rollout/resume/fork** session model.
  Deepest peer by far; source-audited against `codex/codex-rs/`.

pi = disciplined minimalism, hermes = maximalism, opencode = a polished
fundamentals-first daily driver, codex = native-Rust maximalism (OS sandboxing +
multi-agent orchestration + code-mode). `agent-seddon` now covers the coding
fundamentals these peers ship, still sits below them in raw breadth (providers,
tools, UI surfaces), and has a genuinely differentiated observability +
distributed-seam stack. Where it is *behind* is worth naming too: sandbox
confinement and learned embeddings are the notable unbuilt backends — codex's
four-way OS sandbox makes the first the sharpest gap — while real tokenization
now ships (exact BPE / local `tokenizer.json` backends, see below).

---

## TL;DR

The core loop is sound and the coding fundamentals are in: **29 built-in tools**
(`bash`/`read_file`/`write_file`/`edit`/`grep`/`find`/`ls`, full-text `search`,
self-inspection `metrics`, and 9 multi-branch `git_*`) plus MCP, two providers
(OpenAI-compatible + Anthropic-native) with real SSE **streaming**, **parallel**
tool execution, **summarizing** *or* truncating compaction, a layered file-memory,
an interactive **REPL** (history, slash commands, session resume), **skills**,
**subagent delegation**, and an **MCP client + server**. Our standout remains
**production-grade observability** (Prometheus metrics + ClickHouse event/log/usage
streaming) plus **distributed gRPC seams**, which none of the four reference
harnesses ships out of the box (codex ships OpenTelemetry tracing + OTLP metrics,
but not a queryable transaction history or config-swappable gRPC seams).

What's left is mostly breadth and a handful of backends that are seams with
placeholder implementations: sandbox confinement, learned embeddings,
model-invocable skill loading, more providers, a full-screen TUI, and activating
the distillation (episodic→semantic) pipeline. See **Remaining** below, which is
the maintained list.

---

## The four harnesses at a glance

| | **agent-seddon** (us) | **pi** | **hermes-agent** | **opencode** | **codex** |
|---|---|---|---|---|---|
| Language | Rust | TypeScript | Python (+ TS UIs) | TypeScript / Bun | Rust |
| Scale | 15 crates | TS monorepo, 5 packages | ~40k+ core LOC, ~900 test files | TS monorepo, ~34 packages | ~130-crate Cargo workspace |
| Philosophy | Trait seams, config-swappable | Minimal core + extensions | Batteries-included | Fundamentals-first, Effect-based | Native-Rust maximalist, Responses-only |
| Maturity | Experimental, fundamentals complete | Production, polished | Production, sprawling | Production, polished | Production, deep |
| Standout strength | **Observability + distributed seams** | Provider breadth + TUI + branching | Tools/providers/surfaces breadth + multi-agent | **Editing (edit + patch + LSP) + multi-UI** | **OS sandboxing + multi-agent + code-mode** |
| Providers | 2 (OpenAI-compat + Anthropic) | 40+ | 27 | 13+ | 4 built-in + user-defined (Responses-only) |
| Tools | 18 built-in + MCP | ~8 | ~94 | 12 built-in + MCP | small native set (`apply_patch`/`shell`/`update_plan`/`web_search`) + MCP |
| Test framework | `#[rstest]` + iai + dhat | vitest | pytest | Bun/vitest | cargo `#[test]` + insta snapshots |
| UI surfaces | CLI + interactive REPL | Rich TUI + print/JSON/RPC/SDK | CLI + TUI + web + desktop | CLI + TUI + desktop + web | CLI + TUI + app-server/IDE |

---

## Feature comparison matrix

Coverage rubric (our column): ✅ Full · 🟡 Partial · 🟦 Seam only (trait defined, no
impl) · ❌ Missing · ➖ N/A. The **codex** column is **source-audited** against the
read-only clone at `codex/codex-rs/` (🟦 "Seam only" is an agent-seddon-specific
verdict and does not apply to the peers).

| Feature area | agent-seddon | pi | hermes | opencode | codex | Our coverage |
|---|---|---|---|---|---|---|
| Agent loop (assemble→call→tools→record) | Yes | Yes | Yes | Yes | Yes | ✅ |
| Streaming completions | Yes (SSE, both providers) | Yes | Yes | Yes | Yes (SSE, Responses API) | ✅ |
| Parallel tool execution | Yes (concurrent per turn) | Yes | Yes | Yes | Yes (`tools/parallel.rs`) | ✅ |
| Steering / follow-up while running | No | Yes | Yes | Yes (`question` + bg jobs) | Yes (interrupt + queued `write_stdin`) | ❌ |
| Multi-turn session (REPL) | Yes | Yes | Yes | Yes | Yes (TUI) | ✅ |
| `bash` tool | Yes | Yes | Yes | Yes | Yes (`shell`/`exec_command`) | ✅ |
| `read_file` / `write_file` | Yes | Yes | Yes | Yes (`read`/`write`) | ❌ (reads via `shell`, writes via `apply_patch`) | ✅ |
| `edit` (surgical string replace) | Yes | Yes | Yes (`patch`) | Yes | ❌ (no surgical editor) | ✅ |
| Patch/diff (unified) edit tool | Yes (`apply_patch`, fuzzy chain) | No | Yes (`patch`) | Yes (`apply_patch`) | ✅ (`apply_patch`, add/update/delete — the sole editor) | ✅ |
| `grep` / `find` / `ls` | Yes (gitignore-aware) | Yes | Yes (`search_files`) | Yes (`grep`/`glob`) | ❌ (via `shell` ripgrep) | ✅ |
| Full-text indexed code search | Yes (tantivy `search`) | No | No | No (ripgrep only) | No (fuzzy filename only) | ✅ |
| Multi-branch git tools (revision-addressed) | Yes (9 `git_*` + worktrees/checkpoint) | No | No | No (git via `bash`) | No (git via `shell`) | ✅ |
| LSP integration | Yes (diagnostics + navigation + `rename`) | No | Diagnostics only | Navigation only | No | ✅ |
| Structured task list (todos) | Yes (`todo_write`, plan progress as a metric) | No | Yes (kanban) | Yes (`todowrite`) | Yes (`update_plan`) | ✅ |
| Web search / fetch | Yes (`web_fetch` + `web_search`, cached, SSRF-guarded) | Via extension | Yes | Yes (`websearch`/`webfetch`) | 🟡 hosted `web_search` (Responses API); no local `web_fetch` | ✅ |
| Browser automation | No | No (external) | Yes | No | No | ❌ |
| LLM providers | 2 (OpenAI-compat + Anthropic) + a `Router` seam for failover | 40+ | 27 | 13+ | 🟡 4 built-in (OpenAI/Bedrock/Ollama/LM Studio) + Azure + user-defined, all Responses-only | 🟡 |
| Provider capability metadata | Yes (basic) | Yes (rich, cost) | Yes | Yes | 🟡 model/context window, no pricing | 🟡 |
| Context assembly | Yes | Yes | Yes | Yes | Yes | ✅ |
| Compaction | Truncation **and** LLM summary | LLM summary | LLM summary | LLM summary | LLM summary (+ remote-compaction fallback) | ✅ |
| Session branching | Yes (content-addressed checkpoints: branch/undo/fork) | Yes (`/tree`) | Partial | No | Yes (`codex fork`) | ✅ |
| Working / episodic / semantic memory | Yes (layered) | Sessions only | MEMORY+USER files | Sessions (SQLite) + AGENTS.md | Yes (two-phase episodic→semantic pipeline, `~/.codex/memories/`) | ✅ |
| Memory recall | Keyword + vector, hybrid RRF fusion | ➖ | FTS5 + LLM + vector plugins | ➖ | Yes (cited memories + ripgrep + SQLite index) | 🟡 |
| Distillation (episodic→semantic) | Seam only (no-op stub) | ➖ | Curator | ➖ | ✅ (Phase-2 consolidation via model) | 🟦 |
| Prometheus metrics | Yes | No | No | No | 🟡 (OTel/OTLP metrics, not Prometheus) | ✅ |
| Structured telemetry sink (ClickHouse) | Yes | Adapter interface | Trace upload | No | 🟡 (OTLP → any collector + analytics) | ✅ |
| MCP client | Yes (stdio + HTTP) | No (by design) | Yes | Yes (stdio + HTTP) | Yes (`rmcp-client`; resources/prompts/OAuth/SSE) | ✅ |
| MCP server | Yes (`--serve-mcp`, stdio) | No | Yes | No | Yes (`mcp-server`) | ✅ |
| Distributed components (run seams as services) | Yes — 22 seams over TCP/UDS + `--serve-all`, reflection + health | No | No | No | 🟡 (app-server/exec-server/proxies — not config-swappable seams) | ✅ |
| Distributed tracing | Yes (OpenTelemetry/OTLP → ClickStack) | No | Trace upload | No | Yes (OTLP, W3C propagation) | ✅ |
| Permission / approval gate | Yes (auto/interactive) | No (trust model) | Yes (rich) | Yes (rich, per-agent) | Yes (tiered `AskForApproval` + granular + guardian) | 🟡 |
| Path-traversal safety on file tools | Yes | — | — | Yes (Location-scoped) | Yes (OS-sandbox-scoped) | ✅ |
| Sandboxed execution backends | `Sandbox` seam; `nix` backend is **reproducibility, not confinement** | Docs/patterns | 6 backends | Partial (codemode) | ✅ seatbelt / landlock+seccomp / bwrap / Windows AppContainer | 🟡 |
| Subagents / delegation | Yes (`delegate`, depth-capped) | Extension | Yes + kanban | Yes (build/plan/general agents) | Yes (`agent-graph-store` graph; spawn/wait/close) | ✅ |
| Session persistence / resume | Yes (JSONL + `--continue`/`--resume`/`/resume`) | Yes (JSONL + `/resume`) | Yes (SQLite) | Yes (SQLite) | Yes (JSONL + SQLite index; `resume --last`/`fork`) | ✅ |
| Interactive REPL / TUI | REPL (line-based, rustyline) | Rich TUI | Rich TUI | Rich TUI | Rich TUI | 🟡 |
| Slash commands | Yes | Yes | Yes | Yes | Yes | ✅ |
| Skills (SKILL.md) | Yes (`/skill:<name>` load, user-invoked) | Yes | Yes | Yes (model-invocable `skill` tool) | Yes (model-invocable `skill` tool + remote sources) | ✅ |
| Plugins / extensions | Compile-time seams + MCP tools + skills | Yes (hot-reload TS) | Yes (19 plugin types) | Yes (hot-reload plugins) | Yes (plugin marketplace: add/upgrade/remote) | 🟡 |
| Hooks | Yes (5 lifecycle points, one typed seam) | Yes (events) | Yes | Yes (plugin events) | Yes (11 points incl. pre/post-compact) | ✅ |
| Interactive terminals (PTY) | Yes (bounded rolling buffer, cursor reads) | No | Yes (`ptyprocess`) | Yes (`Pty.Service`) | Yes (`tty` on `exec_command`) | ✅ |
| Forge (PRs / issues / reviews) | Yes — GitHub **and** GitLab behind one seam | No | Partial | No | No (`gh` via `shell`) | ✅ |
| Structured output | Validates **and** repairs (bounded) | Validates tool args | Validates, raises | Validates tool I/O | 🟡 (Responses `response_format`, no repair) | ✅ |
| Multimodal content (image/document) | Yes (typed content blocks) | Yes | Yes | Yes | Yes (`view_image`, image input) | ✅ |
| Prompt-cache breakpoint placement | Yes (`CacheStrategy` seam) | Yes (Anthropic) | Partial | Yes | 🟡 (`prompt_cache_key`, server-side) | ✅ |
| Content scanner feeding the policy gate | Yes (secrets + injection, severity → `Decision`) | No | Partial | No | 🟡 (secret sanitizer + guardian/execpolicy) | ✅ |
| `@`-reference expansion in prompts | Yes (`@file`/`@dir`/`@symbol`/`@url`, budgeted) | Yes | No | Yes | 🟡 (`@file` fuzzy completion) | ✅ |
| Session export | Yes (deterministic render, scanner-redacted) | No | Partial | No | 🟡 (JSONL rollouts on disk, no render) | ✅ |
| Cross-session recall | Yes (`session_recall` — FTS over past transcripts via the `DocumentSource` seam) | No | FTS5 recall | No | Yes (ripgrep over rollouts + memories) | ✅ |
| Config system | TOML | JSON | YAML | JSON | TOML (+ named profiles) | ✅ |
| User context files (project rules) | Yes (`context.d/`) | Skills/templates | `.hermes/context` | Yes (`AGENTS.md`) | Yes (`AGENTS.md`) | ✅ |
| Multi-platform messaging | No | No | 19 platforms | No | No | ➖ |
| Cron / scheduled runs | Yes (`schedule` tool + `--scheduler` driver) | No | Yes | No | No (cloud-tasks, no local cron) | ✅ |
| Test suite | Table-driven `#[rstest]` + instruction-count benches + dhat leak gate | vitest | ~17k pytest | Bun/vitest | cargo `#[test]` + insta snapshots (large) | 🟡 |

---

## Coding fundamentals — deep dive

The matrix above is broad. This section zooms in on the three things a coding
agent lives or dies by — **code editing, skills, and tool calling** — at finer
grain. Cells use the same rubric (✅ Full · 🟡 Partial · ❌ Missing · ➖ N/A);
**—** means *not assessed at this grain*. The `agent-seddon`, `opencode`, and
`codex` columns are **source-audited**; the `pi` and `hermes` columns reflect their
documented tool surface, so their fine-grained cells are deliberately sparse.

> **Going deeper:** for the top-10 fundamentals, per-feature parity specs in
> [`parity/`](parity/) mine each peer's *test suite* and lay out table-driven test
> plans to match and exceed them — execution-ready detail behind this matrix.

### Code editing

opencode is the clear leader here: it ships **two** editors (`edit` +
`apply_patch`) and a fully hardened write path (line-ending, BOM, stale-file). Our
`edit` (`crates/agent-tools/src/edit.rs`) is a clean but *minimal* exact-string
replace — the biggest single gap this comparison surfaces.

| Capability | agent-seddon | pi | hermes | opencode | codex |
|---|---|---|---|---|---|
| Exact string-replace (`old`→`new`) | ✅ | ✅ | ✅ | ✅ | ❌ (no surgical editor) |
| Unique-match guard (errors on ambiguous match) | ✅ | — | — | ✅ | ➖ (no string editor) |
| Replace-all option | ✅ (`replace_all`) | — | — | ✅ (`replaceAll`) | ➖ |
| Unified-diff / patch tool | ❌ | ❌ | ✅ (`patch`) | ✅ (`apply_patch`, add/update/delete) | ✅ (`apply_patch` — the sole editor) |
| Multi-file edit in one call | ❌ | — | 🟡 (`patch`) | 🟡 (`apply_patch` spans files) | ✅ (`Vec<Hunk>` spans files) |
| Line-ending (CRLF/LF) preservation | ❌ | — | — | ✅ | — |
| UTF-8 BOM preservation | ❌ | — | — | ✅ | — |
| Stale-file detection (changed since read) | ❌ | — | — | ✅ | 🟡 (context match rejects stale) |
| Fuzzy / whitespace-tolerant match | ❌ | — | — | ❌ (planned) | 🟡 (context-line fuzz) |
| LSP-assisted edits / diagnostics | ❌ | ❌ | ❌ | 🟡 (LSP present, not wired into edit) | ❌ |
| Format-on-save | ❌ | — | — | ❌ (planned) | ❌ |
| Snapshot / undo | ❌ | — | 🟡 | ❌ (planned) | 🟡 (`turn_diff_tracker` net diff) |
| Path-traversal / scope safety | ✅ | — | — | ✅ (Location-scoped) | ✅ (OS-sandbox-scoped) |

### Skills

Both agent-seddon and opencode read `SKILL.md` with frontmatter and progressive
disclosure. The decisive difference: **who loads a skill.** Ours are **user-driven**
(`/skills`, `/skill:<name>` in the REPL — `crates/agent-runtime/src/skills.rs`);
opencode exposes a **`skill` tool the model itself calls** to pull a capability in
mid-task, plus URL/embedded sources and per-agent permission filtering.

| Capability | agent-seddon | pi | hermes | opencode | codex |
|---|---|---|---|---|---|
| `SKILL.md` discovery (dir + flat file) | ✅ | ✅ | ✅ | ✅ | ✅ |
| YAML frontmatter (name / description) | ✅ | ✅ | ✅ | ✅ | ✅ |
| Progressive disclosure (load body on demand) | ✅ | ✅ | — | ✅ | ✅ |
| Model-invocable (agent loads via a tool) | ❌ (user `/skill:<name>`) | — | — | ✅ (`skill` tool) | ✅ (`skill` tool + `allow_implicit_invocation`) |
| Skill sources: dir / URL / embedded | Dir only | — | — | ✅ (dir + URL + embedded) | ✅ (dir + remote + bundled) |
| Per-agent permission filtering | ❌ | — | — | ✅ | ✅ (`skill_approval`) |
| Bundled files / scripts referenced | ❌ | — | — | ✅ | ✅ |
| Slash-command exposure | ✅ | ✅ | — | ✅ (optional `slash`) | 🟡 |

### Tool calling

The core mechanics are at parity — a `Tool` trait + registry, JSON-schema params,
parallel dispatch, per-call approval, output caps. We even lead on a per-tool
`parallel_safe()` flag. opencode's edges are ergonomic: a **large-output → managed
file** fallback and a per-tool **`toModelOutput` projection** that shapes results
for the model.

| Capability | agent-seddon | pi | hermes | opencode | codex |
|---|---|---|---|---|---|
| Tool trait + registry | ✅ | ✅ | ✅ | ✅ | ✅ |
| JSON-Schema parameter validation | ✅ | ✅ | ✅ | ✅ (Effect schema) | ✅ |
| Parallel tool execution | ✅ | ✅ | ✅ | ✅ | ✅ |
| Per-tool parallel-safety flag | ✅ (`parallel_safe()`) | — | — | — | — |
| Per-call approval / permission gate | ✅ (Policy seam) | ❌ (trust) | ✅ | ✅ (per-agent) | ✅ (tiered + guardian) |
| Output size caps | ✅ (12 KB) | — | — | ✅ (bounded) | ✅ (`tool_output_token_limit`) |
| Large-output → file fallback | ❌ | — | — | ✅ (managed file) | — |
| Custom model-output projection | ❌ | — | — | ✅ (`toModelOutput`) | — |
| Dynamic MCP tools at runtime | ✅ | ❌ | ✅ | ✅ | ✅ (MCP + `tool_search` + code-mode) |

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
Eighteen built-ins (per `config/agent.toml`), all registered through the plugin
registry and gated by cargo features: `bash`, `read_file`, `write_file`
(`tool-core`), `edit` (`tool-edit`, unique/`replace_all` string replace),
`grep`/`find`/`ls` (`tool-search`, gitignore-aware via ripgrep's `ignore` crate),
full-text `search` (`search`, the tantivy `SearchBackend` seam), self-inspection
`metrics` (`tool-metrics`), and nine multi-branch `git_*` tools (`tool-git`, the
`RepoBackend` seam: `git_read`/`git_tree`/`git_diff`/`git_grep`/`git_log`/
`git_branches`/`git_status`/`git_worktree`/`git_checkpoint`). All share lexical
path-traversal protection and output caps. MCP servers add more at runtime as
`mcp_<server>_<tool>`. The remaining tool gaps are **editing depth** (no
unified-diff/patch tool — see the deep dive; hermes and opencode both ship one)
and web/browser tools (hermes has ~90 tools total).

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
`crates/agent-metrics` exposes Prometheus metrics over a `/metrics` endpoint (+
optional Pushgateway) — loop-level counters plus per-seam series recorded by a
metrics wrapper (`crates/agent-runtime/src/metered.rs`), scraped by a
Nix-deployed Prometheus/Grafana stack with a per-component dashboard
([`docs/metrics.md`](metrics.md)). `crates/agent-telemetry/` streams a
full transaction history to **ClickHouse** — three tables (`agent_events`,
`agent_logs`, `agent_usage`), keyed by per-run `session_id`, via a batched
background writer that drops rows rather than blocking if ClickHouse is down.
On top of that, **OpenTelemetry tracing**: the loop is instrumented as a span tree
and exported over OTLP; W3C context propagates across gRPC seam boundaries, so a
distributed run reassembles into **one trace** in a ClickStack/HyperDX collector
(see [`tracing.md`](tracing.md)). Neither pi nor hermes ships a turnkey metrics +
queryable-history + distributed-tracing stack; **codex** does ship OpenTelemetry
tracing (OTLP, W3C propagation) + OTLP/Statsig metrics, but not Prometheus scraping
or a queryable ClickHouse-style transaction history.

### Distributed components (gRPC) — a differentiator
Because every seam is a config-selected trait, a component can run as its own
process/container: `agent-proto` (binary protobuf contracts) + `agent-grpc`
(per-seam servers/clients over TCP or unix domain sockets) let the loop dial a
remote seam with `= "grpc"`, hosted by `agent --serve-<seam>` (see
[`grpc.md`](grpc.md)). **22 of the 26 seams** are served today, plus a
`--serve-all` gateway; every server carries reflection and the standard
`grpc.health.v1` health service. Three seams are deliberately not distributed —
their primary operation is a synchronous pure function, so a network hop would
cost more than the work; the reasoning is recorded in `grpc.md`.

None of the four peers offers this config-swappable-seam model out of the box
(codex runs an `app-server`/`exec-server`/proxy split as separate processes, but
its components are not interchangeable gRPC seams selected from config). But be
precise about what
exists: the contract, the transports, the health checks and the tests are real —
including an end-to-end test that runs the loop with four seams remote at once —
while **container images, orchestration manifests and any multi-host deployment
are not built**. Everything runs over loopback or a unix socket today. Calling it
a "k8s-style topology" (as an earlier draft of this document did) promised more
than the code delivers.

### MCP — client and server
`crates/agent-mcp` is an MCP **client** (stdio subprocess + streamable HTTP behind
an `McpTransport` trait): it runs `initialize`, discovers tools (`tools/list`), and
registers each into the same `ToolRegistry` as the built-ins. `agent --serve-mcp`
(`crates/agent-cli/src/mcp_server.rs`) is the **server** side — exposes a single
`run` tool so another MCP client can drive the whole agent loop. Matches hermes and
codex (which ship both an `rmcp-client` and an `mcp-server`, and go deeper on
resources/prompts/OAuth/SSE — see parity spec 43); pi deliberately omits MCP.

### Permissions / sandboxing / security — good primitives
A `Policy` seam (`AutoApprove`, `Interactive`) plus lexical path-traversal
protection and output/time caps. More built-in safety than pi (trust model only),
far less than hermes (dangerous-command detection + 6 execution backends) and far
less than **codex**, whose four-way OS sandbox (seatbelt / landlock+seccomp / bwrap
/ Windows AppContainer), Starlark `execpolicy` DSL, tiered `AskForApproval`, an LLM
`guardian` gate, and a `network-proxy` egress allow-list make it the security
benchmark this document measures against (parity specs 34–37, 46, 48). Remaining:
an `AllowList` policy and a sandboxed (Docker/OS) execution backend.

### Subagents / orchestration — implemented
`crates/agent-runtime/src/subagent.rs`: with `[agent] subagents = true`, a
`delegate` tool spawns a child agent from the same components, runs it in isolated
context, and returns only the summary (the boomerang pattern), depth-bounded by
`subagent_max_depth`. hermes goes further with batch/async workers + a kanban board;
codex goes further still with a persisted **orchestration graph** (`agent-graph-store`
parent/child spawn edges + `spawn`/`wait`/`interrupt`/`close_agent` tools) — the
anchor for parity spec 31.

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
(`/skills`, `/skill:<name>`), and slash commands. Two gaps stand out. First,
**skill loading is user-driven, not model-invocable**: the model can't pull a
skill in mid-task the way opencode's `skill` *tool* lets it (and we load only
local-directory skills, not URL/embedded sources — see the skills deep dive).
Second, still missing vs. pi/hermes/opencode: hot-reloadable extensions and
lifecycle hooks.

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
3. **Rust.** Memory safety and a single static binary with no runtime or venv —
   and, because there is no GC or JIT in the way, instruction-count benchmarks
   stable enough to gate a build on.
4. **Reproducible tooling.** Modular Nix flake (dev shell, checks, containers).
5. **A quality gate that includes performance and memory.** 17 iai-callgrind
   benches with hard instruction ceilings and 14 dhat leak budgets, wired into
   `nix flake check` alongside clippy/fmt/tests/audit/buf. No peer gates on either.
   Caveat: it runs locally — there is no CI.
6. **Specified against the peers before being built.** The 50 specs in
   `docs/parity/` were each written by reading the peers' implementation *and their
   tests* — the top-30 against pi/hermes/opencode, and the codex-driven depth round
   (31–50) against codex — which is also why this document can cite source paths.

---

## Roadmap

### Shipped since the original analysis (2026-07-17 → 2026-07-22)
Plugin registry + cargo-feature gating · `edit` / `grep` / `find` / `ls` tools ·
**full-text code search** (tantivy `SearchBackend`) · **multi-branch git** (9
`git_*` tools, the `RepoBackend` seam) · **self-inspection `metrics` tool** ·
**per-component Prometheus metrics** + Grafana stack · **distributed gRPC seams**
(`--serve-<seam>`) · **OpenTelemetry tracing** (OTLP → ClickStack) ·
Anthropic-native provider · streaming (both providers) · parallel tool execution ·
summarizing compaction · MCP client (stdio + HTTP) · MCP server (`--serve-mcp`) ·
subagent `delegate` · interactive REPL (rustyline history) · session resume · slash
commands · skills.

Then the 30-spec parity programme: `apply_patch` · web fetch + search · **LSP**
(diagnostics, navigation **and** `rename`) · the `Sandbox` seam · vector search with
hybrid RRF fusion · structured output **with bounded repair** · `@`-reference
expansion · a content **scanner** feeding the policy gate · content-addressed
**session checkpoints** (branch/undo/fork) + export · **todos** · **lifecycle
hooks** · tokenizer/cost accounting · **prompt-cache** placement · **model routing**
and failover · **multimodal** content blocks · **forge** (GitHub + GitLab) ·
**scheduler** · **PTY** · skill authoring. Then the distribution programme took
served gRPC seams from 7 to **22**, added `--serve-all`, health checking, and an
end-to-end test running the loop with four seams remote at once.

### Remaining

Effort key: **S** ≈ hours–1 day · **M** ≈ a few days · **L** ≈ 1–2 weeks.

| Feature | Current | Target | Effort |
|---|---|---|---|
| ~~**Real tokenization**~~ ✅ | **Shipped**: exact OpenAI-family BPE (`tiktoken`, offline `tiktoken-rs`) + a local model's `tokenizer.json` (`hf`, HuggingFace) behind the `Tokenizer` seam, feature-gated with per-backend gate checks; unmapped models fall back to `approx` | done (#211, #212) | ✅ |
| **Sandbox with teeth** | The `nix` backend gives reproducibility, **not confinement** (`network_off: false`) | Network/mount enforcement via sandboxed derivations or a container backend | L |
| **Real embeddings** | `LocalEmbedder` is feature hashing, not a learned model | A hosted or local model behind the `Embedder` seam | M |
| Deployment for distributed seams | Transport, health and tests exist; nothing to deploy with | Container images + a compose/k8s example | M |
| CI | `nix flake check` run locally by hand | The gate on every PR, with a badge | S |
| Model-invocable skill loading | User `/skill:<name>` | A `skill` tool the model calls mid-task (as opencode) | S |
| Line-ending / BOM-safe editing | Plain UTF-8 rewrite | CRLF/LF + BOM preservation + stale-file detection | S |
| Fuzzy hunk matching in `apply_patch` | Exact + line-oriented fuzzy chain | Offset-tolerant hunk application | M |
| Distillation pipeline *(seam exists)* | No-op stub | Episodic→semantic promotion via the model | M |
| Secret-path write deny-list | Guard screens dangerous paths; no explicit deny-list | hermes-style deny-list in `Policy` | S |
| More providers | 2 hand-rolled + a `Router` | `genai`-style wrapper for breadth (DESIGN.md §9) | M |
| Full-screen TUI | Line-based REPL | Differential-render multi-turn TUI | L |
| Steering / follow-up | None | Interrupt / queue work mid-run | M |

#### Codex-driven depth — parity specs 31–50

The codex peer surfaces twenty seam-shaped gaps beyond the top-30. Each is a
⬜ spec-written, four-peer design-of-record in [`parity/`](parity/) (codex is the
anchor for most), to be built one PR each. Effort keys use the same S/M/L scale.

| # | Feature | Effort |
|---|---|---|
| 31 | [sub-agent orchestration graph](parity/31-subagent-graph.md) | L |
| 32 | [elicitation / `request_user_input`](parity/32-elicitation.md) | S |
| 33 | [tool-search / dynamic tool catalog](parity/33-tool-search.md) | M |
| 34 | [OS-level sandbox backends](parity/34-os-sandbox.md) | L |
| 35 | [`execpolicy` command-safety DSL](parity/35-execpolicy.md) | M |
| 36 | [unified-exec escalation](parity/36-unified-exec.md) | M |
| 37 | [network egress policy / proxy](parity/37-network-policy.md) | M |
| 38 | [shell env snapshot](parity/38-shell-snapshot.md) | S |
| 39 | [SQLite rollout / resume-last / fork](parity/39-rollout-sqlite.md) | M |
| 40 | [turn-diff tracker + apply](parity/40-turn-diff-apply.md) | M |
| 41 | [structured cited memories](parity/41-cited-memories.md) | M |
| 42 | [pre/post-compact hooks + remote fallback](parity/42-compact-hooks.md) | M |
| 43 | [MCP depth (resources/prompts/OAuth/SSE)](parity/43-mcp-depth.md) | L |
| 44 | [config profiles + runtime flags](parity/44-config-profiles.md) | S |
| 45 | [doctor / self-diagnostics](parity/45-doctor.md) | S |
| 46 | [guardian — runtime action safety gate](parity/46-guardian.md) | M |
| 47 | [reasoning-effort / thinking controls](parity/47-reasoning-controls.md) | M |
| 48 | [granular / tiered approval](parity/48-granular-approval.md) | M |
| 49 | [fuzzy file-search / `@`-completion](parity/49-file-search.md) | S |
| 50 | [secret store / keyring credentials](parity/50-secret-store.md) | M |

---

## Philosophy note

pi keeps a **small core** and pushes MCP, subagents, and plan-mode into installable
extensions; hermes bundles **everything**; opencode leads on editing fundamentals;
codex goes deepest on native-Rust systems concerns (OS sandboxing, multi-agent
orchestration, code-mode). `agent-seddon` has pi's structural
discipline (trait seams + config wiring) — so it can pursue hermes-like breadth
*incrementally*, each item landing behind an existing seam. The posture that got us
here and should continue: keep the core small and swappable, close the fundamentals
the references ship, and lean into observability as the differentiator.

Two honest notes on that posture. The breadth gaps that remain are real — provider
count, UI surfaces, and tool volume are all still well behind. And the rows where
this project leads are mostly *structural* (seams, transports, instrumentation,
gates) rather than *capability* rows; that is a deliberate bet, not an accident,
but it should not be mistaken for being ahead overall.
