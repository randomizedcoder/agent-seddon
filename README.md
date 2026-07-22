# agent-seddon

<p align="center">
  <img src="agent-seddon.png" alt="agent-seddon logo" width="300">
</p>

A coding agent in Rust, built so you can see what it is doing and change how it
does it.

## What this is

`agent-seddon` edits files, runs shells, searches code, drives git across
branches, talks to language servers, and holds interactive terminals. That part is
not unusual — the harnesses it was measured against do most of it too.

What is unusual is the shape. **Every capability is a named component behind a
trait** — a *seam* — and each seam is swappable by a config string, separately
instrumented, individually benchmarked, and can be moved into its own process
behind a gRPC service. The thesis is that an agent should be **legible**: when it
does something, you should be able to say which component did it, read the metric
it emitted and the span it opened, and replace that component without forking the
project.

It is a single-author experimental project. There are 1,669 tests and a nine-target
`nix flake check` gate — but **no CI**: the gate runs locally, by hand, so take the
claims below as things you can verify yourself rather than things a badge asserts.

## What's different

The five things below are all in service of that one thesis.

- **It was specified against its peers before it was built.** 30 per-feature specs
  in [`docs/parity/`](docs/parity/README.md), each written by reading the
  corresponding implementation *and its test suite* in three mature harnesses, then
  laying out a test plan to match them. The specs are in the repo, including the
  parts where the peers are still ahead.
- **Every capability is a seam.** 26 traits in
  [`agent-core`](crates/agent-core/src/lib.rs), concrete impls in sibling crates
  behind cargo features, selected by config. Adding a backend is never a fork.
