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
| 5a-i | Index when a repo is added: per-row search index of the default branch queued by the sync pass, manifest, `knowledge/status.json` + `FleetSession.index`, metrics, disk guard, GC of 2 generations | [04](04-repo-knowledge.md#as-built-5a-i) | ✅ | #687 |
| 5a-ii-a | `Reindex` RPC (forced rebuild, per-row cooldown, disabled rows refused), staged builds swapped in, GC keeps the newest finished fallback, no refcount needed | [04](04-repo-knowledge.md#as-built-5a-ii-a) | ✅ | #691 |
| 5a-ii-b | Portal index badge + Re-index button (contact sheet), #691's review items (failed-swap restore, `busy`, forced-while-waiting, wire test + client, RBAC row) | [04](04-repo-knowledge.md#as-built-5a-ii-b) | ✅ | #692 |
| 5a-ii-c | Path-scoped indexing (nixpkgs by default), configurable Re-index cooldown, #692's review items | [04](04-repo-knowledge.md#as-built-5a-ii-c) | 🟡 | this PR |
| 5a-ii-d | Push-webhook base-moved trigger, incremental refresh, `knowledge_max_bytes`, language indexes, history signals, overview | [04](04-repo-knowledge.md) | ⬜ | — |
| 5b-i | Index consumption, grounding: each fleet review's `nearby` searches its row's index (read-only open, never the shared backend), and the brief names the index used or why there is none | [04](04-repo-knowledge.md#as-built-5b-i) | ✅ | #688 |
| 5b-ii | The review session's `search` tool on the row index, hits in changed files flagged pre-PR, replacing any process-wide `search` | [04](04-repo-knowledge.md#as-built-5b-ii) | ✅ | #689, #690 |
| 5b-iii | `find_symbol`/`find_references` on a symbol index, `wait_for_index_secs`, overview/call-graph/history context (after 5a-ii) | [04](04-repo-knowledge.md#consumption) | ⬜ | — |
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

- **5a-i** (#687, 3a1494a9): index when a repo is added, the search index only.
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
  - From the commit security review: tantivy's reindex loop has no await point, so the
    outer timeout could not stop it and it held a runtime worker. The build now runs on a
    blocking thread and stops itself (the scan and every read check a deadline), with a
    `max_bytes` read budget (default 2 GiB) and a disk re-check after the build. All scans
    now skip `.git`, which also kept a worktree's host path out of the index and fixes the
    review's `repo_file_count` (it counted every `.git` object file; pinned in
    `repo_facts_e2e`).
  - From the fleet review (approve-with-comments): no timeout ever drops the build, so no
    orphaned blocking task can write into a generation being removed or rebuilt (only the
    git checkout has a timeout, floored at 60 s); the throttle map is pruned of expired
    records past 4096, so a deleted row's entry does not leak; `List` reads index statuses
    16 at a time instead of one by one; zero caps clamp to 1; status writes refuse a
    symlinked dir and use a unique temp name; a convert test covers the wire status.
  - Not taken: `rustix` stays enabled by the `review` feature alone (the module needs
    `review` + `search`; Cargo cannot express "both", and it is a small, audited crate);
    status writes stay on the runtime thread (one small file per job). The portal
    `index` field is 5a-ii.
  - Tests: the job table (first build, unchanged, base moved, GC, partial, redacted forge
    failure), last-good-serves on failure, waiting on a cloning mirror, disk guard, layout
    confinement, other tenant unreachable (store and gRPC), queue and limits tables, a
    torn status file, rows reaching the indexer from the sync pass.

  Merged as #687 (3a1494a9) after a security review and two fleet rounds; the last draft
  had no must-fix. Not deployed to l2 yet. Its leftovers are in 5b-i below.

- **5b-i** (#688, 42878b0b): fleet reviews ground on their row's own index.
  - `TantivyBackend::open_read_only`: an existing index for queries only (no writer lock,
    so reviews share it while the builder works on the next generation; nothing created;
    `reindex` errs; `status` from the saved manifest).
  - `fleet_index::row_index` / `open_row_search`: the row's last good generation, checked
    against status and manifest, opened per review. `WorktreeGrounder` uses it when the
    factory has `with_row_indexes()` (set when `[review_fleet.index]` is on), with no
    fallback to the shared backend (another repo); the brief carries a "Repo index:" line.
  - #687 leftovers: the module doc says why `<root>/<user>/<id>` is tenant-isolated (Put
    binds `user` to the caller's tenant); GC sweeps leftover `*.tmp` status files; the
    scan checks its deadline on every walked entry; `agent_fleet_index_seconds` buckets
    reach 21600 s.
  - Tests: `read_only_open` (built / missing / empty dir, two readers while the writer
    holds its lock), `row_index_cases` (never built, pending, ready, failed serves last
    good, hostile sha, missing or mismatched manifest), `open_row_search_serves_the_row_index`,
    `grounder_search_choice` (shared vs row index, no fallback), GC temp sweep.
  - From the fleet review: with indexing off, fleet reviews no longer get the shared
    backend either (the factory dropped it; it was another tree); a read-only `reindex`
    fails before any scan, and `capabilities().incremental` is false for it; the
    read-only test proves the builder's writer lock is held while readers open; the
    corrupt-generation path ("could not be opened") and the failed-row note are tested;
    GC also sweeps temp files inside kept generations.
  - Second fleet round: `WorktreeGrounder` lost its `search` field and fallback, so no
    code path can hand a fleet review the shared index (the test now asserts "no search"
    with indexing off, instead of a fallback production could not reach); a status whose
    `base_sha` names no commit says so rather than echoing its claimed state; a stale row
    serves its last good index (new row); `reindex_inner` checks for a writer before
    scanning; the GC doc rests on unlink-while-open, not on build spacing.

  Merged as #688 (42878b0b) after three fleet rounds; the third said nothing blocks.
  Its items are done in 5b-ii below.

- **5b-ii** (#689, d38310d7): the review session searches the row index.
  - `SearchTool::for_review(backend, changed)`: the description says the index is pre-PR,
    and a hit in a changed file is flagged `[pre-PR: this PR changes this file]` (exact
    repo-relative path; a look-alike or `./`-prefixed path is not).
  - `GroundedReview.search` carries the backend grounding opened; the fleet orchestrator
    puts it and the change set's paths in `ReviewWorkspace { search, changed }`;
    `bind_review_workspace` registers the tool, replacing any process-wide `search`. No
    index, or failed grounding, adds nothing.
  - #688 round-3 items: `TantivyBackend`'s source and writer are one optional `Writable`
    (a read-only backend has no corpus to misuse); an end-to-end `ground()` test checks
    the brief line and the hand-off; a dirs-only tree with a passed deadline expires; the
    leak gate opens, queries and drops a read-only backend 100 times (~860 blocks/run,
    budget 2048); a `ready` status with no base says "names no commit"; comments on the
    unbudgeted brief line and `time_budget`'s `None`.
  - Tests: `review_search_flags_pre_pr_hits` (6 rows incl. look-alike and `./` paths),
    `review_workspace_search` (adds one, replaces a process-wide one, none without an
    index; `bash` never advertised), `review_session_gets_the_row_index` (index reaches
    the session; none without one; none after failed grounding),
    `grounding_carries_the_row_index`, `adversarial_dirs_only_tree_still_expires`,
    `adversarial_ready_without_base`.
  - From the fleet review: `REVIEW_READONLY_TOOLS` says why `search` is not on it (the
    process index is another tree; a review's `search` is bound per review), and the
    seed/dispatch/bind comments say the dispatch enforces the *advertised* set;
    `real_diff_paths_flag_pre_pr_hits` grounds a real diff end to end and checks the
    change set's paths and the index's ids agree (a mismatch would drop the flag
    silently); a review's output is cut between whole hits so no flag is lost to the
    cap; the description notes a stale index can trail the branch; `GroundedReview`'s
    Debug names the backend.
  - Second fleet round: a rename is flagged on its base side too. `ChangedFile.old_path`
    (serde default, set only for a rename, confined like the display path) and
    `ChangeSet::touched_paths()` give the session both sides; the real-diff test gained
    a rename row. Paths are normalized (no `./`, `/` separators) on both sides of the
    match, so a spelling difference can only add a flag, never drop one. The review
    output keeps 64 bytes for its trailer under the cap.

  Merged as #689 (d38310d7) after three fleet rounds; the third had no must-fix.

- **5b-ii follow-up** (#690, 839de60c), from #689's last draft:
  - `ReviewChangedFile.old_path = 8` on the wire (proto3-additive; "" is none), so a
    change set that crosses gRPC keeps a rename's base side and its pre-PR flag.
  - `old_path` is set only for a rename: a delete's display path already is its base
    side, so `touched_paths()` no longer lists it twice (delete row in the real-diff
    test).
  - `normalized()` folds every spelling (`./`, `//`, `/./`, a trailing `/`, `\`) but
    keeps `..`; a compile-time assert pins `MAX_OUTPUT > TRAILER_ROOM`.

  Merged as #690 (839de60c) after two fleet rounds. Round 1 asked for a rename row in the
  real gRPC round trip (`pool_review_roundtrip`, TCP and UDS); round 2 had no must-fix.
  Its small items are done in 5a-ii-a below.

- **5a-ii-a** (#691, 35ce2c80): the server side of Re-index.
  - `ReviewFleetService.Reindex` looks the row up in the caller's own roster, refuses a
    disabled row, and queues it forced (`queued`, `coalesced`, or `too_soon` with
    `retry_after_secs`, one request per row per 5 minutes).
  - Builds are staged in `<sha>.partial` and swapped in, so a forced rebuild of the served
    base never takes it away; GC keeps the newest finished other generation by manifest
    and removes crash leftovers, but not foreign entries or symlinks.
  - No GC refcount: an open read-only index survives its generation's removal.
  - From #690's last draft: `Copied` and `Deleted` round-trip rows, the proto `old_path`
    comment covers copies, a trailing-slash normalization row, and the `MAX_OUTPUT`
    assert sits under `TRAILER_ROOM`.

  Merged as #691 (35ce2c80) after one fleet round with no must-fix. Its should-fix
  items are done in 5a-ii-b below.

- **5a-ii-b** (#692, 0250692f): the portal side of Re-index, and #691's review items.
  - Fleet roster rows show the index state as a distinct glyph (ready, stale, waiting,
    failed), with base, file count, `partial` and the redacted error in its tooltip.
    For a caller with `trigger:fleet` on an enabled row, the badge is the Re-index
    button; the reply's outcome is a snack. A theme that names states puts the state at
    the start of the subtitle rather than beside the glyph, so the narrow row keeps the
    session id. Contact sheet: the prototype, TN3270 and accessible themes.
  - A swap whose second rename fails puts the served generation back.
  - A full queue answers `busy` (no cooldown spent), not `too_soon`.
  - A forced job that only waited (mirror cloning, disk short) is forced again on the
    next pass.
  - `GrpcFleet::reindex` and a wire round trip over TCP and UDS; the RBAC matrix row.

  Merged as #692 (0250692f) after one fleet round with no must-fix. Its should-fix
  items are done in 5a-ii-c below.

- **5a-ii-c** (this PR): path-scoped indexing, and #692's review items.
  - `[[review_fleet.index.scopes]]` indexes a repo only under some paths; the default
    scopes `NixOS/nixpkgs` to `pkgs/by-name`, `lib` and `nixos/modules`. The walk refuses
    `..`, absolute and `./` paths and any path reached through a symlink, and a scope of
    only refused paths indexes nothing (never the whole tree). The scope is in the
    manifest, so changing it rebuilds an unmoved base.
  - `reindex_cooldown_secs` (default 300, clamped to 30..=86400) replaces the constant.
  - From #692's draft: a test of the `forced` prune (at and one past the cap), the
    forced-flag lapse documented on `force`, portal rows for `coalesced`, `busy` and an
    unknown outcome (shown as text, clipped to 40 characters), the `busy` snack names the
    row, and the `checked` doc comment rewrapped.

