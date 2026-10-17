# agent-seddon documentation

The [project README](../README.md) summarizes available features. This index links
the operating guides, component references, and full design catalog. Three entry points:

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
| [`peer-harnesses.md`](reference/peer-harnesses.md) | You want the peer source/prompt map and reproducible source references used in comparisons |
| [`../CLAUDE.md`](../CLAUDE.md) | You are contributing — conventions, the security model, the PR shape |

## Operating it

| Doc | Covers |
|---|---|
| [`operating.md`](operating.md) | Config reference, API-key precedence, the REPL and its slash commands, `context.d/`, runtime state, the Nix apps |
| [`deployment-l2.md`](deployment-l2.md) | The concrete l2 deployment topology (a diagram + port reference): the two gateways, the Envoy grpc-web bridge, the task-router LLM fleet, and the native/podman observability stack, reached over the LAN |
| [`portal-obs-mcp-runbook.md`](portal-obs-mcp-runbook.md) | "Don't relearn this" runbook for driving the l2 portal + obs stack with the Playwright MCP: what runs where, the `--dart-define` endpoints, bringing the stack up, driving the CanvasKit UI, per-service auth, and OTLP wiring |
| [`llm-endpoints.md`](llm-endpoints.md) | The dev LLM endpoints — GLM-5.2 (SGLang/MI300X, judge + reasoning critic), Kimi-K3 (generator), qwen3 (local critic/distiller): models, TLS/auth, thinking/tool-call config, and the `AGENT_E2E_*` env that points at them |
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
| Model routing, failover and upstream registry | `[router]`, `[route]` | [`router.md`](components/router.md) |
| LLM/GPU pool (load-balanced) | `[pool]` | [`pool.md`](components/pool.md) |
| Generator/critic consensus | `[agent] provider = "consensus"` | [`consensus.md`](components/consensus.md) |
| Prompt-cache placement | `[cache] strategy` | [`prompt-cache.md`](components/prompt-cache.md) |
| Tokenizer and cost | `[tokenizer] backend` | [`tokenizer.md`](components/tokenizer.md) |
| Structured output | `[structured] validator` | [`structured-output.md`](components/structured-output.md) |
| Multimodal content | — | [`multimodal.md`](components/multimodal.md) |

### Memory, search and code

| Component | Config key | Doc |
|---|---|---|
| Memory (episodic + semantic) | `[memory] backend`, `semantic` | [`memory.md`](components/memory.md) |
| Dimensional memory (per-step, by dimension) | `[dimensions] store` | [`dimensions.md`](components/dimensions.md) |
| Background digests | `[digest]` | [`digest.md`](components/digest.md) |
| Instant compaction | `[agent] context = "instant-window"` | [`instant-compaction.md`](components/instant-compaction.md) |
| Cognition graphs and parallel branches | `[graph]`, `--cognition-graph` | [`graph.md`](components/graph.md) |
| Embedder | `[embedder] backend` | [`embedder.md`](components/embedder.md) |
| Code search | `[agent] search` | [`search.md`](components/search.md) |
| Code graphs and structural search | `[ast]`; optional `ast-scip` feature | [`ast.md`](components/ast.md) |
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
| Campaigns (`agent campaign …`) | `[campaign] store` | [`campaigns.md`](components/campaigns.md) |
| Review fleet | `[review_fleet]`, `--serve-fleet` | [`review-fleet.md`](components/review-fleet.md) |
| Skill authoring | `[skills] write` | [`skill-authoring.md`](components/skill-authoring.md) |

### Platform and execution

| Component | Config key | Doc |
|---|---|---|
| Sandbox (`bash` backend) | `[sandbox] backend` | [`sandbox.md`](components/sandbox.md) |
| PTY (interactive terminals) | `[pty] backend` | [`pty.md`](components/pty.md) |
| Web fetch | `[web] backend` | [`web-fetch.md`](components/web-fetch.md) |
| Web search | `[web_search] backends` | [`web-search.md`](components/web-search.md) |
| Forge (GitHub / GitLab; optional Gitea / Bitbucket) | `[forge] backend` | [`forge.md`](components/forge.md) |
| Content scanner | `[scanner] rules` | [`scanner.md`](components/scanner.md) |
| MCP (client and server) | `[[mcp.servers]]` | [`mcp.md`](components/mcp.md) |
| Retry and backoff | `[provider] max_retries` | [`retry.md`](components/retry.md) |

### Portal / management seams

Management and observation services used by the [Agent Portal](design/portal/README.md).
Prompt changes also feed subsequent agent sessions; apply/restart behavior is documented
per component. The column below identifies the service flag or configuration block.

| Component | Service / config | Doc |
|---|---|---|
| Prompt management | `--serve-prompt`, `[prompts]` | [`prompt.md`](components/prompt.md) |
| Config management | `--serve-config`, `[grpc.config]` | [`config.md`](components/config.md) |
| Metrics proxy (PromQL over gRPC) | `--serve-metrics-proxy`, `[metrics_proxy]` | [`metrics-proxy.md`](components/metrics-proxy.md) |
| Live agent-session feed | `--serve-session-stream`, `[grpc.session_stream]` | [`agent-session.md`](components/agent-session.md) |

