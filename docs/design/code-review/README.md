# Design: the Code Review Flow

Status: **increments 1–4 implemented + measured; 5–9 designed.** This directory is
the design of record (requirements, wire contracts, instrumentation, what is
deferred). The grounded-fact core is now **built, gate-tested, and running** — see
the live per-increment tracker in [`STATUS.md`](STATUS.md) and the measured base
rate in [`eval/`](eval/README.md). The component docs below still read as the
original design; where the shipped code refined a detail, `STATUS.md` is authoritative.

**Shipped (merged):** `agent --review <PR#|branch|.|base..head>` produces a
grounded, **compacted** context — repo/change/git-state, the range's commit intent,
budget-bounded diff hunks, **static-analysis findings** (golangci-lint + clippy),
**changed function signatures**, and the **Go call graph / blast radius** — driven
by a health-checked LLM pool, with two gRPC services and a dual-judge (assistant +
GLM-5.2) evaluation harness that documents the base rate. **Next:** code-style
fingerprint (07), then cheap-LLM summaries (08) and recording (09).

## The idea

`agent-seddon` is built to **not** lean on expensive API models. The target
deployment is a pool of cheap, high-throughput ones: a colocated **GLM** on
8×MI300 (192 GB each) that is powerful *and* nearly free to run, a local **MI50**
(32 GB) fine for simple jobs, and an even smaller **RTX 3070** for trivial ones.
The economics invert the usual advice — instead of *minimizing* model calls we
**fan out aggressively**, because tokens here are close to free.

The most important workflow this serves is **code review** (a lot of Go). The
thesis:

> When a task turns into a review, don't spend the model — or a swarm of
> subagents — re-discovering the repository. **Establish the facts
> deterministically first**, then let cheap local models summarize the parts a
> human needs prose for. The model context is then *grounded in fact and cannot
> hallucinate the codebase*.

A file list from the index cannot be wrong. A diff from `git` cannot be wrong. A
`golangci-lint` finding is a fact, not an opinion. A call graph from the Go AST is
the real call graph. We build that fact base with tools, in parallel, and only
*then* hand the model a bundle it can reason over without inventing structure.

## The pipeline

```
                      ┌─────────────────────────────────────────────┐
  prompt / PR# ─────▶ │  02 mode-detection  (deterministic + vote)  │
                      └───────────────────┬─────────────────────────┘
                                          │ TaskMode::Review
                                          ▼
                      ┌─────────────────────────────────────────────┐
                      │  03 orchestrator  — opens `review.collect`   │
                      │       fan-out span, dispatches concurrently: │
                      └───────────────────┬─────────────────────────┘
          ┌───────────────┬───────────────┼───────────────┬───────────────┐
          ▼               ▼               ▼               ▼               ▼
  ┌───────────────┐┌─────────────┐┌─────────────┐┌─────────────┐┌───────────────┐
  │ 04 repo/change││ 05 static   ││ 06 AST /    ││ 07 code     ││ 08 cheap-LLM  │
  │ + git-state   ││ analysis    ││ call-graph  ││ style       ││ summaries     │
  │ (fast, free)  ││ (Go tools)  ││ (Go AST)    ││ (determin.) ││ (pool 01)     │
  └───────┬───────┘└──────┬──────┘└──────┬──────┘└──────┬──────┘└───────┬───────┘
          └───────────────┴───────────────┼───────────────┴───────────────┘
                                          ▼
                      ┌─────────────────────────────────────────────┐
                      │  09 ReviewFacts  — grounded bundle + record  │
                      │       injected as context; facts → memories; │
                      │       row → ClickHouse `agent_reviews`       │
                      └─────────────────────────────────────────────┘
```

The **LLM Pool (01)** underpins the two stages that call models — the mode vote
(02) and the summaries (08) — with health-checked, capability-tiered,
parallel-fan-out dispatch over the heterogeneous endpoints.

The flow **stops at the grounded fact-bundle + summaries**. Turning those facts
into review *findings*, and posting them to the PR via the `Forge` seam, are
future scope (noted in `09` and `04`), not designed here.

## Three principles

1. **Grounded-first.** `ReviewFacts` is assembled from tool output only. LLM
   summaries are labelled *derived/soft* and never overwrite a hard fact. A fact
   the tools could not establish is recorded as *unknown*, never guessed.
2. **Cheap-heavy pool.** With near-free local tokens, fan out: vote on the mode
   with several models, summarize every changed function, and (future) run an
   ensemble of reviewers. Health-check the pool because it is intermittent.
3. **Everything a parallel seam.** Each collector is an `agent-core` seam that can
   run as its own gRPC service (`--serve-<name>`), so collection is genuinely
   concurrent — and, per the project thesis, legible: each stage emits a metric, a
   span, and a duration you can account for.

## The components

