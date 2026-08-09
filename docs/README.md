# agent-seddon documentation

Everything in `docs/`, grouped by what you are trying to do. Three entry points:

- **Understanding the design** → [`../DESIGN.md`](../DESIGN.md), then
  [`architecture.md`](architecture.md).
- **Running it** → [`operating.md`](operating.md), then the observability guides.
- **Changing it** → [`extending.md`](extending.md), then the component doc for the
  seam you are replacing.

## Start here

| Doc | Read this if |
|---|---|
| [`../DESIGN.md`](../DESIGN.md) | You want the rationale — why the seams are where they are, the loop, the layered memory model |
| [`architecture.md`](architecture.md) | You want the boundary map: which crate owns what |
| [`extending.md`](extending.md) | You want to add a provider, tool, memory, context strategy, policy or transport |
| [`features-comparison.md`](features-comparison.md) | You want the full comparison against pi, hermes-agent, opencode and codex (dated snapshot) |
| [`../CLAUDE.md`](../CLAUDE.md) | You are contributing — conventions, the security model, the PR shape |

## Operating it

| Doc | Covers |
|---|---|
| [`operating.md`](operating.md) | Config reference, API-key precedence, the REPL and its slash commands, `context.d/`, runtime state, the Nix apps |
| [`observability.md`](observability.md) | The three signals together — metrics, traces, logs — and how the agent inspects itself |
| [`metrics.md`](metrics.md) | Prometheus + Grafana runbook, single-process and distributed |
| [`tracing.md`](tracing.md) | OpenTelemetry + ClickStack runbook, including the two-process distributed trace |
| [`grpc.md`](grpc.md) | Running seams as services: contract, transports, health, security warnings, and why three seams are deliberately not distributed |
| [`benchmarking.md`](benchmarking.md) | The performance and leak gate — iai-callgrind ceilings and dhat budgets |
| [`eval.md`](eval.md) | Evaluating the agent with promptfoo — the `nix run .#eval` quality harness and `nix run .#redteam` security harness (opt-in, model-graded) |
| [`swebench.md`](swebench.md) | Benchmarking the agent on SWE-bench — `nix run .#swebench` drives it to patch a real repo, then Docker-grades the patch against `FAIL_TO_PASS`/`PASS_TO_PASS` (opt-in; % resolved) |
| [`inspect.md`](inspect.md) | Grading the agent with UK AISI's Inspect AI — `nix run .#inspect` drives it through a custom solver over hermetic tasks or any `inspect_evals` benchmark (opt-in; % score) |
| [`openai-evals.md`](openai-evals.md) | Grading the agent with OpenAI Evals — `nix run .#openai-evals` drives it through a custom completion function over a hermetic registry eval or any registry eval (opt-in; % accuracy) |
| [`swe-agent.md`](swe-agent.md) | The SWE-agent comparison baseline — `nix run .#swe-agent` runs Princeton's scaffold with the same model on the same SWE-bench instances and grades with the swebench harness, for a resolved% next to our agent (opt-in) |
| [`eval-all.md`](eval-all.md) | Running the whole eval/benchmark family in one shot — `nix run .#eval-all` orchestrates inspect/openai-evals/eval/redteam/swebench/swe-agent and prints a comparison table (opt-in) |

## Components

One doc per seam or subsystem. The **config key** column is what you set in
[`../config/agent.toml`](../config/agent.toml) to swap the implementation.

### The loop

| Component | Config key | Doc |
|---|---|---|
| Runtime and loop | — | [`runtime.md`](components/runtime.md) |
| Tools | `[tools] enabled` | [`tools.md`](components/tools.md) |
| Policy (approval gate) | `[agent] policy` | [`policy.md`](components/policy.md) |
| Verifier (tool-call correctness gate) | `[verifier] backend` | [`verifier.md`](components/verifier.md) |
| Task-mode detection | `[mode] classifier` | [`mode.md`](components/mode.md) |
| Context assembly (incl. mode-aware) | `[agent] context` | [`context.md`](components/context.md) |
| Hooks | `[hooks] enabled` | [`hooks.md`](components/hooks.md) |

### Model and provider