### Cross-cutting

| Component | Doc |
|---|---|
| Testing conventions | [`testing.md`](components/testing.md) |
| Benchmarking and leak gate | [`benchmarking.md`](components/benchmarking.md) |
| Multi-tenancy coverage audit | [`mt-audit.md`](components/mt-audit.md) |
| Doc-link + discoverability gate | [`doc-links.md`](components/doc-links.md) |
| Cognition-graph value harness (`nix run .#graph-arena`) | [`graph-arena.md`](graph-arena.md) |

## Gap analysis

The [gap analysis](gap-analysis/README.md) records the designed-versus-implemented
review and its closing work. Security, API coverage, routing, and documentation
have progressed since the original audit; use the linked implementation logs for
current scope. The [self-improvement analysis](gap-analysis/self-improvement.md)
motivated [repository knowledge](design/repo-knowledge/README.md), whose storage
foundations are implemented, and [campaigns](design/campaigns/README.md), whose
planner, CLI, worker, PR poller, and observability are available. Their remaining
integrations are still tracked separately.

## Design notes

Design documents explain intent and tradeoffs; component guides describe how to
use the implementation. The catalog below summarizes the available scope and links
implementation trackers where present. Some older design introductions and tracker
rows retain historical branch labels; consult component docs and the later as-built
logs rather than treating an entire design as an implemented feature.

### Agent capabilities

| Design | Implemented scope and remaining work | Tracker |
|---|---|---|
| [Adaptive cognition](design/adaptive-cognition/README.md) | Task-mode detection/switching, mode-aware compaction, and optional dimensional memory are implemented. | [Status](design/adaptive-cognition/STATUS.md) |
| [Code graph](design/code-graph/README.md) | Go type-aware graph queries, structural search, and optional SCIP support; query capabilities depend on the engine. See [AST guide](components/ast.md). | [Status](design/code-graph/STATUS.md) |
| [Code review](design/code-review/README.md) | Grounded review with parallel collectors, static analysis, code structure, synthesis, and risk assessment. | [Status](design/code-review/STATUS.md) |
| [Cognition graph](design/cognition-graph/README.md) | Consensus, background digests, instant compaction, configurable graph documents, parallel branches, and graph-arena evaluation are implemented. Consensus streaming and error behavior are bounded in the [component guide](components/consensus.md). | [Status](design/cognition-graph/STATUS.md) |
| [GPU/LLM pool](design/gpu-pool/README.md) | Health-checked selection, capacity/backpressure, and latency-graded routing across local model targets. | [Status](design/gpu-pool/STATUS.md) |
| [Model router](design/model-router/README.md) | Task-aware routing and live upstream registry are implemented, including capacity, spillover, cost, and budget controls documented in the [router guide](components/router.md). | [Status](design/model-router/STATUS.md) |
| [Multi-session](design/multi-session/README.md) | Backend/session split, identity propagation, scoped memory, admission, and lifecycle primitives are implemented; the tracker records remaining state-keying and lifecycle wiring work. | [Status](design/multi-session/STATUS.md) |
| [Prompt library](design/prompts/README.md) | Mode-selected fragments, editable prompts, named personalities, and portal selection are implemented. Upstream prompt sourcing/refresh is pending; bespoke SQLite version history was retired during store convergence. | [Status](design/prompts/STATUS.md) |
| [Tool-call verification](design/tool-call-verification.md) | Optional correctness checks with allow/revise/deny verdicts, shadow/enforce modes, and recorded outcomes. See the [implemented verifier](components/verifier.md) for defaults and fail-open behavior. | Component guide |

### Workflows and repository knowledge

| Design | Implemented scope and remaining work | Tracker |
|---|---|---|
| [Campaigns](design/campaigns/README.md) | Hierarchical Postgres task store, planner, approval gates, CLI, worker-to-draft-PR path, merge polling, and observability. Repo-knowledge integration, campaign gRPC, and webhook follow-ups remain pending. | [Status](design/campaigns/STATUS.md) · [Progress](design/campaigns/PROGRESS.md) |
| [Review fleet](design/review-fleet/README.md) | Forge polling and Slack triggers, workspaces, reviews, persisted drafts, feedback, approval/posting, and observability are implemented; child-session inheritance remains deferred. See the [fleet guide](components/review-fleet.md). | [Status](design/review-fleet/STATUS.md) · [Progress](design/review-fleet/PROGRESS.md) |
| [Fleet grounding](design/fleet-grounding/README.md) | Worktree-rooted review inputs, repository indexing, grounding telemetry, and collector skip/failure reporting. | As-built notes in design |
| [Review analysis depth](design/review-analysis-depth/README.md) | Nix-provisioned Go/Rust analysis, parallel execution, deduplicated/risk-ranked digests, and overflow-gated model summarization. | [Status](design/review-analysis-depth/STATUS.md) |
| [Review parallelism](design/review-parallelism/README.md) | Experimental latency investigation, including chunked map-reduce proposals. Cross-review concurrency exists; proposed single-review optimizations are not a shipped-feature commitment. | [Evidence and status](design/review-parallelism/STATUS.md) |
| [Repository knowledge](design/repo-knowledge/README.md) | Graph model/store interface and Postgres storage are implemented (RK-01/02). Extractors, indexing CLI, agent tools, cited inventory, and briefing consumers remain pending. | [Status](design/repo-knowledge/STATUS.md) · [Progress](design/repo-knowledge/PROGRESS.md) |

