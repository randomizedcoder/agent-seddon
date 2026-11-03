# 04 — Repo knowledge: index when a repo is added, reuse in every review

The fleet reviews the same repos again and again, but today it relearns them each time:

- Grounding rebuilds every collector result per PR (`WorktreeGrounder::ground`,
  `crates/agent-runtime/src/agent.rs:459`).
- Only the bare mirror persists (`<fleet_root>/<user>/<id>/mirror`,
  `crates/agent-runtime/src/fleet_review.rs`).
- The `CacheFingerprint` (`fleet_review.rs:356`) caches clone wiring, not knowledge.
- No architecture overview of the repo exists anywhere.

This document makes repo knowledge **precomputed and kept fresh**, so review steps can add it
to their context cheaply.

## Index when a repo is added

Adding a roster row starts an **indexing job** for that row, so the first review finds
everything already built. A row can be added through `PutSession`, the seed file
(`[review_fleet] seed_file`) or the portal roster dialog. Changing a row's `repo`, `backend`
or `forge_id` also queues a job.

The job runs these steps in order, each fail-soft, with its own deadline:

1. **Mirror.** `CliBackend::ensure_mirror` / `fetch` (`crates/agent-git/src/cli.rs`). Then a detached
   worktree of the default branch at `<fleet_root>/<user>/<id>/knowledge/<base-sha>/src`.
2. **Search index.** Full-text plus symbol index (agent-search / tantivy) rooted at that
   worktree, stored at `knowledge/<base-sha>/index/`. It is **per row**, not the
   process-global index (which is rooted at `cfg.agent.working_dir`).
3. **Language indexes.**
   - Go: call graph (`agent-go-ast` / `go-graph`), plus SCIP via `scip-go` where it is
     available.
   - Rust, Cargo and docs: a repo-graph snapshot (`agent repo add` / `index`, Postgres, keyed
     by `(tenant, repo, commit)`; [repo-knowledge](../repo-knowledge/README.md) RK-01..06).
4. **History signals.** Churn, co-change and bus factor over the history (the existing
   `cochange.rs` / `churn.rs` logic, run once over the whole history instead of per changed
   file).
5. **Architecture overview.** A strong-model pass (`RouteRole::Summarize`, heavy tier) over
   the indexes. It covers entry points, modules and their roles, conventions, test style, and
   build and CI. Every claim must cite `file:line`, and uncited claims are dropped **mechanically**:
   - the overview is produced as structured JSON (`{sections:[{claim, citations:[file:line]}]}`
     via `complete_structured`);
   - each citation is `confine()`d and checked against the indexed tree (the file exists,
     and the line is within its length);
   - a claim left with no valid citation is removed before rendering.

   Row: `adversarial_overview_fabricated_citation_dropped`. The output
   is capped at 16 KiB and stored at `knowledge/<base-sha>/overview.md`.

**Manifest.** `knowledge/<base-sha>/manifest.json` records, per index: base sha, tool name
and version, built-at, size, duration and outcome. A reader can tell whether an index is
present and fresh without opening it.

## Status

The row shows an index status, held in the roster store next to the row so it survives
restarts:

| Status | Meaning |
|---|---|
| `indexing` | A job is running (with `progress`: step k/5) |
| `ready` | Every index matches the current base sha |
| `stale` | The base has moved and a refresh is queued or running; the last-good indexes still serve |
| `failed` | The last job failed (with `error`, redacted). The last-good indexes still serve if any exist |

Status is visible in three places:

- On the portal Fleet roster card, as a badge plus a "Re-index" button that needs
  `can('approve','review')`.
- Through the gRPC `ReviewFleetService`, as a new `IndexStatus` on `FleetSession`.
- As metrics: `agent_fleet_index_seconds{tenant,repo,index,outcome}`,
  `agent_fleet_index_bytes{tenant,repo,index}` and `agent_fleet_index_status{tenant,repo,status}`.

### As built (5a-i)

Increment 5a is split. 5a-i (`crates/agent-runtime/src/fleet_index.rs`) builds step 2, the
search index, end to end; the rest of this document is still the target.

- **Trigger.** The fleet sync pass (at boot and every 60 s per tenant) queues each admitted,
  enabled row with `FleetIndexer::enqueue`. A row skipped by the pass (a broken or missing
  forge card) is never queued, so it is never cloned. The queue coalesces per
  `(tenant, id)` and is capped at 4096; one worker runs a job at a time, under the row's
  tenant scope. A row is re-checked at most every `refresh_secs` (default 3600) unless its
  `repo`, `backend`, `base_url` or `forge_id` changed, which re-checks it on the next pass.