| Component | Config key | Doc |
|---|---|---|
| Providers | `[agent] provider` | [`providers.md`](components/providers.md) |
| Model routing and failover | `[router]` | [`router.md`](components/router.md) |
| LLM/GPU pool (load-balanced) | `[pool]` | [`pool.md`](components/pool.md) |
| Prompt-cache placement | `[cache] strategy` | [`prompt-cache.md`](components/prompt-cache.md) |
| Tokenizer and cost | `[tokenizer] backend` | [`tokenizer.md`](components/tokenizer.md) |
| Structured output | `[structured] validator` | [`structured-output.md`](components/structured-output.md) |
| Multimodal content | — | [`multimodal.md`](components/multimodal.md) |

### Memory, search and code

| Component | Config key | Doc |
|---|---|---|
| Memory (episodic + semantic) | `[memory] backend`, `semantic` | [`memory.md`](components/memory.md) |
| Dimensional memory (per-step, by dimension) | `[dimensions] store` | [`dimensions.md`](components/dimensions.md) |
| Embedder | `[embedder] backend` | [`embedder.md`](components/embedder.md) |
| Code search | `[agent] search` | [`search.md`](components/search.md) |
| Git (multi-branch) | `[git]` | [`git.md`](components/git.md) |
| Language servers | `[lsp] backend` | [`lsp.md`](components/lsp.md) |
| `@`-reference expansion | `[reference] backend` | [`reference.md`](components/reference.md) |

### Session and workflow

| Component | Config key | Doc |
|---|---|---|
| Session checkpoints | `[session] backend` | [`session.md`](components/session.md) |
| Session export | `[session_export]` | [`session-export.md`](components/session-export.md) |
| Task / plan tracking | `[tasks] backend` | [`tasks.md`](components/tasks.md) |
| Scheduler | `[scheduler]` | [`scheduler.md`](components/scheduler.md) |
| Skill authoring | `[skills] write` | [`skill-authoring.md`](components/skill-authoring.md) |

### Platform and execution

| Component | Config key | Doc |
|---|---|---|
| Sandbox (`bash` backend) | `[sandbox] backend` | [`sandbox.md`](components/sandbox.md) |
| PTY (interactive terminals) | `[pty] backend` | [`pty.md`](components/pty.md) |
| Web fetch | `[web] backend` | [`web-fetch.md`](components/web-fetch.md) |
| Web search | `[web_search] backends` | [`web-search.md`](components/web-search.md) |
| Forge (GitHub / GitLab) | `[forge] backend` | [`forge.md`](components/forge.md) |
| Content scanner | `[scanner] rules` | [`scanner.md`](components/scanner.md) |
| MCP (client and server) | `[[mcp.servers]]` | [`mcp.md`](components/mcp.md) |
| Retry and backoff | `[provider] max_retries` | [`retry.md`](components/retry.md) |

### Cross-cutting

| Component | Doc |
|---|---|
| Testing conventions | [`testing.md`](components/testing.md) |
| Benchmarking and leak gate | [`benchmarking.md`](components/benchmarking.md) |

## Design notes

Per-track design documents in [`design/`](design/) — the input to a feature's
implementation phase, distinct from the shipped `components/` docs. Each carries a
`STATUS.md` tracking which increments have merged; the tracks below are largely
**implemented** (see each `STATUS.md`), while `tool-call-verification.md` and
`portal/` remain forward-looking (the latter **design / pre-implementation**).

