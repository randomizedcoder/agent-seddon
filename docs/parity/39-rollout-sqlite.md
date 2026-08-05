# Parity spec 39 — SQLite rollout / resume-last / fork index

Per-feature parity spec for a **durable, indexed `SessionStore` backend**: a
rebuildable SQLite index (plus a reverse-scannable rollout log) layered over the
existing content-addressed checkpoint store, so the agent can reverse-scan the
latest session, `resume --last` in one indexed lookup, and `list`/`fork`/`archive`
across the whole store without walking every object. Extends the `SessionStore`
seam from [`19-session-checkpoint.md`](19-session-checkpoint.md) (checkpoint/branch/
undo/fork/GC) and pairs with [`20-session-export.md`](20-session-export.md)
(cross-session recall), which reads the same saved sessions.

> **Status: ⬜ spec written, not started.** Proposes a **new `SessionStore`
> backend** — `sqlite` — selected by config alongside today's `file`/`grpc`
> (`[session] backend = "sqlite"`), living in the existing
> [`agent-session`](../../crates/agent-session) crate behind a non-default cargo
> feature (`rusqlite`, so the default build stays dependency-lean). It adds a
> **durable index table** (session id → head checkpoint, branch, `updated_at`,
> turn count, preview, cwd, fork parent, `archived`) with a recency index, plus a
> per-session append-only **rollout log** and a **reverse-JSONL scanner** that
> recovers the latest session even when the index is stale or the log tail is a
> partial/corrupt line. The content-addressed object store from spec 19 stays the
> **source of truth**; the SQLite table is a *derived, rebuildable* index (a
> missing/corrupt index is backfilled by re-scanning), so the seam gains speed and
> inspectability **without** losing its distributed, content-addressed, GC'd
> nature. Three new trait methods — `sessions()`, `latest(filter)`, `archive(id)`
> — turn `--continue`/`resume --last` and cross-store listing into O(1)/O(log n)
> index lookups instead of directory walks. **Differentiator:** none of the four
> peers pairs a *content-addressed, reachability-GC'd, gRPC-served* checkpoint
> store with a *rebuildable* SQLite index + reverse-scan recovery that is metered
> and span-traced — codex has the file+sqlite+reverse-scan trio but it is local
> and not a swappable seam; hermes/opencode have SQLite but no immutable
> content-addressed object DB or reachability GC; pi has neither. **Deferred:**
> per-user index tenancy (follows the multi-session store), FTS over the index
> (spec 20's recall already owns full-text), a `RepoBackend`-backed rollout log
> (spec 19's deferred git object DB), and index sharding for very large stores.

## Feature & why it matters

Spec 19 gave agent-seddon a git-style checkpoint store: immutable, content-addressed
checkpoints, a branch tree, `undo`/`fork`/`diff`, and reachability GC. What it did
**not** give is a fast, durable *catalogue* of the store. Every real coding agent
grows one, because the store's most common operations are not checkpoint/restore —
they are **"give me my last session"** and **"list my sessions, newest first"**:

- **`resume --last` / `--continue`.** The single most-used resume gesture is "pick
  up where I left off." That must resolve in one lookup — not by opening every
  session file, parsing a header, and sorting by mtime. It must also be **robust**:
  if the process was killed mid-write, the latest session's log tail may be a
  half-written line; resume must reverse-scan past the garbage to the last *valid*
  record rather than error or silently skip the newest session.
- **Listing across the store.** A resume picker, a portal session list, `session
  list` — all want `(id, preview, turns, cwd, updated_at, branch)` per session,
  ordered by recency, cheaply. Recomputing that by re-reading every object on every
  invocation is O(store) per keystroke.
- **Fork / archive at catalogue speed.** `fork` should record lineage (`parent`) as
  one row insert; `archive` should soft-hide a session with one flag flip, not move
  or rewrite files. Listing then filters `archived = 0` in the query.

The failure modes are all about the index being a *cache, never the truth*: a
missing or corrupt SQLite file must **rebuild** from the object store (backfill),
never lose a session; a session that exists on disk but not in the index must still
be resumable (scan fallback); and GC/`prune` must still run over the
content-addressed object DB (spec 19), with the index updated to match — never the
other way around. Get that inversion wrong and you have a fast index that lies about
what history exists. Codex's design is the north star here: a SQLite `threads` table
for speed, an immutable JSONL rollout for truth, and a reverse-scanner + backfill
that reconcile the two.

## agent-seddon today

The spec-19 store is content-addressed and correct, but it has **no durable index
and no reverse-scan**, and the CLI's resume path does not even use it yet.

- **`FileSessionStore` walks the store.** [`agent-session/src/file.rs`](../../crates/agent-session/src/file.rs)
  keeps immutable checkpoints at `objects/<id>.json` and per-session mutable heads at
  `sessions/<id>.json` (`branches: BTreeMap<String, CheckpointId>` + `current`).
  `list(session)` walks the head-chain **within one session**; `prune` `read_dir`s
  `objects/` and `sessions/` and does a reachability sweep. There is **no**
  cross-session enumerate, no `latest()`, no recency index — the trait
  ([`SessionStore` in `agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs),
  ~L1238) is `checkpoint`/`list`/`restore`/`branch`/`undo`/`fork`/`diff`/`prune`,
  all **session-scoped or id-scoped**. Nothing answers "what is my latest session?"
- **`--continue` uses the *flat* store and an mtime scan.** The CLI
  ([`agent-cli/src/main.rs`](../../crates/agent-cli/src/main.rs) `resolve_resume`)
  resolves `--continue` via
  [`session_store::most_recent`](../../crates/agent-runtime/src/session_store.rs),
  which is `list(dir).into_iter().next()` — and `list` is a `read_dir` over
  `.agent/sessions/*.jsonl` **sorted by filesystem `modified()` mtime**. This is
  *exactly pi's design* (a directory scan by mtime) and it is entirely separate from
  the spec-19 `FileSessionStore`: the rich checkpoint store and the resume path do
  not share a catalogue. `--resume ID` is a direct `load(dir, id)`.
- **No rollout log, no reverse-scan.** Each flat session is a `.jsonl` **overwritten**
  each turn ([`session_store::save`](../../crates/agent-runtime/src/session_store.rs)),
  so there is no append-only tail to reverse-scan and no partial-tail recovery. A
  crash mid-save can truncate the whole file, not just its last line.
- **The seam is already served and observed** — the reusable half. `session.proto`
  ([`session.proto`](../../crates/agent-proto/proto/agent/v1/session.proto)) exposes
  `Checkpoint`/`List`/`Restore`/`Branch`/`Undo`/`Fork`/`Diff`/`Prune`; a `GrpcSession`
  client ([`agent-grpc/src/client/session.rs`](../../crates/agent-grpc/src/client/session.rs))
  and `backend = "grpc"` already dial a remote store; the store is metered
  (`agent_session_ops_total{op}`) and span-traced. Adding `sessions`/`latest`/
  `archive` is *additive* to a live seam, not a new subsystem.

Honest gap: the object store is durable and content-addressed, but there is **no
SQLite index, no reverse-JSONL scan for the latest session, no `resume --last` over
the checkpoint store, and listing/fork/archive have no catalogue to hit** — they
walk the store or, for `--continue`, mtime-scan a *different* flat directory.

## Peer implementations & their tests

| Peer | Impl path | Test path | Framework |
| --- | --- | --- | --- |
| codex | `codex-rs/rollout/src/state_db.rs` + `state/migrations/0001_threads.sql` (SQLite `threads` index: `updated_at`/`archived`/`rollout_path`, `idx_threads_updated_at ON threads(updated_at DESC, id DESC)`), `rollout/src/recorder.rs` (`find_latest_thread_path`, JSONL rollout), `rollout/src/reverse_jsonl_scanner.rs` (`ReverseJsonlScanner`, `ScanOutcome`), `rollout/src/session_index.rs` (append-only `session_index.jsonl`, scan-from-end), `thread-store/src/local/{archive_thread,unarchive_thread,delete_thread}.rs` (fork via `forked_from_id`) | `rollout/src/reverse_jsonl_scanner_tests.rs`, `rollout/src/session_index_tests.rs`, `rollout/src/state_db_tests.rs`, `rollout/src/recorder_tests.rs`, `app-server/tests/suite/v2/thread_archive.rs` | cargo `#[test]` / insta |
| opencode | `packages/core/src/session/sql.ts` (`SessionTable`: `time_updated`/`time_archived`/`parent_id`, `session_parent_idx`), `packages/core/src/database/database.ts` (`opencode.db`), `packages/opencode/src/session/session.ts` (`listByProject`/`listGlobal` `ORDER BY time_updated DESC`, `fork`), `packages/opencode/src/cli/cmd/run.ts` (`--continue` = first root of the list) | `packages/opencode/test/server/session-list.test.ts`, `packages/opencode/test/server/global-session-list.test.ts`, `packages/core/test/session-create.test.ts` | bun:test |
| pi | `pi/packages/coding-agent/src/core/session-manager.ts` (`findMostRecentSession` = `readdirSync` + `statSync().mtime` sort; per-cwd `*.jsonl`), `.../cli/args.ts` (`--continue`/`--resume`), `.../main.ts` (`continueRecent`) | `pi/packages/coding-agent/test/session-manager/file-operations.test.ts`, `.../test/session-info-modified-timestamp.test.ts`, `.../test/session-selector-search.test.ts` | vitest |
| hermes | `hermes-agent/hermes_state.py` (SQLite `sessions` table: `started_at`/`ended_at`/`parent_session_id`/`archived`, `idx_sessions_started ON sessions(started_at DESC)`; `search_sessions` `ORDER BY last_active DESC`; `set_session_archived`), `hermes-agent/hermes_cli/main.py` (`_resolve_last_session` → `--continue`) | `hermes-agent/tests/hermes_cli/test_resolve_last_session.py`, `hermes-agent/tests/test_hermes_state.py`, `hermes-agent/tests/hermes_state/test_session_archiving.py`, `hermes-agent/tests/hermes_state/test_resolve_resume_session_id.py` | pytest |

**codex** is the anchor — it is the only peer that ships **exactly** agent-seddon's
target shape: an immutable JSONL rollout (the truth) paired with a SQLite index (the
speed) reconciled by a reverse-scanner + backfill.

- **SQLite index of sessions** (`state/migrations/0001_threads.sql`): a `threads`
  table keyed by `id` with `rollout_path`, `created_at`, `updated_at`, `cwd`,
  `title`, `source`, `archived`, `archived_at`, and — critically —
  `CREATE INDEX idx_threads_updated_at ON threads(updated_at DESC, id DESC)` plus
  `idx_threads_archived`. Listing and resume never walk the sessions directory; they
  query this index. The DB file is `state_5.sqlite` under the codex home
  (`state/src/sqlite.rs`).
- **Resume-last is index-first with a scan fallback** (`recorder.rs`
  `find_latest_thread_path`, ~L706): it asks the SQLite state DB for the newest
  thread page (`list_threads_db`, `ThreadSortKey::UpdatedAt`, `SortDirection::Desc`)
  and returns the first usable candidate; **only on a complete miss** does it fall
  back to `get_threads` (the filesystem scan). `exec resume --last`
  (`exec/src/cli.rs` `ResumeArgs`, tested at `exec/src/cli_tests.rs:80`) rides this;
  the `--last` lookup "avoids auditing every rollout" when the index hits
  (`exec/src/lib.rs` ~L1509).
- **Reverse-JSONL scanner** (`reverse_jsonl_scanner.rs`): reads a newline-delimited
  JSON file **from the end** in 64 KiB chunks (`READ_CHUNK_SIZE`), yielding
  `ScanOutcome::Parsed(T)` or `ScanOutcome::Rejected(serde_json::Error)` **without
  stopping** — so a partial/corrupt trailing record is skipped and the scan
  continues to the last valid one. `new_at(end_byte_offset)` scans a *frozen prefix*
  (ignore records appended after a snapshot). This is the partial-tail-recovery
  primitive.
- **Append-only session index** (`session_index.rs`): `session_index.jsonl` is
  append-only (`SessionIndexEntry { id, thread_name, updated_at }`); readers **scan
  from the end** so "the most recent entry wins" (`find_thread_name_by_id`,
  `find_thread_meta_by_name_str` — the latter keeps walking newest-first until an id
  resolves to a *loadable* rollout, skipping an entry whose rollout was never
  materialized or is partial).
- **Fork + archive as catalogue ops.** `SessionMeta.forked_from_id` records lineage
  (`recorder.rs` ~L803); TUI `/fork` (`tui/src/slash_command.rs:37`). Archive is a
  flag/timestamp in the index (`archived`/`archived_at`), with
  `thread-store/src/local/{archive_thread,unarchive_thread,delete_thread}.rs`
  server RPCs — listing filters `archived = 0`.
- Tests pin every seam: `reverse_jsonl_scanner_tests.rs`
  (`scans_jsonl_records_from_end`, `rejects_invalid_json_and_continues_scanning`,
  `accepts_valid_json_at_eof`, `scans_across_read_chunk_boundaries`);
  `session_index_tests.rs` (`find_thread_name_by_id_prefers_latest_entry`,
  `find_thread_meta_by_name_str_skips_partial_rollout`,
  `..._skips_newest_entry_without_rollout`, `scan_index_finds_latest_match_among_mixed_entries`);
  `state_db_tests.rs` (`cursor_to_anchor_preserves_recency_tie_breaker`,
  `try_init_waits_for_concurrent_startup_backfill`); `recorder_tests.rs`
  (`state_db_init_backfills_before_returning`); `thread_archive.rs`
  (`thread_archive_requires_materialized_rollout`).

**hermes** is the second SQLite data point: a durable `sessions` table
(`hermes_state.py`, `state.db`) with `parent_session_id`, `archived`, `started_at`,
and `idx_sessions_started ON sessions(started_at DESC)`. Resume-last is a query, not
a scan: `_resolve_last_session` → `search_sessions(limit=1)`, which LEFT-JOINs a
per-session `MAX(messages.timestamp)` as `last_active` and orders
`ORDER BY last_active DESC, s.started_at DESC, s.id DESC` — so "latest" is **most
recent activity**, with `started_at` as tie-breaker. Archive is a recursive
`set_session_archived` over the lineage chain; lineage is `parent_session_id`
(compression-split children). Tests:
`test_resolve_last_session_prefers_last_active_over_started_at`,
`test_search_sessions_exposes_last_active_column` (`test_resolve_last_session.py`);
`test_create_and_get_session`, `test_parent_session` (`test_hermes_state.py`);
`test_archiving_compression_tip_archives_projected_root` (`test_session_archiving.py`).

**opencode** stores sessions in SQLite (drizzle-orm) — `SessionTable` in
`session/sql.ts` (`opencode.db`), no JSONL and no reverse-scan. List/resume are
`ORDER BY time_updated DESC` (`listByProject`/`listGlobal`); `--continue` picks the
first **root** session (`parent_id IS NULL`) off that ordered list
(`cli/cmd/run.ts`); archive is the `time_archived` column
(`listGlobal` filters `time_archived IS NULL`); `fork` records `parent_id`
(`Session.fork`, `getForkedTitle` → `… (fork #N)`). Tests:
`"filters root sessions"`, `"includes metadata in listed sessions"`
(`session-list.test.ts`); `"excludes archived sessions by default"`
(`global-session-list.test.ts`); `"returns the existing Session when one ID is reused"`
(`session-create.test.ts`).

**pi** is the **un-indexed baseline** — it is what agent-seddon's `--continue` does
today. Sessions are per-cwd `*.jsonl` files (no SQLite, no reverse-scan);
`findMostRecentSession` is `readdirSync` + read each header + `statSync().mtime` sort
descending, `files[0]`. `--continue` = `continueRecent`; `--resume` = an interactive
picker over a fully-re-parsed list. Tests:
`"returns most recently modified session"`, `"filters most recent session by cwd"`
(`file-operations.test.ts`);
`"uses last user/assistant message timestamp instead of file mtime"`
(`session-info-modified-timestamp.test.ts`).

Net: codex owns the **file-store + SQLite-index + reverse-scan** trio (local, not a
seam); hermes + opencode own **SQLite listing/resume/archive** (no content-addressed
object DB or GC); pi owns the **mtime-scan baseline**. This spec unifies the index +
reverse-scan onto agent-seddon's *content-addressed, GC'd, gRPC-served* seam.

## Completeness gaps

Behaviour agent-seddon must add to be the most complete (spec only — do **not**
implement here). Each maps to a test case below.

- **`sqlite` `SessionStore` backend.** A new backend in
  [`agent-session`](../../crates/agent-session) behind a `rusqlite` cargo feature, a
  factory line in [`register_builtins`](../../crates/agent-runtime/src/registry.rs)
  gated by the feature, selected by `[session] backend = "sqlite"`. It wraps the
  spec-19 content-addressed object store (still the source of truth) and maintains a
  derived SQLite index alongside it.
- **Durable index table.** A `sessions` table — `id TEXT PRIMARY KEY`, `head`
  (current head `CheckpointId`), `branch`, `turn_count`, `preview`, `cwd`, `parent`
  (fork lineage), `updated_at`, `archived`/`archived_at` — with
  `CREATE INDEX ... ON sessions(updated_at DESC, id DESC)` and an `archived` index,
  **mirroring codex's `idx_threads_updated_at` and hermes's `idx_sessions_started`**.
  Every `checkpoint`/`branch`/`undo`/`fork` updates the row; the object store is
  untouched by the index.
- **Three new trait methods (additive to spec 19's `SessionStore`).**
  `sessions() -> Vec<SessionSummary>` (whole-store catalogue, recency-ordered),
  `latest(filter) -> Option<SessionId>` (indexed resume-last, optional `cwd` filter
  like codex/pi), `archive(id, bool) -> Result<()>` (soft-hide). The `file`/`grpc`
  backends implement them too (`file` by a store walk — its honest slow path;
  `grpc` by delegation), so the seam contract stays uniform. (Port codex
  `find_latest_thread_path` / hermes `search_sessions` / opencode `listGlobal`.)
- **Append-only rollout log + reverse-scan recovery.** Each session gains an
  append-only rollout `.jsonl` (checkpoints appended, never overwritten — unlike
  today's clobbering `save`). A `ReverseJsonlScanner` (64 KiB chunks, from the end,
  skip-and-continue on a bad record) recovers the latest valid session header even
  when the process died mid-write, so `latest()` is robust to a partial tail.
  **Index-first, scan-fallback**: `latest()` queries SQLite; on a miss (empty/stale
  index) it reverse-scans the newest rollout. (Port codex `reverse_jsonl_scanner` +
  `find_latest_thread_path`'s fallback.)
- **Rebuild / backfill (index is a cache, never the truth).** A missing or corrupt
  SQLite file is **rebuilt** by re-scanning the object store / rollout logs — no
  session is ever lost to an index problem. Opening the backend backfills any
  sessions present on disk but absent from the index. (Port codex
  `state_db_init_backfills_before_returning`.)
- **Fork lineage + archive as index ops.** `fork` inserts a child row with `parent`
  set; `archive(id, true)` flips `archived` (listing filters it out by default,
  `archive(id, false)` unhides). Neither rewrites objects. GC/`prune` still runs over
  the content-addressed object DB (spec 19 reachability) and updates the index to
  match — never the reverse. (Port opencode `parent_id`/`time_archived`, hermes
  `set_session_archived`, codex `archive_thread`.)
- **Served, metered, traced.** Additive `session.proto` RPCs
  (`ListSessions`/`Latest`/`Archive`) so a remote `sqlite` store is dialable like the
  `grpc` backend; `agent_session_ops_total{op=list_sessions|latest|archive}` +
  an `agent_session_index_total{outcome=hit|miss|rebuilt}` counter; a
  `session.latest`/`session.list`/`session.archive` span (attrs `session_id`,
  `filter`, `hits`, `scanned`) reusing [`agent-telemetry`](../../crates/agent-telemetry/).
- **Wire `--continue`/`resume --last` to the seam.** The CLI resume path
  ([`main.rs`](../../crates/agent-cli/src/main.rs) `resolve_resume`) calls
  `store.latest(cwd_filter)` instead of the flat `session_store::most_recent` mtime
  scan — so `--continue` finally rides the checkpoint store, and a `--last` positional
  is disambiguated from a prompt the way codex's `resume --last <prompt>` is.

## Table-driven test plan

New `#[rstest]` tables next to the `sqlite` backend in
[`agent-session`](../../crates/agent-session) (index + reverse-scan + backfill),
plus a gRPC roundtrip case. Doubles from
[`agent-testkit`](../../crates/agent-testkit/src/lib.rs): `tempdir()` for the store
root; the spec-19 `ws(&[(Role,&str)])` working-set helper; a `TestClock` (injected
`now()`) so recency ordering is deterministic (advance the clock by hand between
checkpoints — never wall-clock `sleep`). Prefixes: `positive_` succeeds,
`negative_` rejects, `corner_` odd-but-valid, `boundary_` at a limit,
`adversarial_` a hostile input that must be **rejected**. `(port: <peer>)` marks a
case mined from a peer test; `(new: agent-seddon)` are ours.

```rust
// crates/agent-session/src/sqlite.rs — SQLite-indexed SessionStore backend tests.
// Doubles: agent_testkit::{tempdir, TestClock}; local ws(&[(Role,&str)]) helper.

// ---- resume-last: indexed lookup returns the most-recently-updated session ---
#[rstest]
#[tokio::test]
async fn positive_resume_last_returns_most_recent() {                        // (port: codex find_latest_thread_path / hermes search_sessions / opencode ORDER BY time_updated DESC)
    // checkpoint sess A (clock=1), sess B (clock=2), sess A again (clock=3).
    // latest(None) == Some("A"): index ordered by updated_at DESC, not started.
    // assert it was ONE indexed query (agent_session_index_total{outcome="hit"} += 1),
    // no store walk.
}

// ---- resume-last with a cwd filter (codex/pi filter by matching cwd) ---------
#[rstest]
#[case::positive_latest_filters_by_cwd("/repo/a", Some("A"))]                 // (port: codex cwds_match / pi filters most recent by cwd)
#[case::corner_latest_cwd_no_match_returns_none("/repo/z", None)]            // (new: agent-seddon)
#[tokio::test]
async fn latest_cwd_filter_cases(#[case] cwd: &str, #[case] want: Option<&str>) {
    // two sessions with distinct cwds; latest(Some(cwd)) honours the filter.
}

// ---- sessions(): whole-store catalogue, recency-ordered, archived excluded ---
#[rstest]
#[tokio::test]
async fn positive_sessions_listed_newest_first_excludes_archived() {         // (port: opencode "excludes archived sessions by default" / hermes list_sessions_rich)
    // checkpoint A,B,C at clock 1,2,3; archive(B,true).
    // sessions() == [C, A] (B hidden), each summary carries {id, turns, preview,
    // cwd, branch, updated_at}. archive(B,false) -> B reappears between A and C.
}

// ---- fork lineage recorded in the index (one row insert, parent set) --------
#[rstest]
#[tokio::test]
async fn positive_fork_records_parent_in_index() {                           // (port: opencode Session.fork parent_id / codex forked_from_id)
    // checkpoint parent P; fork() -> child C. sessions() shows C with parent==P;
    // a write to C never changes P's head row (fork independence, spec 19).
}

// ---- CORRUPT/PARTIAL rollout tail: reverse-scan recovers the latest session --
#[rstest]
#[tokio::test]
async fn corner_partial_rollout_tail_reverse_scan_recovers() {               // (port: codex reverse_jsonl_scanner rejects_invalid_json_and_continues / session_index skips_partial_rollout)
    // append two valid checkpoint records to the newest session's rollout .jsonl,
    // then a HALF-WRITTEN third line (truncated JSON). Drop the SQLite index to
    // force the scan fallback. latest() reverse-scans from EOF, SKIPS the bad tail
    // (ScanOutcome::Rejected), and returns the last VALID session — not an error,
    // not the skipped/garbage record. index_total{outcome="miss"} then a rebuild.
}

// ---- index rebuild/backfill: corrupt index rebuilds from the object store ----
#[rstest]
#[tokio::test]
async fn corner_missing_index_rebuilds_from_object_store() {                  // (port: codex state_db_init_backfills_before_returning)
    // checkpoint A,B,C; delete the sqlite file; reopen the backend.
    // opening backfills: sessions() still returns {A,B,C} (no session lost),
    // agent_session_index_total{outcome="rebuilt"} += 1. Object store is the truth.
}

// ---- resume-last with ZERO sessions is None, not an error -------------------
#[rstest]
#[tokio::test]
async fn boundary_resume_last_zero_sessions_returns_none() {                  // (new: agent-seddon; cf. pi empty-dir / codex complete-miss)
    // fresh empty store. latest(None) == None (ranked-empty, no panic/err).
    // sessions() == []. --continue on this store is a clean "no prior session".
}

// ---- resume a specific unknown id → typed error ----------------------------
#[rstest]
#[case::negative_restore_unknown_session(Op::Latest, "no-such-sess", Err("not found"))] // (port: opencode/codex miss)
#[tokio::test]
async fn unknown_session_cases(#[case] op: Op, #[case] id: &str, #[case] want: Result<(), &str>) {
    // resolving an explicit, never-created session id is a distinct not-found error.
}

// ---- ADVERSARIAL: malicious session id / path traversal in a session ref ----
#[rstest]
#[case::adversarial_traversal_session_id("../../etc/passwd")]                 // escapes objects/sessions dir
#[case::adversarial_absolute_session_id("/etc/shadow")]                      // absolute path
#[case::adversarial_nul_and_sep_session_id("a\0/b")]                         // nul + separator injection
#[case::adversarial_sqlite_meta_in_id("a'; DROP TABLE sessions;--")]         // SQL-meta in the id
#[tokio::test]
async fn adversarial_session_ref_is_rejected(#[case] id: &str) {
    // every op that takes a session id (latest-not applicable; archive/fork/checkpoint)
    // routes the id through safe_segment (agent-git) BEFORE it becomes a path segment,
    // and binds it as a PARAMETER (never string-interpolated) in every SQL statement.
    // assert: rejected (Err), NO file created/read outside the store root, the
    // sessions table still exists and is intact (no injection took effect).
}
```

gRPC roundtrip (extend
[`agent-grpc/tests/session_roundtrip.rs`](../../crates/agent-grpc/tests/session_roundtrip.rs)):
over TCP **and** UDS, `Checkpoint` two sessions, call the new `ListSessions` and
`Latest` RPCs and assert the newest id comes back ordered, `Archive` one and assert
it drops from the default `ListSessions`, asserting the `sqlite` backend behaves
identically in-process vs. served — the pattern every seam's roundtrip test uses.

Prefix legend (repo convention): `positive_` expected success, `negative_` expected
error, `corner_` odd-but-valid, `boundary_` at a limit, `adversarial_` hostile input
that must be rejected. `(port: <peer>)` names the peer a case was mined from;
`(new: agent-seddon)` marks the zero-session, cwd-no-match, and index-rebuild
assertions with no direct peer analogue.

## Harness obligations

The implementing PR must satisfy all (follows the #21–46 checklist and the
[`23-tokenizer-cost.md`](23-tokenizer-cost.md) backend-behind-a-feature pattern):

- **Backend + trait extension.** Add `sessions`/`latest`/`archive` to the
  `SessionStore` trait in [`agent-core`](../../crates/agent-core/src/lib.rs) with a
  default store-walk impl so `file`/`grpc` compile; a `sqlite` backend in
  [`agent-session`](../../crates/agent-session) behind a `rusqlite` cargo feature
  (default build stays lean — no bundled DB); one factory line in
  [`register_builtins`](../../crates/agent-runtime/src/registry.rs) guarded by the
  feature (`[session] backend = "sqlite"`); the `MeteredSession` decorator in
  [`metered.rs`](../../crates/agent-runtime/src/metered.rs) must **delegate the three
  new methods** (a metered wrapper that silently drops a seam method is the recurring
  bug — cf. tokenizer `count_batch`); update
  [`docs/components/session.md`](../components/session.md) with the new backend + the
  index-is-a-cache invariant.
- **Proto + gRPC.** Add `ListSessions`/`Latest`/`Archive` RPCs + messages to
  [`session.proto`](../../crates/agent-proto/proto/agent/v1/session.proto)
  **additively** (a new RPC passes `buf breaking` untouched), server/client in
  `agent-grpc`, reflection; commit the `crates/agent-proto/buf.image.binpb` bump via
  `nix run .#buf-image`; extend the roundtrip test.
- **CLI wiring.** Route `resolve_resume` in
  [`main.rs`](../../crates/agent-cli/src/main.rs) through `store.latest(cwd_filter)`
  (retiring the flat `session_store::most_recent` mtime scan for the resume path),
  with `--last` disambiguated from a prompt positional.
- **Metrics + OTel.** `agent_session_ops_total{op=list_sessions|latest|archive}` and
  a new `agent_session_index_total{outcome=hit|miss|rebuilt}` counter in
  [`agent-metrics`](../../crates/agent-metrics/src/lib.rs); a
  `session.latest`/`session.list`/`session.archive` span (attrs `session_id`,
  `filter`, `hits`, `scanned`) reusing [`agent-telemetry`](../../crates/agent-telemetry/).
- **Bench.** The genuine deterministic CPU hot path is the **reverse-JSONL scan of a
  rollout tail** (fixed bytes in → fixed records out → stable instruction count) —
  an iai-callgrind bench with an absolute Ir ceiling in `nix/checks/bench.nix`,
  mirroring codex's `reverse_jsonl_scanner`. The SQLite query/backfill path is
  I/O-bound (like the `05-text-search`/tantivy path) — **document the bench skip**.
- **Leak.** A dhat `tests/leak.rs` (`dhat-heap` feature) over the
  open → checkpoint → `latest`/`sessions` → close path, asserting the index handle,
  the reverse-scan chunk buffer (64 KiB), and the query result set are freed each
  iteration and stay under budget — and that a **rebuild/backfill** over N sessions
  stays within an allocation budget (the alloc-heavy scan path).

## References

- **agent-seddon:**
  [`crates/agent-session/src/file.rs`](../../crates/agent-session/src/file.rs) (`FileSessionStore` — content-addressed objects + per-session heads; the store this backend indexes),
  [`crates/agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs) (`SessionStore` trait ~L1238 — extend with `sessions`/`latest`/`archive`; `WorkingSet`/`CheckpointId`),
  [`crates/agent-runtime/src/session_store.rs`](../../crates/agent-runtime/src/session_store.rs) (flat `save`/`load`/`list`/`most_recent` — the mtime-scan resume path this seam supersedes),
  [`crates/agent-cli/src/main.rs`](../../crates/agent-cli/src/main.rs) (`resolve_resume`, `--continue`/`--resume` — route through `latest`),
  [`crates/agent-proto/proto/agent/v1/session.proto`](../../crates/agent-proto/proto/agent/v1/session.proto) (`SessionService` — add `ListSessions`/`Latest`/`Archive`),
  [`crates/agent-grpc/src/client/session.rs`](../../crates/agent-grpc/src/client/session.rs) (`GrpcSession` client),
  [`crates/agent-grpc/tests/session_roundtrip.rs`](../../crates/agent-grpc/tests/session_roundtrip.rs) (roundtrip pattern),
  [`crates/agent-runtime/src/registry.rs`](../../crates/agent-runtime/src/registry.rs) (`register_builtins`),
  [`crates/agent-runtime/src/metered.rs`](../../crates/agent-runtime/src/metered.rs) (metered decorator — must delegate new methods),
  [`crates/agent-metrics/src/lib.rs`](../../crates/agent-metrics/src/lib.rs), [`crates/agent-telemetry/`](../../crates/agent-telemetry/), [`crates/agent-testkit/src/lib.rs`](../../crates/agent-testkit/src/lib.rs) (`tempdir`, doubles); dependencies: [`19-session-checkpoint.md`](19-session-checkpoint.md) (the checkpoint store), [`20-session-export.md`](20-session-export.md) (recall over the same sessions), and [`docs/components/session.md`](../components/session.md).
- **codex (anchor):** `codex-rs/rollout/src/state_db.rs` + `codex-rs/state/migrations/0001_threads.sql` (`threads` SQLite index: `updated_at`/`archived`/`archived_at`/`rollout_path`, `idx_threads_updated_at ON threads(updated_at DESC, id DESC)`), `codex-rs/rollout/src/recorder.rs` (`find_latest_thread_path` — index-first, scan-fallback), `codex-rs/rollout/src/reverse_jsonl_scanner.rs` (`ReverseJsonlScanner`, `ScanOutcome`, `READ_CHUNK_SIZE = 64 KiB`, `new_at`), `codex-rs/rollout/src/session_index.rs` (`session_index.jsonl` append-only, scan-from-end), `codex-rs/thread-store/src/local/{archive_thread,unarchive_thread,delete_thread}.rs`, `codex-rs/exec/src/cli.rs` (`resume --last`); tests `codex-rs/rollout/src/reverse_jsonl_scanner_tests.rs` (`scans_jsonl_records_from_end`, `rejects_invalid_json_and_continues_scanning`, `accepts_valid_json_at_eof`, `scans_across_read_chunk_boundaries`), `codex-rs/rollout/src/session_index_tests.rs` (`find_thread_meta_by_name_str_skips_partial_rollout`, `find_thread_name_by_id_prefers_latest_entry`), `codex-rs/rollout/src/state_db_tests.rs` (`cursor_to_anchor_preserves_recency_tie_breaker`), `codex-rs/rollout/src/recorder_tests.rs` (`state_db_init_backfills_before_returning`), `codex-rs/app-server/tests/suite/v2/thread_archive.rs`.
- **opencode:** `packages/core/src/session/sql.ts` (`SessionTable`: `time_updated`/`time_archived`/`parent_id`, `session_parent_idx`), `packages/core/src/database/database.ts` (`opencode.db`), `packages/opencode/src/session/session.ts` (`listByProject`/`listGlobal` `ORDER BY time_updated DESC`, `fork`/`getForkedTitle`), `packages/opencode/src/cli/cmd/run.ts` (`--continue` = first root of the list); tests `packages/opencode/test/server/session-list.test.ts` (`"filters root sessions"`, `"includes metadata in listed sessions"`), `packages/opencode/test/server/global-session-list.test.ts` (`"excludes archived sessions by default"`), `packages/core/test/session-create.test.ts`.
- **pi:** `pi/packages/coding-agent/src/core/session-manager.ts` (`findMostRecentSession` = `readdirSync` + `statSync().mtime` sort; per-cwd `*.jsonl`; **no SQLite, no reverse-scan**), `.../cli/args.ts` (`--continue`/`--resume`), `.../main.ts` (`continueRecent`); tests `pi/packages/coding-agent/test/session-manager/file-operations.test.ts` (`"returns most recently modified session"`, `"filters most recent session by cwd"`), `.../test/session-info-modified-timestamp.test.ts` (`"uses last user/assistant message timestamp instead of file mtime"`), `.../test/session-selector-search.test.ts`.
- **hermes:** `hermes-agent/hermes_state.py` (SQLite `sessions` table: `started_at`/`ended_at`/`parent_session_id`/`archived`, `idx_sessions_started ON sessions(started_at DESC)`; `search_sessions` `ORDER BY last_active DESC, started_at DESC`; `set_session_archived`; `resolve_resume_session_id`), `hermes-agent/hermes_cli/main.py` (`_resolve_last_session` → `--continue`); tests `hermes-agent/tests/hermes_cli/test_resolve_last_session.py` (`test_resolve_last_session_prefers_last_active_over_started_at`, `test_search_sessions_exposes_last_active_column`), `hermes-agent/tests/test_hermes_state.py` (`test_create_and_get_session`, `test_parent_session`), `hermes-agent/tests/hermes_state/test_session_archiving.py`, `hermes-agent/tests/hermes_state/test_resolve_resume_session_id.py`.