- **Job.** Fetch the row's mirror (the same backend the reviews use), resolve `HEAD` to a
  commit sha, and stop if the `ready` index already has that base. Otherwise add a detached
  read-only worktree `knowledge-<sha12>` at the base, index it into
  `knowledge/<sha>/index` with `TantivyBackend` over a capped `FsDocumentSource`, write
  `knowledge/<sha>/manifest.json`, and remove the worktree (only the index is kept, to save
  disk). The build runs on a blocking thread (tantivy's reindex loop has no await point)
  and stops itself at `deadline_secs` (default 1800): the scan and every file read check
  the deadline, and an expired build fails and keeps no generation. Nothing abandons the
  build by a timeout (an orphaned blocking task could write into a generation being
  removed); only the git checkout has one, at least 60 s.
- **Waiting.** While the mirror is still being cloned the job returns at once and the row
  shows `pending`; the next pass retries, since a wait is not counted against the throttle.
- **Status** lives in `knowledge/status.json` beside the row's mirror, not in the roster
  store, so the three roster backends needed no schema change. It is written atomically
  (temp file and rename) and read back with no symlink following and a 64 KiB cap; a torn
  or hostile file reads as no status. States are `pending | indexing | ready | stale | failed`.
  A failure keeps the last good `base_sha`, and its error is redacted and capped at 512
  characters.
- **Limits.** `max_files` (default 200 000, files taken in name order so a capped index is
  deterministic), `max_file_bytes` (default 1 MiB, larger files skipped) and `max_bytes`
  (default 2 GiB read per index, which bounds its size on disk); a capped index is
  `partial`. Git's own data (`.git`) is never indexed. Indexing pauses below
  `disk_min_free_pct` (default 10 %) free on the fleet root's filesystem, and a build that
  left the disk under it is dropped.
- **GC** keeps the current and the previous generation and removes leftover
  `knowledge-*` worktrees.
- **Visible** on `ListSessions` / `GetSession` as `FleetSession.index` (`FleetIndexStatus`),
  looked up by the row the caller's own tenant-scoped roster returned, and as metrics:
  `agent_fleet_index_runs_total{outcome,user,repo}` (`ready`, `partial`, `unchanged`,
  `waiting`, `failed`), `agent_fleet_index_seconds`, `agent_fleet_index_bytes` and
  `agent_fleet_index_files`, all retired with the row's other fleet series.
- **Config:** `[review_fleet.index]` `enabled` (default true), `refresh_secs`,
  `deadline_secs`, `max_files`, `max_file_bytes`, `disk_min_free_pct`, each clamped.

Left for 5a-ii: the portal badge and Re-index button, an incremental refresh (5a-i rebuilds
per new base), a base-moved trigger from the poller or a push webhook, the language indexes,
history signals and overview, `knowledge_max_bytes`, path-scoped nixpkgs indexing and the
GC refcount.

## Refresh

- **Trigger:**
  - A forge poll or push webhook that sees a new default-branch sha. The poller already
    fetches PR heads, so it lists the base head too, which costs one more conditional GET and
    is ETag-cached.
  - Or a daily sweep.
- **Incremental:**
  - The search index re-indexes only the files changed since the manifest's base.
  - The repo-graph uses its `(tenant, repo, commit)` snapshot diff.
  - History signals are recomputed from the delta.
  - The overview is regenerated only once the base has moved more than `overview_max_commits`
    (default 200) or it is older than `overview_max_age_days` (default 14).
- **Full rebuild** only when a tool version in the manifest changes.
- **Garbage collection:** the last 2 `knowledge/<sha>/` generations are kept. An older one is
  removed once no in-flight review references it (refcounted through the review's ctx).

## Concurrency and size

- **Priority.** Indexing is low priority. It has its own semaphore, `[review_fleet]
  index_concurrency` (default 1 per host), so it never takes review slots. Jobs run under the
  same Sandbox seam as collectors, with the network off except for the mirror fetch.
- **Limits per index:** `index_max_bytes` (default 2 GiB), `index_max_files` (default 200k)
  and `index_deadline_secs` (default 1800). A capped index is marked `partial` in the
  manifest, never silently truncated.
- **Fleet-wide disk budget.** The worst case is `index_max_bytes` × 2 generations per
  row, so 4 GiB a row and 400 GiB for 100 rows, plus each row's mirror (capped by
  `mirror_max_bytes`, [05](05-nix-and-native-analysis.md)). Real indexes are far smaller.
  For example, a Go or Rust repo of this size is tens of MiB. Even so:
  - `[review_fleet] knowledge_max_bytes` (default 100 GiB) bounds the total.
  - Over the budget, GC drops the older generation of the least recently reviewed rows
    first, then marks the largest rows `partial`.
  - Below `disk_min_free_pct` (default 10 %) on the fleet root's filesystem, indexing
    pauses. Reviews continue.
  - `agent_fleet_knowledge_bytes{tenant,repo}` and a disk-free gauge make this visible.
    l2's disk filling up on 2026-11-02 is why this guard is here.
- **nixpkgs** (40k+ packages, a 3.6 GB mirror) uses **path-scoped indexing**. A row-level
  `index_paths` allowlist (`pkgs/by-name`, `pkgs/top-level`, `lib`, `nixos/modules`,
  `pkgs/build-support`) is `confine()`d to the worktree; `..` or an absolute path is rejected.

## Consumption

The `ground` node ([01](01-review-graph.md)) and the single-session grounder both get these:

- **Context added to the brief, within the existing `context_budget_bytes`:**
  - the overview, or its section for the touched modules;
  - callers and callees of changed symbols, from the call graph or SCIP;
  - nearby similar code (search hits for new identifiers in the *row's* index, which fixes
    today's `nearby` collector reading the process-global index);
  - history signals for the changed files.
- **Tools for steps:**
  - `find_symbol` / `find_references` / `search` join the read-only step tool set, backed by
    the row's prebuilt index.
  - Hits are mapped from `knowledge/<sha>/src` paths to the PR worktree and `confine()`d.
  - A hit in a file the PR changed is flagged "pre-PR content".
- **Overview fed as data.** The overview is model output, so every step receives it fenced
  as data, never as instructions.

### As built (5b-i)

- **Nearby code from the row's index.** With `[review_fleet.index]` on, each fleet review
  opens its row's last good index read-only (`TantivyBackend::open_read_only`: no writer
  lock, nothing created) and grounds on it, so the `nearby` collector searches *this*
  repo. Before, it searched the shared backend, which indexes the fleet process's own
  tree. A row with no finished index gets no search, never the shared backend, and with
  `[review_fleet.index]` off no fleet review gets one (the factory no longer holds the
  shared backend at all).
- **The brief says which index it used:** "Repo index: the default branch at `<sha12>`
  (N files[, partial][; the row is stale/failed, so this is the last good index]). Search
  hits are pre-PR content", or "Repo index: not built yet / pending / … ; nearby-code
  search is skipped until it is ready".
- **What counts as finished:** the status names a valid commit, and that generation's
  manifest has the current format and the same base. Anything else (a hostile `base_sha`,
  a missing or mismatched manifest) is not opened.

### As built (5b-ii)

- **A `search` tool for the review session** over the same row index grounding opened
  (`GroundedReview.search` → `ReviewWorkspace.search`). It is the one tool added to the
  read-only review set: it reads a prebuilt index, walks nothing and writes nothing. It
  replaces any process-wide `search` (which indexes another tree), so a session never
  has both.
- **Pre-PR flags.** Its description says the index is the default branch, not the PR,
  and to read the PR's own files with `read_file`/`grep`. A hit in a file the PR changes
  (exact repo-relative path from the grounded change set) ends with
  `[pre-PR: this PR changes this file]`.
- **No index, no tool:** with no row index, or when grounding failed, the session's
  tool set is unchanged.
- `TantivyBackend`'s read-only form now has no corpus at all (the source and writer are
  one optional `Writable`), so no future method can mistake the index dir for a corpus.

