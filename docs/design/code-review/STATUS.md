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
| 01 | LLM Pool & Health | ✅ | ✅ | ✅ | ⬜ | ⬜ | ⬜ | — |
| 02 | Task-mode detection | ✅ | ✅ | ✅ | ⬜ | n/a | ⬜ | — |
| 03 | Orchestrator + `ReviewFacts` | ✅ | ✅ | ✅ | ⬜ | ⬜ | ⬜ | — |
| 04 | Repo / change / git-state | ✅ | ✅ | ✅ | ⬜ | ⬜ | ⬜ | — |
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

- **2026-07-22** — Design complete: 12-doc set + this status tracker. Nothing
  implemented.