| Doc | About |
|---|---|
| [`tool-call-verification.md`](design/tool-call-verification.md) | A measured, multi-model gate that inspects a tool call before it runs (Allow / Revise / Deny), records every verdict to ClickHouse, and learns which verifier to trust per task type |
| [`code-review/`](design/code-review/README.md) | The **Code Review Flow**: detect a review task, then a parallel fan-out of mostly-deterministic collectors (file set, diff, Go static analysis, AST/call-graph, style, git state) builds a *grounded* fact-bundle a model can't hallucinate over — driven by a health-checked pool of cheap LLMs, deeply instrumented for duration + parallel-optimization accounting. A 12-doc set with a status table |
| [`adaptive-cognition/`](design/adaptive-cognition/README.md) | **Adaptive Cognition**: spend cheap local LLMs (GLM-5.2 + MI50) on the agent's own meta-cognition — per-turn **mode** detection with a switch decision, **mode-aware compaction** that reshapes context on a switch (a before/after table of what to keep/shed/pull-in), and **dimensional memory** that summarizes each step *by dimension* into per-dimension histories. The mode switch is the pivot joining all three. Supersedes `code-review/mode-detection.md` |
| [`gpu-pool/`](design/gpu-pool/README.md) | The **GPU/LLM pool**: extend the health-checked `LlmPool` into a capacity-aware load balancer across many local targets — in-flight-aware selection policies (least-loaded / weighted / round-robin), per-target concurrency cap + fail-soft backpressure, and latency-graded `healthy \| degraded \| dead` routing. Shipped component: [`pool.md`](components/pool.md) |
| [`model-router/`](design/model-router/README.md) | The **Model Router & Upstream Registry**: evolve the pool from a local-GPU load balancer into a **task-aware router over 10–50 upstream LLMs**. Each upstream carries a **model card** (per-upstream context window, cost, latency, capability tags); a declarative **`RoutePolicy`** you author + live health/load signals decide the pick; a `RouteHint` (TaskMode · role · request requirements · explicit override) rides each request; and the fleet migrates from TOML to a gRPC **`ProviderRegistryService`** control plane (CRUD + swappable storage), with TOML kept as a back-compat seed. Extends `gpu-pool/`. **Design / pre-implementation** |
| [`loadtest/`](design/loadtest/README.md) | **Overload/backpressure conformance + seam load harness**: a uniform admission layer so every seam sheds overload with `RESOURCE_EXHAUSTED` + a `grpc-retry-pushback-ms` hint (the client already backs off + clamps), plus an opt-in load/conformance harness that drives each service into overload, asserts the contract, and reports throughput/latency. **In progress** |
| [`portal/`](design/portal/README.md) | The **Agent Portal**: a small Flutter, **gRPC-only** app that launches the observability UIs, lets the operator **see + CRUD every prompt per mode** (system · context.d pre/post-pends · externalized per-mode compaction lenses), and gives a live **agent view** — loop narration + a status bar (mode · context size · GPU pool · gRPC p50/p99). Adds three seams (`PromptService`, `AgentSessionService`, `MetricsProxyService`) + Dart codegen. **Design / pre-implementation** |
| [`prompts/`](design/prompts/README.md) | The **Prompt Library**: a pre-created, editable library of prompt fragments **selected by the situation** — a base always applied plus **tagged fragments** chosen when the situation supplies their tags (`mode:review` today; `language:`/`tier:`/… as the signals land), composed additively behind a cache-stable base. Stored behind a **swappable backend** (`file` git-legible default · embedded `sqlite` · `grpc`→central catalog), all behind the portal's one `PromptService` CRUD. Realises the portal's deferred *per-mode system prompts* item; drafts the content for all six modes and contrasts hermes/pi/opencode. **Design / pre-implementation** |
| [`multi-session/`](design/multi-session/README.md) | **Multi-session / multi-user**: make every seam serve many users and concurrent sessions over the existing gRPC architecture — a `(user_id, session_id)` identity that rides gRPC **metadata** (hybrid with typed fields for `SessionRegistry`/`SessionStore`/confined cwd), per-user memory tenancy, a `Backend`/`Session`/`SessionManager` split, fixes for two single-loop state hazards, and session-attributed traces + a curated per-session metric label. Isolation via `confine`/`safe_segment` holds even against a spoofed identity; **authentication is a named follow-up**, not this track. An 8-doc set with a status table. **Design / pre-implementation** |

## The parity program

Fifty per-feature specs in [`parity/`](parity/README.md). Each was written by
reading the corresponding implementation **and its test suite** in the peer
harnesses — pi, hermes-agent, opencode, and (for specs 31–50) **codex**, the
~130-crate Rust agent that is by far the deepest peer — then laying out an
`#[rstest]` plan intended to match them, so each spec doubles as an honest record of
where the peers were ahead.

Specs **01–30** are shipped; specs **31–50** are the **codex-driven depth** round —
design-of-record only, not yet built. They are design-of-record: written before
implementation, and the status table in [`parity/README.md`](parity/README.md)
tracks which shipped, which are partial, and what remains. The accumulated open
follow-ups are at the foot of that page.

## Conventions

Testing conventions, the security model, and the per-PR shape live in
[`../CLAUDE.md`](../CLAUDE.md). The short version: tests are table-driven
`#[rstest]` with `positive_` / `negative_` / `corner_` / `boundary_` prefixes and
**mandatory** `adversarial_` cases for untrusted input; the gate is
`nix flake check`.
