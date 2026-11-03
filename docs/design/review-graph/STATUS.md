# Review graph — status

Living tracker for the [review-graph track](README.md). Each increment is one gated PR
**based off main**. `nix flake check` is the gate for every one, and each increment updates
this file in its own PR.

Legend: ⬜ not started · 🟡 in progress · ✅ merged.

## Increments

| # | Title | Design | State | PR |
|---|---|---|---|---|
| 0 | Design directory | all | ✅ | #681 (+ follow-ups #683) |
| 1 | Fix "Files changed: 0" on nixpkgs: heads+tags mirror via an atomic staged `clone --bare` (a killed clone is replaced), background clone off the review path, target-branch base fetch (`fetch_branch`) instead of a per-review `fetch --all`, process-group kill on timeout, loud unknown change set | [05](05-nix-and-native-analysis.md#prerequisite-files-changed-0-increment-1) | ✅ | #684 |
| 1c-i | Mirror robustness (from #684's reviews): best-effort `.stale` delete after a swap plus a background sweep of a leftover one, fail closed on a `.clone-lock` that isn't a regular file, `fetch_branch` a no-op when the root isn't the mirror, the shared `agent_core::mirror_cloning` wording logged at info by auto-fetch and the fleet trigger loop, a log when an inline clone yields to a running one, `safe_branch` rejects a trailing `.` | [05](05-nix-and-native-analysis.md#prerequisite-files-changed-0-increment-1) | ✅ | #686 |
| 1c-ii | Mirror upkeep: `refs/fleet/pr` retention, aged `tmp_pack`/`.keep` sweep, `agent_fleet_mirror_bytes` + `mirror_max_bytes`, old-shape (`clone --mirror`) migration as a maintenance job, an off-path refresh of tags and non-target branches, a `.stale` sweep that renames then deletes without the clone lock, a typed `Error::MirrorCloning`, an async clone-lock retry (no `thread::sleep` on a worker) with a deterministic patience test, a best-effort pre-swap `.stale` delete, an unopenable foreign lock failing closed, a symlinked-root `root_is_mirror` row | [05](05-nix-and-native-analysis.md#prerequisite-files-changed-0-increment-1) | ⬜ | — |
| 1c-iii | Remote and fallback: a `FetchBranch` RPC for `GrpcRepo`, forge file-list fallback for a failed diff, a stronger reftable readiness signal, the fleet's 30 s re-poll while a mirror clones | [05](05-nix-and-native-analysis.md#prerequisite-files-changed-0-increment-1) | ⬜ | — |
| 1b | Work queues + worker pools: (i) in-process stage pools (fetch/ground/review/finish/index) with per-stage worker counts and per-repo/tenant caps; (ii) durable `FleetJobQueue` (memory + Postgres, SKIP LOCKED claim, leases, retries, coalescing); (iii) multi-process with affinity, graceful shutdown, portal stage view | [06](06-work-queue.md) | ⬜ | — |
| 2 | Condensed review on the single-session path: `agent_review::condense()`, record + ClickHouse column + rows/history, proto + gen, roster `post`, approve posting switch, portal tab | [03](03-condensed-review.md) | ⬜ | — |
| 3 | Pipeline document kind: `review_step`/`ground`/`assemble`/`condense` schema rows, validator rules, `GraphService` describe/validate | [01](01-review-graph.md) | ⬜ | — |
| 4 | `PhaseRunner` + fleet wiring: roster `graph`, `review-fleet.textproto`, `0009_condense.md`, fallbacks, step metrics | [02](02-pipeline-executor.md) | ⬜ | — |
| 5a-i | Index when a repo is added: per-row search index of the default branch queued by the sync pass, manifest, `knowledge/status.json` + `FleetSession.index`, metrics, disk guard, GC of 2 generations | [04](04-repo-knowledge.md#as-built-5a-i) | 🟡 | (this PR) |
| 5a-ii | Portal index badge + Re-index (contact sheet), incremental refresh, base-moved trigger (poll/webhook), language indexes, history signals, overview, `knowledge_max_bytes`, nixpkgs path scoping, GC refcount | [04](04-repo-knowledge.md) | ⬜ | — |
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

  Fleet review round 2 fixes:
  - An inline clone (the CLI, without the background clone) keeps the 600 s git guard instead of
    the 2 h one, and a failed inline clone is logged rather than dropped.
  - The reftable shortcut is checked against git 2.55 (a finished clone's `HEAD` file is the stub)
    and its limit documented: only a pre-staging reftable mirror killed partway could read ready.
  - The `GrpcRepo` gap is documented in `docs/components/git.md`.
  - Fixture tests: a leftover `.partial` is cleared by the next clone; an inline fetch clones.
  - The rest of round 2's lower-priority items (off-path refresh, `.stale` recovery,
    `safe_branch` parity) → 1c.

  Fleet review round 3 fixes:
  - `build_repo` sets the background clone when `auto_fetch_secs > 0` or a `[review_fleet] root`
    is set, so the `spawn_fetch` bootstrap and a single-repo fleet get the 2 h background clone
    rather than a 600 s inline one that would restart forever (round 2's doc claimed this
    already held; it didn't). Table test `mirror_clone_mode_cases`.
  - The swap re-checks the mirror path before the rename (a long clone's TOCTOU).
  - `repo-change`'s only partial is a failed diff; the invariant is commented where it is
    returned.
  - Deferred to 1c: fail closed on a `.clone-lock` that isn't a regular file; a stronger
    reftable readiness signal (an empty `reftable/` doesn't separate a killed clone, since
    `git init` writes a table).

  Fleet review round 4 fixes:
  - A clone re-checks readiness after it claims the slot, so a clone that finished while
    another waited is not repeated (`clone_rechecks_readiness_after_the_claim`).
  - A crash between the staged clone and the swap is recovered: a finished `.partial` whose
    refspecs match is moved into place instead of re-cloned; an unfinished one is re-cloned
    (fixture tests for both).
  - `docs/components/git.md` notes the refreshed base reaches the diff only when the root is
    the mirror (the fleet shape).
  - Auto-fetch logs "waits for the mirror clone" at info, not a "failed" warning.
  - The `Skipped` arm of `change_unknown` is commented as unreachable today.
  - Deferred to 1c: the fleet's 30 s re-poll while a mirror clones.

  Merged as #684 (609658fc) and live on l2 on 2026-11-02. The nixpkgs mirror went from a
  33G half-clone to 3.8G (heads and tags, no `refs/pull`), cloned in about 8 minutes, and
  the first nixpkgs drafts report real counts (1 and 2 files changed, not 0).

- **1c-i** (#686, 4f39847b): the robustness items #684's last reviews left.
  - The old mirror's delete after a swap is best-effort (warn), so a ready mirror is never
    reported as a failed clone; a leftover `<mirror>.stale` is swept in the background by
    the next fetch, under the clone lock.
  - A `<mirror>.clone-lock` that is a symlink or a dir fails the clone closed.
  - `fetch_branch` does nothing when the root is not the mirror (the diff would never see
    the branch); the fleet passes the mirror as both.
  - The "being cloned" wording is `agent_core::mirror_cloning`, read with
    `is_mirror_cloning` by auto-fetch and the fleet trigger loop, both now at info (on l2 the
    trigger loop warned once per queued nixpkgs PR per poll during the clone).
  - An inline clone that yields to a running one logs it; `safe_branch` rejects a trailing
    `.` and a `.lock` segment mid-path is covered.
  - From the fleet review of this PR: the foreign-lock guard moved into the lock open
    (`O_NOFOLLOW | O_NONBLOCK`, then a regular-file check), so the `.stale` sweep, which
    claimed without the guard, is covered too and there is no check-then-open race. A
    FIFO lock is refused without hanging. A busy claim retries for about 40 ms, which also
    fixed a pre-existing test flake (a child forked by a parallel test held the inherited
    lock fd until exec). A mid-path segment ending in `.` stays valid, as git agrees.
  - Left for 1c-ii: a sweep holds the clone lock while it deletes a large `.stale`, so a
    concurrent clone attempt reads "being cloned" (rename first, then delete unlocked);
    a typed `Error::MirrorCloning` instead of the pinned message prefix.

  Merged as #686 (4f39847b); the second fleet review found nothing blocking. Its
  should-fix items moved to the 1c-ii row: the busy retry sleeps on a tokio worker (rare:
  only cross-process or fork-window contention) and has no deterministic test; the
  pre-swap `.stale` delete still hard-fails a clone while the others are best-effort; an
  unopenable foreign lock (`EACCES`) degrades to the in-process claim; a symlinked root
  reading as the mirror is untested. A PR the trigger loop skips while the mirror clones
  stays open and unreviewed, so the next poll lists it again (nothing is dropped).

- **5a-i** (this PR): index when a repo is added, the search index only.
  - `FleetIndexService` (`agent-runtime/src/fleet_index.rs`) behind the new `FleetIndexer`
    seam in agent-core. The fleet sync pass queues each admitted, enabled row (a skipped
    row, such as one with a missing forge card, is never queued, so never cloned).
  - One worker, a coalescing queue capped at 4096, each job under the row's tenant. A row
    is re-checked at most every `refresh_secs` unless its repo, backend, base URL or forge
    changed; a wait on a mirror clone is not counted.
  - The job fetches the mirror, resolves `HEAD`, skips a base that is already `ready`,
    else builds a tantivy index from a read-only `knowledge-<sha12>` worktree under a
    deadline, writes the manifest and removes the worktree. GC keeps two generations.
  - Deviation from 04: status is a per-row `knowledge/status.json` (atomic write, no
    symlinks, 64 KiB cap), not a roster-store column, so the three roster backends need
    no schema change. Served as `FleetSession.index` (proto field 16, output only).
  - agent-search gained `ScanLimits` / `ScanReport` (name-ordered, capped, oversized files
    skipped) for a bounded scan of an untrusted tree.
  - Metrics `agent_fleet_index_{runs_total,seconds,bytes,files}`, retired with the row.
  - Disk guard: indexing pauses below `disk_min_free_pct` free.
  - Tests: the job table (first build, unchanged, base moved, GC, partial, redacted forge
    failure), last-good-serves on failure, waiting on a cloning mirror, disk guard, layout
    confinement, other tenant unreachable (store and gRPC), queue and limits tables, a
    torn status file, rows reaching the indexer from the sync pass.
