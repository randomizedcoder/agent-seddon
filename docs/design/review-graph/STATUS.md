# Review graph — status

Living tracker for the [review-graph track](README.md). Each increment is one gated PR
**based off main**. `nix flake check` is the gate for every one, and each increment updates
this file in its own PR.

Legend: ⬜ not started · 🟡 in progress · ✅ merged.

## Increments

| # | Title | Design | State | PR |
|---|---|---|---|---|
| 0 | Design directory | all | 🟡 | (this PR) |
| 1 | Fix "Files changed: 0" on nixpkgs: target-branch base fetch, loud empty diff, forge file-list fallback | [05](05-nix-and-native-analysis.md#prerequisite-files-changed-0-increment-1) | ⬜ | — |
| 1b | Work queues + worker pools: (i) in-process stage pools (fetch/ground/review/finish/index) with per-stage worker counts and per-repo/tenant caps; (ii) durable `FleetJobQueue` (memory + Postgres, SKIP LOCKED claim, leases, retries, coalescing); (iii) multi-process with affinity, graceful shutdown, portal stage view | [06](06-work-queue.md) | ⬜ | — |
| 2 | Condensed review on the single-session path: `agent_review::condense()`, record + ClickHouse column + rows/history, proto + gen, roster `post`, approve posting switch, portal tab | [03](03-condensed-review.md) | ⬜ | — |
| 3 | Pipeline document kind: `review_step`/`ground`/`assemble`/`condense` schema rows, validator rules, `GraphService` describe/validate | [01](01-review-graph.md) | ⬜ | — |
| 4 | `PhaseRunner` + fleet wiring: roster `graph`, `review-fleet.textproto`, `0009_condense.md`, fallbacks, step metrics | [02](02-pipeline-executor.md) | ⬜ | — |
| 5a | Index when a repo is added: job queue, manifest, roster index status, portal badge, metrics, incremental refresh, GC | [04](04-repo-knowledge.md) | ⬜ | — |
| 5b | Index consumption: brief context, `find_*`/`search` step tools on the row index, overview fed as data | [04](04-repo-knowledge.md#consumption) | ⬜ | — |
| 6 | Nix + C/C++ static suite, phase-snippet shellcheck, toolbox + checks, nixpkgs lens fragments | [05](05-nix-and-native-analysis.md#nix-static-suite-increment-6) | ⬜ | — |
| 7 | Sandboxed `nix_eval` collector (daemon-socket spike first) | [05](05-nix-and-native-analysis.md#sandboxed-eval-collector-increment-7-opt-in) | ⬜ | — |

**Build order:** 1 first (the collectors and the condensed review depend on a correct changed
set), then 1b-i straight after (it removes the head-of-line blocking; 1b-ii/iii follow). Then 2, which brings value on today's path. 5a/5b can land before 3/4, because the
single-session review also uses the richer brief. 3 → 4 next. 6 and 7 are independent of
3–5.

## As-built log

- **2026-11-02** — Increment 0 opened. Wrote README (decisions, checklist → graph mapping,
  trade-offs, open questions) and 01–05. Grounded the anchors against main `e9c57b6f`.