### Configuration, security, and deployment

| Design | Implemented scope and remaining work | Tracker |
|---|---|---|
| [Unified configuration](design/config/README.md) | Bootstrap TOML plus transactional file/SQLite/Postgres config stores, tenant-scoped registries, durable scheduling, and forge/message-transport cards are implemented. The tracker records remaining consolidation work. | [Status](design/config/STATUS.md) |
| [Multi-tenancy](design/multi-tenancy/README.md) | Optional bubblewrap isolation, tenant-scoped data/configuration, ClickHouse policies and recall, and coverage audits. Org-tenancy integration remains partial; isolation depends on deployment/configuration. | [Status](design/multi-tenancy/STATUS.md) |
| [Security hardening](design/security-hardening/README.md) | OIDC/JWT login, RBAC, sessions, TLS/mTLS, auth audit, secret confinement, CLI/browser/desktop sign-in, credential reload, and portal Access controls are implemented. | [Status](design/security-hardening/STATUS.md) |
| [Forge authorization](design/forge-auth/README.md) | Optional GitHub App API-token minting and tenant installation-registry CRUD are implemented. Mint authority still comes from the operator allowlist; verified onboarding, clone credential injection, and the webhook receiver remain pending. | [Status](design/forge-auth/STATUS.md) |
| [Kubernetes deployment](design/k8s/README.md) | Nix images, renderer/GitOps, k3s deployment, certificate reload, and SPIFFE identity are implemented (K0–K5). Cluster edge/portal, sandbox sidecar, observability cut-over, and full HA target validation remain pending (K6–K9). | [Status](design/k8s/STATUS.md) |
| [REST + OpenAPI](design/rest-openapi/README.md) | Implemented annotations, generated OpenAPI contract, Envoy transcoding to the same authenticated gRPC handlers, integration checks, and benchmarks. gRPC remains primary. | [Status](design/rest-openapi/STATUS.md) |

### Operations, portal, and evaluation

| Design | Implemented scope and remaining work | Tracker |
|---|---|---|
| [Doctor](design/doctor/README.md) | Implemented `agent doctor` CLI and fleet `Preflight` RPC, including dependency and authentication probes. | Design and implementation notes |
| [Observability dimensions](design/observability/README.md) | Tenant/repository attribution across metrics, spans, and history, with explicit label-cardinality rules and metric/span censuses. | [Status](design/observability/STATUS.md) |
| [Portal](design/portal/README.md) | Implemented Flutter launcher, live agent view, prompts/personalities, Settings, Router, graph editor, review fleet, sign-in, and Access management. Later tracks extend the original portal design. | [Status](design/portal/STATUS.md) |
| [Portal GUI testing](design/portal-gui-testing/README.md) | Widget/contract checks, live integration harnesses, reports, performance tracking, and Envoy instrumentation. | [Status](design/portal-gui-testing/STATUS.md) |
| [Load testing](design/loadtest/README.md) | Admission/backpressure contract, service conformance/stress harnesses, full-loop concurrency, and wire-load measurements are implemented. | [Status](design/loadtest/STATUS.md) |
| [Cargo libnix experiment](design/libnix/README.md) | Opt-in libnix Cargo pin, development shell, dependency mapping, and smoke runner are implemented. Evaluation is ongoing; Crane remains the default build. | [Status](design/libnix/STATUS.md) · [Evaluation](design/libnix/EVALUATION.md) · [Feedback](design/libnix/FEEDBACK.md) |
| [Tokenization cache](design/tokenization-cache.md) | Deferred idea: memoize token counts behind a tokenizer decorator. | Design only |

## The parity program

The [parity specifications](parity/README.md) record feature designs and test plans
informed by pi, hermes-agent, opencode, and codex. Peer comparisons are dated source
snapshots, not current capability rankings.

Specs 01–30 cover the original feature set; specs 31–50 describe deeper follow-ups.
Some related capabilities have since landed through other tracks (for example,
`agent doctor` and cross-session recall), so the parity checklist alone is not a
complete implementation inventory. Use the component guides and the track summaries
above for available behavior; the parity index retains the original requirements
and accumulated follow-ups.

## Conventions

Testing conventions, the security model, and the per-PR shape live in
[`../CLAUDE.md`](../CLAUDE.md). The short version: tests are table-driven
`#[rstest]` with `positive_` / `negative_` / `corner_` / `boundary_` prefixes and
**mandatory** `adversarial_` cases for untrusted input; the gate is
`nix flake check`.
