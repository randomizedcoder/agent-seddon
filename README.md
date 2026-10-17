# agent-seddon

<p align="center">
  <img src="agent-seddon.png" alt="agent-seddon logo" width="300">
</p>

A coding agent in Rust, built so you can see what it is doing and change how it
does it. It edits code, runs tools, manages sessions, reviews pull requests, and
turns objectives into tasks that workers execute in separate worktrees.

Replaceable components sit behind named Rust traits, called **seams**. Configuration
selects their implementations; many can also run as gRPC services. Metrics, traces,
and stored history help explain which component acted and what happened. The same
architecture supports a local CLI, unattended review fleets, and a management portal.

This is a single-author experimental project. Features below are implemented unless
marked partial or experimental; availability still depends on build features,
configuration, and external services. Tests and the Nix gate run locally; the project
has no CI. Start with the [quickstart](#quickstart), browse the
[features](#features), or use the [full documentation index](docs/README.md).

## Quickstart

Install Nix with flakes enabled and run a local Ollama server. The flake supplies
the Rust toolchain, `protoc`, and development tools. The small example configuration
uses Ollama, so no hosted-model account is needed:

```sh
ollama pull llama3.1:latest
# Run `ollama serve` in another terminal if the server is not already running.
nix develop
cargo build
cargo run -p agent-cli -- --config config/local-ollama.toml \
  "write a hello world program in C called hello.c"
cargo run -p agent-cli -- --config config/local-ollama.toml   # no goal: REPL
```

The model must actually support tool calling. The
[Ollama example](config/local-ollama.toml) enables a small file/shell tool set and
auto-approves ordinary calls while retaining the policy guard. The
[annotated configuration](config/agent.toml) covers the broader system. Hosted
providers can use inline, environment, or file-based credentials; see the
[operating guide](docs/operating.md) for configuration, slash commands, and state
layout, and [provider documentation](docs/components/providers.md) for backends.

## Features

### Coding tools and integrations

**Files, commands, and git.** Read and write files, make guarded edits, apply
patches, and run shell commands through the configured execution backend.
[Interactive PTYs](docs/components/pty.md) persist across turns;
[git tools](docs/components/git.md) support branches, revision-addressed reads,
and disposable worktrees sharing an object database.
[Tool reference](docs/components/tools.md).

**Web and prompt references.** [Fetch pages](docs/components/web-fetch.md),
[search the web](docs/components/web-search.md), and expand
[`@file`, `@dir`, `@symbol`, and `@url` references](docs/components/reference.md)
into bounded context. Reference expansion routes through the search and web
components and screens imported content.

**MCP, skills, and delegation.** Configure MCP servers to add tools; the agent
also serves a single goal-running tool over stdio with `--serve-mcp`
([MCP](docs/components/mcp.md)). Invoke reusable skills with `/skill:<name>`,
optionally enable guarded [skill authoring](docs/components/skill-authoring.md),
or enable [subagents](docs/components/runtime.md) with isolated context and
bounded delegation depth.

### Code intelligence

**Search and language servers.** Gitignore-aware file search complements a
Tantivy full-text index and semantic search with hybrid result fusion
([search](docs/components/search.md)). [LSP integration](docs/components/lsp.md)
adds diagnostics, navigation, and rename; structural search matches AST patterns
across supported languages.

**Structural code graphs.** The Go engine answers symbol, implementation,
caller/callee, call-chain, and dependency queries. Optional SCIP support adds
cross-language symbols and references where an indexer is configured; its supported
queries differ from the Go engine.
[Component guide](docs/components/ast.md) ·
[code-graph design](docs/design/code-graph/README.md).

### Models, routing, and output

**Providers and output.** OpenAI-compatible and Anthropic-native providers support
streaming and tool calls. Typed [multimodal messages](docs/components/multimodal.md)
carry images and documents through supported providers, while
[structured output](docs/components/structured-output.md) validates responses and
can attempt bounded repairs.

**Routing and capacity.** Route requests by task mode, role, capabilities, context
window, and policy. The live upstream registry supports health/load-aware selection,
failover, circuit breakers, spillover, optional hard concurrency limits, cost
ceilings, and rolling spend budgets. A separate health-checked GPU/LLM pool offers
least-loaded, weighted, and round-robin selection.
[Router](docs/components/router.md) · [pool](docs/components/pool.md) ·
[routing design](docs/design/model-router/README.md).

**Caching and accounting.** [Prompt-cache placement](docs/components/prompt-cache.md)
keeps stable context reusable; [token and cost accounting](docs/components/tokenizer.md)
tracks usage. Shared [retry/backoff handling](docs/components/retry.md) bounds
transient-failure retries and server-provided delays.

### Memory and cognition

**Memory and task modes.** An append-only episodic log and curated semantic facts
support [recall and distillation](docs/components/memory.md). The agent detects its
task mode, switches as work changes, and can reshape context through the destination
mode's lens. Optional [dimensional memory](docs/components/dimensions.md) files
per-step summaries by topic for later recall.
[Adaptive-cognition design](docs/design/adaptive-cognition/README.md).

**Compaction and digests.** Choose sliding-window, summarizing, or mode-aware
[context strategies](docs/components/context.md). A
[background distiller](docs/components/digest.md) stores summaries and key facts;
[instant compaction](docs/components/instant-compaction.md) assembles context from
that ledger instead of generating a fresh summary on the critical path.

**Consensus and cognition graphs.** A generator/critic
[consensus provider](docs/components/consensus.md) can revise a response through
bounded feedback. Textproto [cognition graphs](docs/components/graph.md) connect
these engines and add parallel branches with all/any/quorum joins and comparison
or synthesis. Consensus checking requires non-streamed responses; critic errors
can fail open, so it is a quality mechanism with documented limits.
[Design and evaluation](docs/design/cognition-graph/README.md).

### Prompts and sessions

**Editable prompts and personalities.** Manage system prompts, context fragments,
mode lenses, and named personalities through a shared service and the portal.
Situational fragment selection currently uses mode tags. Personality changes apply
to the next assembled session; other edits follow their documented reload rules.
[Prompt guide](docs/components/prompt.md) ·
[prompt-library design](docs/design/prompts/README.md).

**Checkpoints, export, and recall.** [Session checkpoints](docs/components/session.md)
support resume, branching, undo, and forks of conversation state.
[Exports](docs/components/session-export.md) produce redacted Markdown, HTML, or
JSON. Cross-session recall is implemented, including a tenant-scoped ClickHouse
backend over redacted history
([data scoping](docs/design/multi-tenancy/02-data-scoping-and-rls.md)).

**Concurrent sessions.** Shared backends and separate session state support
concurrent users, identity propagation, scoped memory, capacity admission, and
lifecycle management. A [live event feed](docs/components/agent-session.md) exposes
tokens, tool activity, mode switches, and status. Remaining session-isolation and
lifecycle work is tracked in the
[multi-session status](docs/design/multi-session/STATUS.md).

### Automation and code review

**Tasks and scheduling.** [Task tracking](docs/components/tasks.md) records plans
and progress. The [scheduler](docs/components/scheduler.md) supports scheduled
work, including a durable store and a driver that fires jobs under each tenant's
identity.

**Campaigns.** `agent campaign …` plans hierarchical objectives in Postgres,
applies approval gates, and dispatches workers into separate worktrees. Workers
can implement tasks and open draft PRs; a poller observes merges and rolls progress
up the task tree. Repository-knowledge integration, a campaign gRPC service, and
webhook-driven follow-ups remain pending.
[Usage](docs/components/campaigns.md) ·
[design and status](docs/design/campaigns/README.md).

**Grounded code review.** Parallel collectors assemble diffs, repository state,
static-analysis findings, code structure, and summaries before model review.
The analysis toolbox provisions tools through Nix and condenses findings into a
bounded brief.
[Review design](docs/design/code-review/README.md) ·
[grounding](docs/design/fleet-grounding/README.md) ·
[analysis depth](docs/design/review-analysis-depth/README.md).

**Unattended review fleets.** `agent --serve-fleet` watches configured repositories
through forge polling and Slack triggers, checks out PRs in isolated session
workspaces, and runs reviews through the task router. It persists drafts, tracks
feedback, and supports human approval before posting, including through the portal.
[Fleet guide](docs/components/review-fleet.md) ·
[fleet design](docs/design/review-fleet/README.md).

**Forge and messaging integrations.** GitHub and GitLab share the
[forge interface](docs/components/forge.md); Gitea and Bitbucket Cloud are optional
build features. Registry cards select hosts and credentials. A
[message-transport registry](docs/design/config/05-message-transport.md) supports
Slack and Matrix implementations with inbound/outbound operations.

### Security, tenancy, and configuration

**Tool controls.** [Policy](docs/components/policy.md) controls approvals and
execution. [Scanning](docs/components/scanner.md) screens imported content;
an optional [tool-call verifier](docs/components/verifier.md) evaluates correctness
in shadow or enforce mode. Verification is off by default and fails open on
verification errors; policy decisions remain authoritative.

**Execution isolation.** The [sandbox interface](docs/components/sandbox.md)
provides local execution, reproducible Nix environments, and optional Linux
bubblewrap isolation. Bubblewrap can enforce namespaces, read-only checkout
with disposable writes, and seccomp. Separate controls provide resource limits
and an optional allow-list proxy for the agent's HTTP egress. The default local
backend is unconfined; Nix reproducibility alone does not provide confinement.

**Identity and access.** Implemented security support includes OIDC login, agent
JWTs, sessions, RBAC roles and bindings, CLI/browser/desktop sign-in, TLS/mTLS,
credential reload, and authentication auditing. Tenant identity comes from the
authenticated principal on secured services.
[Security design](docs/design/security-hardening/README.md) ·
[implementation status](docs/design/security-hardening/STATUS.md).

**Tenant-scoped state and configuration.** Tenant-aware stores, search partitions,
ClickHouse row policies, and scoped configuration support multi-org operation when
configured. Bootstrap TOML works alongside dynamic registries backed by file,
SQLite, or Postgres storage and atomic multi-card transactions. The
[configuration editor](docs/components/config.md) preserves TOML comments and
reports restart requirements.
[Configuration design](docs/design/config/README.md) ·
[tenancy boundaries and remaining work](docs/design/multi-tenancy/STATUS.md).

### APIs, portal, and deployment

**Services and REST.** Supported seams run individually with `--serve-<seam>` or
in the `--serve-all` gateway over TCP or Unix sockets, with reflection, health checks,
and configuration-selected remote clients. gRPC remains the primary API. Envoy also
translates REST/JSON to the same authenticated handlers, with a generated OpenAPI
contract and drift gate.
[gRPC guide](docs/grpc.md) · [REST/OpenAPI design](docs/design/rest-openapi/README.md).

**Management portal.** The Flutter portal provides sign-in, live agent activity,
prompt/personality management, cognition-graph editing, router administration,
configuration forms, review drafts, and access management. It also launches the
observability UIs. Registry changes and bootstrap configuration edits have different
apply/restart behavior, shown by their respective surfaces.
[Portal design](docs/design/portal/README.md) ·
[operating runbook](docs/portal-obs-mcp-runbook.md).

**Diagnostics and deployment.** `agent doctor` and the fleet `Preflight` RPC probe
configured dependencies and authentication setup
([diagnostics](docs/design/doctor/README.md)). Native deployment is supported;
Nix-built images, manifest rendering, ArgoCD deployment, certificate reload, and
SPIFFE identity have also been implemented and exercised on the single-node k3s
target. Cluster edge/portal, sandbox sidecar, observability cut-over, and full
HA Kubernetes validation remain pending.
[Native topology](docs/deployment-l2.md) ·
[Kubernetes design](docs/design/k8s/README.md) ·
[deployment status](docs/design/k8s/STATUS.md).

### Observability

Prometheus metrics, Grafana dashboards, and OpenTelemetry spans expose provider,
tool, policy, routing, session, and fleet behavior. Trace context crosses service
boundaries; tenant/repository attribution follows the documented label and span
rules. A built-in `metrics` tool lets the agent inspect its own counters, and the
[metrics proxy](docs/components/metrics-proxy.md) serves portal queries.
[Metrics](docs/metrics.md) · [tracing](docs/tracing.md) ·
[dimensionality design](docs/design/observability/README.md).

ClickHouse stores events, logs, usage, and workflow history. The general telemetry
writer uses a bounded channel and drops rows rather than blocking the loop; the
local episodic log remains the durable session record. Specialized stores such as
the digest ledger have their own persistence guarantees.
[Observability overview](docs/observability.md).

## Quality and evaluation

The local `nix flake check` gate covers formatting, linting, tests, dependency
checks, generated artifacts, protocol compatibility, documentation links,
performance ceilings, and allocation budgets. Tests exercise libraries, the CLI
as a process, service transports, authentication, and portal behavior.
[Testing](docs/components/testing.md) ·
[benchmarks and leak budgets](docs/components/benchmarking.md) ·
[portal testing](docs/design/portal-gui-testing/README.md).

The [multi-tenancy audit](docs/components/mt-audit.md) checks service and
observability coverage against a manifest. The
[load harness](docs/design/loadtest/README.md) exercises overload and backpressure.
[Evaluation tools](docs/eval-all.md) support model-based quality/security runs and
external benchmark harnesses; the [graph arena](docs/graph-arena.md) compares
cognition configurations. These measurements have task, model, and environment
limits; individual reports record the conditions.

The opt-in [libnix Cargo experiment](docs/design/libnix/README.md) adds a development
shell and smoke runner alongside the existing Crane build. Its
[evaluation](docs/design/libnix/EVALUATION.md) records current limitations and
unequal toolchain/cache conditions; it has not replaced the default build.

The [peer comparison](docs/features-comparison.md) and
[parity specifications](docs/parity/README.md) explain the project's original
reference points and remaining gaps. Peer claims are dated snapshots of the
sources inspected, rather than a current ranking.

## Limitations and ongoing work

The bundled [embedder](docs/components/embedder.md) uses feature hashing, and the
bundled tokenizer uses heuristic counts. Model and language support varies by
backend. MCP currently exposes tools; the deeper resources/prompts/OAuth work is
tracked separately. Conversation checkpoints do not imply file-edit rollback.

[Repository knowledge](docs/design/repo-knowledge/README.md) has implemented graph
storage foundations, including the Postgres backend. Its extraction/indexing
pipeline, agent-facing tools, cited inventory, and briefing integrations remain
planned ([status](docs/design/repo-knowledge/STATUS.md)). This is separate from the
available Go/SCIP code intelligence above.

[GitHub App authorization](docs/design/forge-auth/README.md) has optional API token
minting and tenant-scoped installation-registry CRUD. The mint gate still uses an
operator allowlist; ownership-verified onboarding, clone credential injection,
and the webhook receiver remain pending
([status](docs/design/forge-auth/STATUS.md)).

The [docs index](docs/README.md) links the full design catalog, including partial
tracks, experiments, and deferred ideas. Consult component documentation and
implementation logs alongside older design prose when deciding what to enable.

## Documentation and contributing

[Documentation index](docs/README.md) · [architecture](DESIGN.md) ·
[operating guide](docs/operating.md) · [contributor conventions](CLAUDE.md).

To add an implementation, implement its trait, register a factory, and select it
by configuration. The public registry also supports custom binaries and MCP
provides an extension path without Rust. Typed
[lifecycle hooks](docs/components/hooks.md) observe turns, tools, and compaction;
pre-tool hooks can veto an already-authorized call.
[Extension guide](docs/extending.md).

## License

Public domain — [Unlicense](LICENSE).
