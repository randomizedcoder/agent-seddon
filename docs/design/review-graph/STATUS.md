# Review graph — status

Living tracker for the [review-graph track](README.md). Each increment is one gated PR
**based off main**. `nix flake check` is the gate for every one, and each increment updates
this file in its own PR.

Legend: ⬜ not started · 🟡 in progress · ✅ merged.

## Increments

| # | Title | Design | State | PR |
|---|---|---|---|---|
| 0 | Design directory | all | ✅ | #681 (+ follow-ups #683) |
| 1 | Fix "Files changed: 0" on nixpkgs: heads+tags mirror via an atomic staged `clone --bare` (a killed clone is replaced), background clone off the review path, target-branch base fetch (`fetch_branch`) instead of a per-review `fetch --all`, process-group kill on timeout, loud unknown change set | [05](05-nix-and-native-analysis.md#prerequisite-files-changed-0-increment-1) | 🟡 | (this PR) |
| 1c | Mirror upkeep: old-shape (`clone --mirror`) migration as a maintenance job, `refs/fleet/pr` retention, aged `tmp_pack`/`.keep` sweep, `agent_fleet_mirror_bytes` + `mirror_max_bytes`, forge file-list fallback for a failed diff, a `FetchBranch` RPC for `GrpcRepo` | [05](05-nix-and-native-analysis.md#prerequisite-files-changed-0-increment-1) | ⬜ | — |
| 1b | Work queues + worker pools: (i) in-process stage pools (fetch/ground/review/finish/index) with per-stage worker counts and per-repo/tenant caps; (ii) durable `FleetJobQueue` (memory + Postgres, SKIP LOCKED claim, leases, retries, coalescing); (iii) multi-process with affinity, graceful shutdown, portal stage view | [06](06-work-queue.md) | ⬜ | — |
| 2 | Condensed review on the single-session path: `agent_review::condense()`, record + ClickHouse column + rows/history, proto + gen, roster `post`, approve posting switch, portal tab | [03](03-condensed-review.md) | ⬜ | — |
| 3 | Pipeline document kind: `review_step`/`ground`/`assemble`/`condense` schema rows, validator rules, `GraphService` describe/validate | [01](01-review-graph.md) | ⬜ | — |
| 4 | `PhaseRunner` + fleet wiring: roster `graph`, `review-fleet.textproto`, `0009_condense.md`, fallbacks, step metrics | [02](02-pipeline-executor.md) | ⬜ | — |
| 5a | Index when a repo is added: job queue, manifest, roster index status, portal badge, metrics, incremental refresh, GC | [04](04-repo-knowledge.md) | ⬜ | — |
| 5b | Index consumption: brief context, `find_*`/`search` step tools on the row index, overview fed as data | [04](04-repo-knowledge.md#consumption) | ⬜ | — |
| 6 | Nix + C/C++ static suite, phase-snippet shellcheck, toolbox + checks, nixpkgs lens fragments (the fragments are only consumed once 3–4 land; the analyzers work on the single-session path) | [05](05-nix-and-native-analysis.md#nix-static-suite-increment-6) | ⬜ | — |
| 7 | Sandboxed `nix_eval` collector (daemon-socket spike first) | [05](05-nix-and-native-analysis.md#sandboxed-eval-collector-increment-7-opt-in) | ⬜ | — |

**Build order:** 1 first (the collectors and the condensed review depend on a correct changed
set), then 1b-i straight after (it removes the head-of-line blocking; 1b-ii/iii follow). Then 2, which brings value on today's path. 5a/5b can land before 3/4, because the
single-session review also uses the richer brief. 3 → 4 next. 6 and 7 are independent of
3–5.

## As-built log

- **2026-11-02** — Increment 0 opened. Wrote README (decisions, checklist → graph mapping,
  trade-offs, open questions) and 01–05. Grounded the anchors against main `e9c57b6f`. Line numbers are hints at that commit (main has since moved, e.g. `orchestrator.rs` by ~35–50 lines after #679/#680). Navigate by the symbol named beside each anchor, and each increment re-grounds the anchors it touches.
- **2026-11-02** — Increment 1 opened. Re-grounded the root cause on l2: the nixpkgs mirror held
  **no refs at all**, and its `HEAD` was still git's `refs/heads/.invalid` clone placeholder. The
  first `clone --mirror` had been killed by the prep timeout before it wrote any ref, and
  `mirror_ready()` (`HEAD` exists) called it ready. So it was never re-cloned, every PR's base
  failed to resolve, the diff failed, and the draft read "Files changed: 0". Each review's
  `fetch --all` then pulled `refs/pull/*` until it was killed, and `kill_on_drop` killed only
  `git`, leaving `index-pack` writing `tmp_pack_*`. The fix:
  - Mirrors are ready only with a branch.
  - New mirrors are a staged `clone --bare` of heads and tags only, renamed into place when
    complete and cloned in the background for the fleet (one per path, 2 h deadline).
  - The base is fetched per review as one branch with a full refspec.
  - The sandbox kills the whole process group on a timeout or cancel.
  - A failed diff reads "unknown" in the draft and the brief.

  Old-shape mirrors (agent-seddon, xtcp2 on l2) keep working: the base fetch and `fetch` pass
  explicit refspecs, so their `+refs/*:refs/*` config is never used. Removing the `refs/pull/*`
  they already hold moves to **1c**, with retention, the sweep, the size metric and the forge
  file-list fallback.

  Fleet review round 1 fixes:
  - Readiness reads `HEAD` only, so a cloned empty repo is ready and a deep ref can't delete a mirror.
  - A cross-process `<mirror>.clone-lock`.
  - A failed clone removes `.partial`.
  - `fetch_branch` logs when there is no mirror.
  - The collector name is shared.

  `GrpcRepo` has no `FetchBranch` RPC yet (it warns per review and leaves the base as is) → 1c.