| # | Doc | Component | One line |
|---|---|---|---|
| 01 | [`llm-pool.md`](llm-pool.md) | **LLM Pool & Health** | Declarative pool of cheap heterogeneous endpoints; capability/cost tiers; active health probe; failover *and* parallel fan-out. |
| 02 | [`mode-detection.md`](mode-detection.md) | **Task-mode detection** | Detect the review task: deterministic signals first, cheap pool vote to confirm. Fail-safe. |
| 03 | [`orchestration.md`](orchestration.md) | **Orchestrator + `ReviewFacts`** | Concurrent `FactCollector` fan-out into one grounded bundle; the `agent review` entrypoint + in-loop hand-off. |
| 04 | [`repo-and-change-facts.md`](repo-and-change-facts.md) | **Repo / change / git-state** | File set, changed files + diff, language detection, upstream URL, fork-vs-clone. |
| 05 | [`static-analysis.md`](static-analysis.md) | **Static analysis (Go first)** | Tiered `golangci-lint` + `gosec` + `go vet` as a dedicated service, one tool per parallel job; language-extensible. |
| 06 | [`ast-callgraph.md`](ast-callgraph.md) | **AST & call-graph** | Go AST + call-graph summary so the model grasps hierarchy without guessing. |
| 07 | [`code-style.md`](code-style.md) | **Code-style fingerprint** | Comment density, commit style, naming case, indentation — computed, not guessed. |
| 08 | [`summarization.md`](summarization.md) | **Cheap-LLM summaries** | Before/after summaries of changed functions and files, fanned over the pool. |
| 09 | [`recording.md`](recording.md) | **Grounded context + recording** | `ReviewFacts` → injected context, durable memories, ClickHouse `agent_reviews`. |
| 10 | [`wire-contracts.md`](wire-contracts.md) | **Protobuf + gRPC** | Every new `.proto` message and service, consolidated; ports; buf governance. |
| 11 | [`observability.md`](observability.md) | **Deep instrumentation** | Per-component metrics, the fan-out span tree, logs, and duration / parallel-optimization accounting. |

Docs `10` and `11` are cross-cutting: they consolidate what each component
already specifies in its own **Protobuf**, **gRPC interface**, **Prometheus**,
and **Tracing + logs** sections.

## Status

[`STATUS.md`](STATUS.md) is the **live tracker** (per-increment, per-PR, updated as
code merges); the snapshot below is the coarse scorecard. Columns: **Designed** (the
doc is complete), **Wire** (its `.proto` + service exist), **Metrics** (instrumented),
**Impl** (code merged). *Reuses* names the existing primitive it builds on. The
grounded-fact core (01–04) is shipped, gate-tested, and measured (see [`eval/`](eval/README.md));
the deeper enrichments (05–09) are designed and next.

| Component | Designed | Wire | Metrics | Impl | Reuses (exists today) |
|---|:--:|:--:|:--:|:--:|---|
| 01 LLM Pool & Health | ✅ | ✅ | ✅ | **✅** | `Router` / `Candidate` / `RouterCfg` (`agent-providers`) |
| 02 Task-mode detection | ✅ | ✅ | ✅ | **✅** | `SearchMode` enum pattern; `clamped_confidence` |
| 03 Orchestrator + `ReviewFacts` | ✅ | ✅ | ✅ | **✅** | parallel tool-dispatch idiom (`agent.rs`); registry seam pattern |
| 04 Repo / change / git-state | ✅ | ✅ | ✅ | **✅** | `Manifest::scan`, `RepoBackend::diff`/`log_range`, `Forge`, `lang_of` |
| 05 Static analysis (Go) | ✅ | ✅ | ✅ | ❌ | `Sandbox` seam; xtcp2-go tiered `golangci-lint` |
| 06 AST & call-graph | ✅ | ✅ | ✅ | ❌ | `Sandbox` seam (go helper); `RepoBackend::read_file` |
| 07 Code-style fingerprint | ✅ | ✅ | ✅ | ❌ | `Manifest::scan`, `RepoBackend::log` |
| 08 Cheap-LLM summaries | ✅ | ✅ | ✅ | ❌ | LLM Pool (01); `DiffResult`; `join_all` idiom |
| 09 Grounded context + recording | ✅ | ✅ | ✅ | ❌ | `MemoryEvent` side-channel, `CompositeMemory`, telemetry rows/writer |
| 10 Wire contracts | ✅ | ✅ | — | **partial** | `agent-proto` + `build.rs` + buf; `embed` end-to-end template |
| 11 Observability | ✅ | — | — | ❌ | `metered.rs` decorators, `RouteEvent` callback, W3C span propagation |

### Service + metrics allocation

| Service (`--serve-…`) | `nix/constants.nix` block | Fan-out span child | Status |
|---|---|---|:--:|
| `llm-pool` | `llm_pool` (:50073) | `pool.dispatch` | ✅ shipped |
| `fact-collector` (gateway) | `review` (:50074) | `review.collect` (root) | ✅ shipped |
| `analyzer` | new `analyzer` block | `review.analyze` | designed |
| `ast` | new `ast` block | `review.ast` | designed |
| `style` | new `style` block | `review.style` | designed |
| `summarizer` | new `summarizer` block | `review.summarize` | designed |

Metric families `agent_pool_*` and `agent_review_*` are live (see `11`).

## Build order (progress)

1. ✅ **01 Pool** — the foundation (active health + fan-out) everything else calls.
2. ✅ **02 + 03** — the spine: detect the mode, orchestrate the fan-out.
3. ✅ **04** — the cheapest, highest-value grounded facts (file set, diff, git state),
   then the **thicken + compact** pass (diff hunks, commit intent, a byte budget).
4. **05 ← next** — the Go/Rust linter toolset into `flake.nix`; the dedicated
   analyzer service. GLM ranked static analysis its **#1 addition**.
5. **08** — summaries over the pool.
6. **06 · 07 · 09** — AST/call-graph (incl. the cheap **signature-diff** subset),
   style fingerprint, and recording.

## House rules these docs follow

- The security model of [`../../../CLAUDE.md`](../../../CLAUDE.md): the model and
  all repo/PR/remote content are **untrusted**. Paths through `confine`, ids/refs
  through `safe_segment`, external-tool output capped, remote-URL parsing fails
  closed, pool hints/counts clamped. `adversarial_` tests are mandatory.
- The seam conventions of [`../../extending.md`](../../extending.md) and the wire
  conventions of [`../../grpc.md`](../../grpc.md).
- The measurement-first ethos of the sibling design
  [`../tool-call-verification.md`](../tool-call-verification.md), whose ensemble +
  recording pipeline is the direct precedent for `01`, `08`, and `09`.
