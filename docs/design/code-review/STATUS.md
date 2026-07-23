# Code Review Flow — implementation status

The living scorecard for the [Code Review Flow](README.md) design. The design is
**complete**; **nothing is implemented yet**. This file tracks the transition from
design → merged code, increment by increment (the same per-increment PR cadence
the [tool-call-verifier](../tool-call-verification.md) followed).

**Overall: design complete · implementation not started.**

## Legend

- **Design** — the component doc is written and complete.
- **Wire** — its `.proto` messages + service are specified (in the doc + [`10`](wire-contracts.md)).
- **Metrics** — its Prometheus / span / log instrumentation is specified (in the doc + [`11`](observability.md)).
- **Seam** — the `agent-core` trait + `agent-<name>` crate + `register_builtins` factory are merged.
- **gRPC** — the service (`--serve-<name>`) is merged (additive; follows the seam).
- **Tests** — table-driven tests incl. mandatory `adversarial_` cases are merged and green.

## Component status

| # | Component | Design | Wire | Metrics | Seam | gRPC | Tests | PR |
|---|---|:--:|:--:|:--:|:--:|:--:|:--:|:--:|
| 01 | LLM Pool & Health | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | — |
| 02 | Task-mode detection | ✅ | ✅ | ✅ | ✅ | n/a | ✅ | — |
| 03 | Orchestrator + `ReviewFacts` | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | — |
| 04 | Repo / change / git-state | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | — |
| 05 | Static analysis (Go) | ✅ | ✅ | ✅ | ⬜ | ⬜ | ⬜ | — |
| 06 | AST & call-graph | ✅ | ✅ | ✅ | ⬜ | ⬜ | ⬜ | — |
| 07 | Code-style fingerprint | ✅ | ✅ | ✅ | ⬜ | ⬜ | ⬜ | — |
| 08 | Cheap-LLM summaries | ✅ | ✅ | ✅ | ⬜ | ⬜ | ⬜ | — |
| 09 | Grounded context + recording | ✅ | ✅ | ✅ | ⬜ | n/a | ⬜ | — |
| 10 | Wire contracts (consolidation) | ✅ | — | — | — | — | — | — |
| 11 | Observability (consolidation) | ✅ | — | — | — | — | — | — |

`⬜` not started · `🟡` in progress · `✅` merged · `n/a` telemetry-local / no service.

## Planned increments (build order)

Each increment is one focused PR: implement the seam(s), add the metrics via the
typed-event/`metered.rs` pattern, write the `adversarial_`-inclusive tests, and pass
`nix flake check`. gRPC serviceability for a seam is an additive follow-up PR.

1. **Increment 1 — LLM Pool & Health (01).** `LlmPool` generalizing `Router`
   (tiers, active probe, `all()` fan-out); `[pool]` config; typed `PoolEvent`s →
   metrics. Foundation for 02 and 08.
2. **Increment 2 — Spine (02 + 03).** `TaskClassifier` + `TaskMode`; the
   `FactCollector` trait + orchestrator fan-out + `ReviewFacts`; the `agent review`
   CLI entrypoint and the in-loop hand-off.
3. **Increment 3 — Repo/change/git-state (04).** File set, diff, language
   detection, remote-URL parse + fork-vs-clone, working-tree diff.
4. **Increment 4 — Static analysis (05).** Go toolset into `flake.nix`; the
   `Analyzer` seam + dedicated service; per-tool parallel jobs.
5. **Increment 5 — Summaries (08).** `SummaryJob` fan-out over the pool.
6. **Increment 6 — AST/call-graph (06) + style (07).** The Go AST helper; the
   deterministic style fingerprint.
7. **Increment 7 — Recording (09).** `ReviewRecord` side-channel; `agent_reviews`
   + `agent_review_collectors` tables; grounded-context injection; memories.

Docs [`10`](wire-contracts.md) (wire) and [`11`](observability.md) (observability)
are not separate increments — each increment lands its own slice of both.

## Change log

- **2026-07-23** — **Thicken + compact the review context** (model-free, from the
  dual-judge base rate + [GLM design input](eval/design-input-glm.md)): the diff
  hunks are carried through and rendered; the range's commit messages
  (`RepoBackend::log_range base..head`) give intent; and compaction keeps it tight
  — telemetry footer dropped, repo line condensed, lockfile/generated diffs
  collapsed (`is_noisy`), intermediate commit bodies dropped, and a
  `[review] context_budget_bytes` (default 24 KB) fills the diff section with
  graceful truncation. `ChangedFile.patch` + `ChangeSet.commits` + `ReviewCommit`
  added (proto additive). **Refined roadmap (GLM-ranked, dual-judge-validated):**
  static-analysis findings (incr 5) → **AST signature diff** (cheap subset of incr
  6) → test-execution results (new) → call-graph blast radius (incr 6) →
  churn/blame collector.
- **2026-07-23** — **Evaluation harness + base rate** ([`eval/`](eval/README.md)):
  `ReviewTarget::Revs { base, head }` (`agent --review <base>..<head>`, + `revs:`
  gRPC wire); an opt-in `nix run .#review-eval` that generates grounded contexts
  for a code-heavy dual-language corpus (Rust from the local repo via worktrees;
  Go from **flake-pinned xtcp2** changes, hash-locked) and, with `--judge`, drives
  the GLM-5.2 assessment; a hermetic `review-go` gate check on the pinned Go
  corpus; and the recorded baseline with the assistant's + GLM's assessments.
- **2026-07-23** — **gRPC serviceability** for increments 1–3: `LlmPoolService`
  (`--serve-llm-pool`) and `FactCollectorService` (`--serve-fact-collector`) —
  `llm_pool.proto`/`review.proto`, `From`/`TryFrom` conversions, client + server
  adapters, `nix/constants.nix` ports 50073/50074 (+ `gen-constants`), CLI serve
  table, `[grpc.llm_pool]`/`[grpc.review]` client wiring (`[review] backend =
  "grpc"`). buf lint + breaking pass (additive — no baseline bump). 7 round-trip
  tests (TCP + UDS); validated end-to-end with `grpcurl` (reflection + `Collect`).
- **2026-07-23** — Increments 1–3 **seam layer** implemented (in-process): the
  `LlmPool` seam + `PoolProvider` (tiers, active liveness probe, parallel
  fan-out); the `TaskClassifier`/`ReviewCollector` seams + the `agent-review`
  crate (`HybridClassifier`, `ReviewOrchestrator`, `RepoChangeCollector`); the
  `agent --review <PR#|branch|.>` CLI entrypoint + the in-loop hand-off;
  `[pool]`/`[review]` config; `agent_pool_*`/`agent_review_*` metrics.
  `RepoBackend::remote_url` added (default method). Validated end-to-end + 33 new
  tests (unit + integration, incl. `adversarial_`). **gRPC serviceability for the
  `LlmPoolService`/`FactCollectorService` (proto/convert/client/server/constants/
  buf/roundtrip) is the immediate follow-up increment.**
- **2026-07-22** — Design complete: 12-doc set + this status tracker. Nothing
  implemented.
