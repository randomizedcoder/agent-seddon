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
| [`features-comparison.md`](features-comparison.md) | You want the full comparison against pi, hermes-agent and opencode (dated snapshot) |
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

## Components

One doc per seam or subsystem. The **config key** column is what you set in
[`../config/agent.toml`](../config/agent.toml) to swap the implementation.

### The loop

| Component | Config key | Doc |
|---|---|---|
| Runtime and loop | — | [`runtime.md`](components/runtime.md) |
| Tools | `[tools] enabled` | [`tools.md`](components/tools.md) |
| Policy (approval gate) | `[agent] policy` | [`policy.md`](components/policy.md) |
| Context assembly | `[agent] context` | [`context.md`](components/context.md) |
| Hooks | `[hooks] enabled` | [`hooks.md`](components/hooks.md) |

### Model and provider

| Component | Config key | Doc |
|---|---|---|
| Providers | `[agent] provider` | [`providers.md`](components/providers.md) |
| Model routing and failover | `[router]` | [`router.md`](components/router.md) |
| Prompt-cache placement | `[cache] strategy` | [`prompt-cache.md`](components/prompt-cache.md) |
| Tokenizer and cost | `[tokenizer] backend` | [`tokenizer.md`](components/tokenizer.md) |
| Structured output | `[structured] validator` | [`structured-output.md`](components/structured-output.md) |
| Multimodal content | — | [`multimodal.md`](components/multimodal.md) |

### Memory, search and code

| Component | Config key | Doc |
|---|---|---|
| Memory (episodic + semantic) | `[memory] backend`, `semantic` | [`memory.md`](components/memory.md) |
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

## The parity program

Thirty per-feature specs in [`parity/`](parity/README.md). Each was written by
reading the corresponding implementation **and its test suite** in pi,
hermes-agent and opencode, then laying out an `#[rstest]` plan intended to match
them — so each spec doubles as an honest record of where the peers were ahead.

They are design-of-record: written before implementation, and the status table in
[`parity/README.md`](parity/README.md) tracks which shipped, which are partial, and
what remains. The accumulated open follow-ups are at the foot of that page.

## Conventions

Testing conventions, the security model, and the per-PR shape live in
[`../CLAUDE.md`](../CLAUDE.md). The short version: tests are table-driven
`#[rstest]` with `positive_` / `negative_` / `corner_` / `boundary_` prefixes and
**mandatory** `adversarial_` cases for untrusted input; the gate is
`nix flake check`.