- **Seams can become services.** 22 of the 26 have a gRPC service with a
  buf-governed wire contract and server reflection, so a running seam can be
  introspected and called with plain JSON. The point is *structure and
  introspectability*, not speed — for a same-host seam a gRPC hop is slower than a
  direct call. See the honest scope [below](#running-seams-as-services).
- **Observability on all of it.** 74 Prometheus metric families, a 37-panel Grafana
  dashboard provisioned in-tree, OpenTelemetry spans that follow a call across a
  process boundary, and the run history queryable in ClickHouse.
- **Rust, for two specific reasons.** A single binary with no runtime or virtualenv
  to install; and no GC or JIT between the code and the numbers, which is what
  makes the instruction-count benchmarks below stable enough to gate on.

## Compared to the harnesses it was measured against

The peers are [pi](https://github.com/earendil-works/pi) (TypeScript, disciplined
minimalism), [hermes-agent](https://github.com/NousResearch/hermes-agent) (Python,
batteries-included), and [opencode](https://github.com/anomalyco/opencode)
(TypeScript, a polished fundamentals-first daily driver).

> **Peer columns are a snapshot** taken from read-only clones in July 2026. These
> are active projects and will have moved on. Every cell traces to a source path
> in [`docs/features-comparison.md`](docs/features-comparison.md).

**Fundamentals — all four have these.** The last column is the interesting one.

| Capability | pi | hermes | opencode | What agent-seddon does with it |
|---|:--:|:--:|:--:|---|
| Loop, streaming, parallel tools | Yes | Yes | Yes | Same shape; every stage emits a metric and a span |
| `bash`, read / write | Yes | Yes | Yes | Through a `Sandbox` seam, behind a `Policy` gate |
| Surgical `edit` | Yes | Yes | Yes | Unique-match guard; paths canonicalised to block symlink escape |
| Unified-diff `apply_patch` | — | Yes | Yes | Fuzzy-match chain with an explicit failure ladder |
| grep / find / ls | Yes | Yes | Yes | gitignore-aware |
| MCP client | by design, no | Yes | Yes | Plus an MCP *server* (`--serve-mcp`) |
| Subagents | — | Yes | Yes | Depth-bounded, isolated context, returns a summary |
| Skills | Yes | Yes | Yes | User-invoked (`/skill:<name>`); authoring is injection-scanned |
| Sessions and resume | Yes | Yes | Yes | Content-addressed checkpoints — branch, undo, fork |
| Approval gate | trust model | Yes | Yes | A `Policy` seam: swappable, and dialable over gRPC |
| Context compaction | Yes | Yes | Yes | Truncating *and* summarizing |

**Where it differs.** Each row is backed by a spec citing the peers' source paths.
The `Status` column bounds the claim.

| Capability | agent-seddon | Peers | Status |
|---|---|---|---|
| Seams as gRPC services | 22 `--serve-<seam>` + an all-in-one gateway, TCP or unix socket, reflection-introspectable | None ship this | Works and tested; **no container deployment** |
| Per-seam metrics | Every served seam exposes its own `/metrics`; ports generated from `nix/constants.nix` | None | Shipped, Grafana provisioned |
| Queryable run history | ClickHouse `agent_events` / `agent_logs` / `agent_usage` | pi has an adapter interface | Shipped; best-effort, drops rather than blocks |
| Traces across seams | OTLP span tree, W3C propagation into another process | None | Shipped |
| Agent inspects its own metrics | A `metrics` tool, in-process, no stack required | None | Shipped |
| Perf + leak as a build gate | iai-callgrind instruction ceilings, dhat budgets | None | Shipped; runs locally, **no CI** |
| Full-text indexed code search | tantivy index, hybrid RRF fusion | ripgrep or none | Shipped |
| Multi-branch git as tools | 9 `git_*`, one object DB + disposable worktrees, revision-addressed | all shell out to `git` | Shipped |
| LSP | diagnostics **and** navigation **and** `rename` | hermes diagnostics; opencode navigation | Shipped |
| Structured output | Validates **and repairs**, bounded | all three validate; none repair | Shipped |
| Forge | GitHub **and** GitLab behind one seam | — | Shipped |
| Reproducible execution | `nix` backend runs in a pinned flake closure | mutable images | **Reproducibility, not confinement** |

No single peer does all of these, and several do individual rows better: pi's
provider breadth (40+), hermes' ~94 tools, and opencode's UI surfaces are all well
beyond this project.

## Quickstart

Toolchain, `protoc` and dev tools all come from the flake — nothing to install on
the host.

The shipped config targets a local [Ollama](https://ollama.com), so there is no
account and no key to obtain:

```sh
ollama pull llama3.1:latest                   # a model that really calls tools
nix develop                                   # dev shell
cargo build
cargo run -p agent-cli -- --config config/local-ollama.toml \
  "write a hello world program in C called hello.c"
cargo run -p agent-cli -- --config config/local-ollama.toml   # no goal ⇒ REPL
```

[`config/local-ollama.toml`](config/local-ollama.toml) is the short runnable file;
[`config/agent.toml`](config/agent.toml) is the annotated reference that shows every
seam. To use a hosted model instead, point `[provider] base_url`/`model` at it and
supply a key inline, via an env var, or from a file.

**The model must support tool calling** — the loop cannot do anything without it,
and the symptom of a model that lacks it is an agent that replies in prose and
never edits a file. Config, the REPL's slash commands and the runtime state layout
are in [`docs/operating.md`](docs/operating.md).

## What it can do

29 built-in tools: **files** (`read_file`, `write_file`, `edit`, `apply_patch`),
**shell** (`bash`, plus `pty` for interactive sessions held across turns),
**search** (`grep`, `find`, `ls`, indexed `search`, `index_ls`), **git** (9
`git_*`), **web** (`web_fetch`, `web_search`), **code intelligence** (`lsp`), and
**platform** (`forge`, `schedule`, `todo_write`, `session_export`, `skill_write`,
`delegate`, `metrics`) — plus every tool exposed by any MCP server you configure.

Providers are OpenAI-compatible and Anthropic-native, both streaming, with model
routing and failover, prompt-cache breakpoint placement, and token/cost accounting.
Memory is a layered episodic log plus semantic recall. Multimodal content, lifecycle
hooks, and a content scanner that feeds the `Policy` gate are all seams too.

## Understanding what it's doing

This is what the project is actually for.

**Metrics.** 74 Prometheus families covering every seam — provider latency and
tokens, per-tool duration and outcome, context size and compactions, policy
decisions, scanner findings, search and memory timings. The agent serves its own
`/metrics`, and a `metrics` tool lets the model inspect its own performance
mid-run. A Grafana dashboard and the Prometheus scrape config are generated from
the same source of truth as the ports (`nix/constants.nix`), so they cannot drift:

```sh
nix run .#prometheus-up          # scraper, UI :9090
nix run .#grafana-up             # dashboards, UI :3000
```

**Traces.** OpenTelemetry spans with W3C context propagation, so one trace follows
a call *across* a gRPC seam into another process — see the two-process demo in
[`docs/tracing.md`](docs/tracing.md).

**History.** A separate native-protocol writer streams the transaction history,
logs and token usage into ClickHouse (`agent_events`, `agent_logs`, `agent_usage`).
It runs on a bounded channel and **drops rows rather than blocking the loop**: if
ClickHouse is down the agent is unaffected, and `.agent/episodic.jsonl` remains the
durable record.

Full three-signal overview: [`docs/observability.md`](docs/observability.md).

## Running seams as services

22 seams have a gRPC service. Selecting one is a config change, not a code change —
the loop cannot tell a remote seam from a local one:

```toml
[agent]
policy = "grpc"                            # the approval gate now runs elsewhere
[grpc.policy]
endpoint = "http://policy-host:50055"
```

```sh
agent --serve-policy                       # host one seam
agent --serve-all                          # host every enabled seam in one process
```

Every server carries gRPC reflection — introspect and call a live seam with plain
JSON, no `.proto` files — and the standard `grpc.health.v1` health service. The
wire contract is governed by `buf` (lint + breaking-change checks) in the gate.

**What is not shipped: container images, orchestration manifests, or any multi-host
deployment.** Everything runs over loopback or a unix socket today. The seams, the
transports, the health checks and the tests are real — a round-trip suite covers
every seam on both TCP and UDS, and
[`grpc_e2e.rs`](crates/agent-runtime/tests/grpc_e2e.rs) runs the actual agent loop
with four seams remote at once — but the deployment layer does not exist yet.

Three seams are deliberately *not* distributed;
[`docs/grpc.md`](docs/grpc.md#three-seams-are-deliberately-not-distributed) explains
why. `--serve-sandbox`, `--serve-pty` and `--serve-forge` expose arbitrary code
execution or authenticated writes — read the warning there before exposing them.

## How it's kept honest

- **Table-driven tests.** 1,669 tests over 1,096 `#[rstest]` cases, classified by
  prefix: `positive_`, `negative_`, `corner_`, `boundary_`. For anything reading
  untrusted input, `adversarial_` cases are **mandatory** and must assert the
  rejection — there are 82 of them.
- **Instruction-count ceilings.** 17 iai-callgrind benchmarks measure deterministic
  instruction counts under valgrind, each with a hard ceiling. A regression fails
  the build like a lint, and raising a ceiling shows up in the diff.
- **Leak budgets.** 14 dhat tests assert hot paths free what they allocate.
- **One gate.** `nix flake check` runs nine checks: clippy (`-D warnings`), rustfmt,
  tests, `cargo-audit`, nix-fmt, generated-constant drift, buf lint, buf
  wire-compatibility, and the bench and leak suites.

The security model assumes the model is prompt-injectable: every tool argument and
every provider-supplied value is treated as attacker-controlled. The rules are in
[`CLAUDE.md`](CLAUDE.md).

## Extending it

Implement the trait, gate it behind a cargo feature, register a factory, select it
by config string. In-tree that is one line in `register_builtins`; out-of-tree,
build your own binary against the public `Registry` + `build_agent_with` API and
never touch this repo — see [`docs/extending.md`](docs/extending.md) and the
runnable `cargo run -p agent-cli --example custom_provider`. Tools can also arrive
with no Rust at all, from any MCP server named in config.

## Status and limitations

Experimental, single-author, no CI. Known gaps, stated plainly:

- **No deployment story for the distributed seams** — see above.
- **The `nix` sandbox backend is reproducibility, not confinement.** It reports
  `network_off: false` in its own capabilities. Real isolation is unbuilt.
- **The bundled embedder is feature hashing**, not a learned model — deterministic
  and dependency-free, but real embedding backends are a seam away and not shipped.
- **The bundled tokenizer is a heuristic** and does not tokenize per-model; hermes
  is genuinely ahead here. Real BPE backends are deferred.
- Cross-session export recall, model-invocable skills, fuzzy hunk matching in
  `apply_patch`, and a secret-path write deny-list are open follow-ups — the live
  list is at the foot of [`docs/parity/README.md`](docs/parity/README.md).

## Docs

[**`docs/README.md`**](docs/README.md) indexes everything: design and architecture,
per-component docs, the operating guides, and the 30 parity specs.

## License

Public domain — [Unlicense](LICENSE).