Left for 5b-iii: `find_symbol` / `find_references` on a symbol index, `wait_for_index_secs`,
and the overview, call-graph and history context, which need 5a-ii's indexes.

**When a review arrives before the indexes are ready**, it does not wait. `ground` notes
"index pending" in the brief and proceeds. A row can set `wait_for_index_secs` (0–600,
default 0) to wait for a running job.

## Security and tenancy

- Indexes live under the row's tenant-scoped fleet root (`<fleet_root>/<user>/<id>/`) and are
  **never shared across tenants**, even for the same public repo. This matches the router
  shared-stats lesson: per-tenant cells only.
- The repo-graph rows are keyed by tenant (RK-02 schema).
- Paths from indexes are re-confined on every read.
- The overview prompt treats repo content as untrusted, and the overview itself is fenced as
  data downstream.

## Convergence with repo-knowledge

This is a fleet-side layer that reuses existing indexers. When
[repo-knowledge](../repo-knowledge/STATUS.md) RK-07 (fleet refresh), RK-09 (review collector)
and RK-15 (repo profile) land, the job's steps 3 and 5 move onto them, and `knowledge/`
becomes a cache of their output. That avoids a second knowledge store.

## Tests (increments 5a/5b)

- **Job lifecycle:**
  - `positive_add_row_queues_job`
  - `positive_ready_after_all_steps`
  - `negative_step_fails_status_failed_last_good_serves`
  - `boundary_index_max_files_marks_partial`
  - `corner_review_before_ready_does_not_wait`
  - `positive_wait_for_index_waited_then_used`: with `wait_for_index_secs` set, the job
    finishes inside the wait and the brief has the index context
  - `boundary_wait_for_index_timeout_proceeds`: the wait expires, so the review proceeds
    with "index pending"
  - `corner_base_moved_marks_stale_incremental`
  - `adversarial_index_paths_traversal_rejected`
  - `adversarial_other_tenant_index_unreachable`
  - `adversarial_overview_with_injected_instructions_fenced`
- **Manifest round-trip.**
- **GC refcount:** a generation still referenced by an in-flight review isn't removed.
