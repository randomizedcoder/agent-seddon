# Parity spec 40 — turn-diff tracker + apply

Per-feature parity spec for a **`TurnDiff` seam**: accumulate a whole turn's file
mutations into one net, reviewable unified diff, and offer an `apply` that
`git apply`s the agent's last change set onto a clean tree — so a turn's work is a
single inspectable, replayable, remotable artifact instead of a scatter of
individual `edit`/`apply_patch`/`write_file` side effects, and a review flow (or a
human) can green-light or replant the whole change set at once, the way `codex
apply` git-applies an agent's produced diff.

> **Status: ⬜ spec written, not started.** Proposed new **`TurnDiff` seam**
> (async trait in `agent-core`: `track`/`net_diff`/`reset`/`apply`) with a
> dependency-light impl in a new **`agent-turndiff`** crate that observes each
> file-mutating tool result and folds it into a per-turn baseline→current map,
> rendering a **net** unified diff (a file created and later deleted in the same
> turn nets to *nothing*) without rereading the workspace. Surfaced as an `apply`
> **tool** + a `--apply` **CLI subcommand** (git-applies the agent's last turn
> diff to the current tree), config-selected via **`[turndiff] backend`**. Every
> `apply` target passes through [`confine()`](../../crates/agent-tools/src/lib.rs)
> (the model authored the change set; the diff's paths are attacker-controlled).
> Metered (`turndiff_*` families) + a `turndiff.<op>` span, and it **pairs with the
> [`SessionStore`](19-session-checkpoint.md) checkpoints** so each turn's file
> change set is a first-class object alongside its conversation checkpoint.
> **Deferred:** a `turndiff.proto` gRPC service (`--serve-turndiff`, reflection) so
> a remote reviewer can pull a turn diff and `Apply` it over the wire; coupling the
> net diff to the `RepoBackend` code checkpoint (spec 19) so `apply`/revert move the
> filesystem and the session head together; and a `revert` (inverse `git apply -R`)
> once `apply` lands. Cross-refs: [`02-patch-diff-editing.md`](02-patch-diff-editing.md)
> (`apply_patch` — the model *writing* a multi-file diff, which this seam *reads back
> and accumulates*) and [`19-session-checkpoint.md`](19-session-checkpoint.md) (the
> per-turn conversation checkpoint this pairs a per-turn *file* change set with).
> **Unimplemented** — unlike the fundamentals (specs 01–10) and `apply_patch`
> (spec 02), the `TurnDiff` trait, the tracker impl, the `apply` tool/CLI, and the
> metrics/span wiring do not exist yet; this is the design of record.

## Feature & why it matters

A single agent turn routinely mutates the tree through **several** tool calls:
`write_file` scaffolds a module, two `edit`s fix its callers, an `apply_patch`
rewrites a config, a `bash rm` drops a stale file. Today each of those is an
isolated `Observation`; nothing composes them into "here is what this turn did to
your files." That missing artifact costs three things:

- **Reviewability.** To see a turn's net effect you must eyeball N tool
  observations and reconstruct the delta in your head — and a churny turn that
  edits the same file five times shows you five partial hunks, not the one net
  change. A single unified diff per turn is what a human (or the
  [code-review flow](../design/code-review/README.md)) actually wants to approve.
- **Net semantics.** Intermediate churn is noise. A file the turn **creates and
  then deletes** should net to *nothing*; five edits to one file should net to one
  hunk against its start-of-turn baseline; a rename-then-edit should read as a
  rename with a hunk. Only an accumulator that folds every mutation against a
  **start-of-turn baseline** can report the net, and it must do so **without
  rereading the workspace** (the on-disk file already reflects later, unrelated
  writes).
- **`apply` — replant the last change set.** Given that net diff, the natural
  companion is `codex apply`'s move: take the agent's last produced diff and
  `git apply` it onto a *clean* checkout elsewhere (a colleague's tree, CI, a fresh
  worktree) — turning "the agent did some work in its sandbox" into "here is a patch
  you can land." That is impossible today because there is no per-turn diff to apply.

The unit is the **turn**, not the tool call: open a baseline at turn start, fold in
each mutation, render one net diff at turn end, and optionally `apply` it. This is a
**read-back-and-accumulate** layer sitting *above* the write tools — the exact
inverse of `apply_patch` (spec 02), which is the model *authoring* one multi-file
diff. It complements checkpoints (spec 19): a checkpoint captures the *conversation*
after a turn; this captures the *files* the turn changed, so the two together make a
turn a fully reproducible unit.

## agent-seddon today

**No turn-level file-diff accumulation and no `apply` of a change set exist.** The
building blocks are present but wired for other jobs:

- **`apply_patch` writes a model-authored envelope, it does not accumulate.**
  [`ApplyPatchTool`](../../crates/agent-tools/src/patch.rs)
  (`crates/agent-tools/src/patch.rs`) parses a `*** Begin Patch … *** End Patch`
  envelope the **model wrote**, validates every op against the current tree, and
  applies add/update/delete **atomically** — that is the model *emitting* one
  multi-file diff, the opposite direction from folding *many* tool results into one
  net diff. See [`02-patch-diff-editing.md`](02-patch-diff-editing.md). The write
  tools it sits beside — [`EditTool`](../../crates/agent-tools/src/edit.rs) and
  `write_file` — each return a standalone `Observation` with no shared per-turn view.
- **Checkpointing captures conversation, not the file change set.**
  `auto_checkpoint` (in [`config.rs`](../../crates/agent-runtime/src/config.rs))
  fires after each turn; the `CheckpointStore` seam
  (`checkpoint`/`restore`/`undo`/`diff` in [`agent-core`](../../crates/agent-core/src/lib.rs))
  is backed by git **private refs** in [`agent-git`](../../crates/agent-git). But the
  checkpointed `WorkingSet` is `messages: Vec<Message>`
  ([`agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs), ~L2098) — a
  checkpoint captures **conversation history**, so `undo`/`diff` operate on messages,
  not on "what files did this turn change." The git private-ref machinery could
  *back* a per-turn file diff, but nothing computes or exposes one.
- **`bash` can `git diff`, but that is the workspace, not the turn.** A model can
  shell out to `git diff`, yet that reports the working tree against `HEAD` (all
  uncommitted work, including edits from prior turns and unrelated churn), with no
  notion of a per-turn baseline and no net-over-a-turn semantics.
- **The seam + metered + span pattern to reuse exists.** A `TurnDiff` trait wired via
  [`register_builtins`](../../crates/agent-runtime/src/registry.rs), a `MeteredTurnDiff`
  following [`metered.rs`](../../crates/agent-runtime/src/metered.rs), and a
  `turndiff.<op>` span following the #44 span-attribute pattern reuse
  [`agent-metrics`](../../crates/agent-metrics/src/lib.rs) +
  [`agent-telemetry`](../../crates/agent-telemetry/) verbatim. Path safety already has
  its guard: [`confine()`](../../crates/agent-tools/src/lib.rs) canonicalizes to block
  symlink escape, the exact gate `apply` must run every diff path through.

Honest gap: everything above is *reusable scaffolding*. The `TurnDiff` trait, the
baseline→current accumulator that folds tool mutations into a **net** diff, the
render-without-reread logic, the `apply` tool + `--apply` CLI, the git-apply-to-a-
clean-tree path, and the metrics/span wiring **do not exist yet**.

## Peer implementations & their tests

| Peer | Impl path | Test path | Framework |
| --- | --- | --- | --- |
| codex | `codex-rs/core/src/turn_diff_tracker.rs` (`TurnDiffTracker`: `track_delta`/`get_unified_diff`, baseline→current per path, net diff w/o reread) + `codex-rs/chatgpt/src/apply_command.rs` (`ApplyCommand`, `git apply` the agent's diff; CLI `apply`, `visible_alias = "a"`) + `/diff` TUI (`tui/src/slash_command.rs`) | `codex-rs/core/src/turn_diff_tracker_tests.rs` | cargo `#[tokio::test]` (plain `assert!`; codex core uses `insta` elsewhere) |
| opencode | `packages/core/src/snapshot.ts` (`Snapshot.Service`: `capture` two trees to a shadow git repo, `diff(from,to) → File.Diff[]`) — a *snapshot-pair* diff, not a per-turn tool accumulator, and **no `apply`** | `packages/core/test/git.test.ts` (`git.tree.diff` over a `.snapshot` shadow repo, ~L148) | bun:test + Effect |
| pi | — (no session/turn diff or change-set `apply`; only a per-edit-call renderer, `packages/coding-agent/src/core/tools/edit-diff.ts`, via the `diff` lib — one edit at a time, no turn accumulation, no `git apply`) | — | vitest |
| hermes | `agent/display.py` (`capture_local_edit_snapshot` per edit-call + `_diff_from_snapshot`/`extract_edit_diff`, `difflib.unified_diff`) — a *per-edit-call* display diff, not turn-accumulated, **no `apply`** | `tests/agent/test_display.py` | pytest |

**codex is the deep anchor** — it is the only peer that both accumulates a whole
turn's mutations into one net diff *and* ships a change-set `apply`, and it pins
exactly the behaviours this spec needs:

- **Accumulate over a turn, net semantics** (`turn_diff_tracker.rs`): a
  `TurnDiffTracker` holds `baseline_by_path` + `current_by_path` and folds each
  committed `apply_patch` mutation via `track_delta(env_id, delta)` →
  `apply_add`/`apply_update`/`apply_delete`; `get_unified_diff()` renders the net
  `baseline → current` diff. Crucially it tracks **from committed mutations, without
  rereading the workspace filesystem** (its own doc-comment) — later unrelated writes
  don't pollute the turn's diff. Tests: `accumulates_add_then_update_as_single_add`
  (an add followed by an update of the same path reads as one add of the final
  content), `accumulates_delete`, `accumulates_move_and_update`,
  `repeated_updates_only_rerender_the_touched_path`.
- **Net-to-nothing / net-to-update corners** — `pure_rename_yields_no_diff` (a rename
  with no content change nets to no diff), `delete_then_readd_same_path_becomes_update`
  and `add_over_existing_file_becomes_update` (churn collapses to its net kind). These
  are the exact "creates+deletes same file → nothing" family.
- **Content-addressed identity** (`git_blob_oid`, sha1) so unchanged paths short-circuit
  (`reuses_rendered_diffs_for_unchanged_paths`) and the render is cached per
  `DiffCacheKey`.
- **Bounded / promptly-terminating render** — a `DIFF_TIMEOUT` (100 ms) falls back to a
  coarse content-exact diff rather than stalling turn completion on a pathological
  input; `large_rewrite_returns_promptly_and_preserves_exact_content` pins it.
- **Invalidation** — `invalidate()` suppresses a stale diff
  (`invalidated_tracker_suppresses_existing_diff`); multi-root tracking
  (`tracks_same_absolute_path_across_multiple_environments`,
  `displays_foreign_paths_relative_to_their_environment_root`).
- **Surface it** — the tracked net diff is emitted at turn end as a
  `TurnDiffEvent { unified_diff }` (`core/src/session/turn.rs` ~L2737) and the `/diff`
  TUI command shows the git diff (`tui/src/slash_command.rs` — *"show git diff
  (including untracked files)"*).
- **`apply` the agent's diff** (`chatgpt/src/apply_command.rs`): `ApplyCommand` (CLI
  `apply`, **`visible_alias = "a"`**) fetches the agent's latest produced diff and
  runs `apply_git_patch(ApplyGitRequest { cwd, diff, revert:false, preflight:false })`
  from `codex-rs/git-utils`, reporting applied/skipped/conflicted paths — *"Apply the
  latest diff produced by Codex agent as a `git apply` to your local working tree."*
  This is the `apply` tool/CLI move agent-seddon mirrors (over the *local* turn diff,
  not a cloud task).

**opencode** is a second data point but a shallower one: `Snapshot.Service`
(`snapshot.ts`) `capture`s a working tree into a `.snapshot` shadow git repo and
`diff(from, to)` renders per-file diffs between two captures (`git.tree.diff`,
tested in `git.test.ts` ~L148). That yields a between-two-points diff, but it is
snapshot-pair based (not a per-tool-call turn accumulator) and offers **no `apply`
of a change set**. **hermes** (`display.py`) captures a per-edit-call before-state
(`capture_local_edit_snapshot`) and renders one edit's `difflib.unified_diff` for
display (`_diff_from_snapshot`, tested in `test_display.py`) — again per single tool
call, with no whole-turn accumulation and no `apply`. **pi** has neither a
session/turn diff nor a change-set `apply` — only `edit-diff.ts`, a per-edit
renderer over the `diff` library — so it is marked "—". This is a feature where
codex is the sole deep anchor, opencode/hermes contribute the "diff between two
captured states" idea, and agent-seddon can leapfrog all three on **distribution**
(the deferred `turndiff.proto` + reflection), **observability** (metered
turn-diff + span), and **coupling to session checkpoints** (spec 19).

## Completeness gaps

Behaviour agent-seddon must add to be the most complete (spec only — do **not**
implement here). Each maps to a test case below.

- **`TurnDiff` seam.** New async trait in `agent-core`: `track(mutation)` (fold one
  file mutation — add/update/delete/rename with before/after content — into the turn),
  `net_diff() -> Option<UnifiedDiff>` (the accumulated net diff, `None` if the turn
  netted to nothing), `reset()` (start a new turn's baseline), `apply(diff, dst) ->
  ApplyReport` (git-apply a diff onto a target tree). Impl in a new `agent-turndiff`
  crate behind a cargo feature; one factory line in
  [`register_builtins`](../../crates/agent-runtime/src/registry.rs); config-selected via
  `[turndiff] backend`. (Port codex `TurnDiffTracker`.)
- **Baseline→current accumulation, net semantics, no reread.** Hold a per-path
  `baseline` (start-of-turn content) + `current` (latest); fold each mutation into
  `current` **without rereading the workspace**, so unrelated later writes never leak
  in. The rendered diff is `baseline → current` per path; a path whose current equals
  its baseline (created-then-deleted, or pure rename) contributes **no hunk**.
  (Port codex `apply_add`/`apply_update`/`apply_delete`, `pure_rename_yields_no_diff`.)
- **Churn collapse.** N edits to one file → one hunk vs. its baseline;
  add-then-update → one add of the final content; delete-then-readd → an update;
  add-over-existing → an update. (Port codex `accumulates_add_then_update_as_single_add`,
  `delete_then_readd_same_path_becomes_update`, `add_over_existing_file_becomes_update`.)
- **Content-addressed short-circuit + bounded render.** Hash each blob (sha1/blake3) so
  an unchanged path skips re-render; render under a **timeout** with a coarse
  content-exact fallback for pathological inputs, never stalling turn completion; cap
  the rendered diff size (`truncate`, like every built-in). (Port codex `git_blob_oid`,
  `DIFF_TIMEOUT`, `reuses_rendered_diffs_for_unchanged_paths`.)
- **`apply` tool + `--apply` CLI.** An `apply` **tool** (`agent_core::Tool`) and an
  `agent --apply` **subcommand** that take the last turn's net diff (or a supplied
  diff) and `git apply` it onto the current/target tree, reporting
  applied/skipped/conflicted paths — the local analogue of `codex apply`. Selectable by
  name via `[tools] enabled`. (Port codex `ApplyCommand`/`run_apply_command`.)
- **Path confinement on apply (the diff is attacker-controlled).** The model authored
  the change set, so **every path a diff touches** is untrusted: each target resolves
  through [`confine()`](../../crates/agent-tools/src/lib.rs) (canonicalize, block
  symlink escape) before any write, and a diff that references a path **outside the
  workspace root** is **refused with nothing applied** — mirroring `apply_patch`'s
  path-escape rejection (spec 02) and the repo's fail-closed rule. `git apply` runs
  `--check` (preflight) first so a partial apply can't land from a bad diff. (New —
  the security-critical path; no peer confines the apply target.)
- **Empty / no-op turns.** A turn that mutated no files (or whose mutations netted to
  nothing) yields `net_diff() == None` and an `apply` of it is a no-op, not an error.
  (Port codex net-to-nothing; new boundary framing.)
- **Metered + spanned (differentiator).** `turndiff_tracked_total{kind=add|update|delete|rename}`,
  `turndiff_net_files` / `turndiff_net_bytes` gauges (per turn), a
  `turndiff_apply_total{outcome=applied|conflict|refused}` counter, and a per-turn
  `turndiff.turn` span (attrs `turn_id`, `files_changed`, `net_bytes`, `applied`,
  `conflicts`) reusing [`agent-metrics`](../../crates/agent-metrics/src/lib.rs) +
  [`agent-telemetry`](../../crates/agent-telemetry/). (New — no peer analogue.)
- **Pairs with `SessionStore` (spec 19).** At turn end the net diff is recorded
  *alongside* the conversation checkpoint so a turn is a reproducible unit
  (conversation + file change set); the deferred `RepoBackend` coupling lets a
  `restore`/`undo` optionally move the filesystem via `apply -R`. Cite
  [`19-session-checkpoint.md`](19-session-checkpoint.md). (New.)
- **gRPC service (deferred, noted).** `turndiff.proto` with `Track`/`NetDiff`/`Reset`/
  `Apply` RPCs + reflection + `--serve-turndiff` so a remote reviewer can pull a turn
  diff and `Apply` it over the wire, dialable like any other seam.

## Table-driven test plan

New `#[rstest]` tables in the `agent-turndiff` crate (accumulation + render + apply),
plus an `apply`-tool table and a gRPC roundtrip stub for the deferred service. The
accumulator is pure (in-memory baseline→current), so most cases need no filesystem;
the `apply` cases use a `tempdir()` git tree. Doubles from
[`agent-testkit`](../../crates/agent-testkit/src/lib.rs): `tempdir()` for the apply
target tree and a tiny `mutation(kind, path, before, after)` builder to feed
`track`. Prefixes: `positive_` succeeds, `negative_` rejects, `corner_`
odd-but-valid, `boundary_` at a limit; `adversarial_` (mandatory for the untrusted
apply path) asserts the rejection. `(port: codex)` marks cases mined from
`turn_diff_tracker_tests.rs`; `(new: agent-seddon)` are ours.

```rust
// crates/agent-turndiff/src/lib.rs — TurnDiff accumulation + render.
// Helper: `track_all(&[Mutation]) -> TurnDiff`, then assert on `net_diff()`.

#[rstest]
// ---- accumulate several mutations → one net diff ---------------------------
#[case::positive_add_then_update_is_single_add(
    &[Mut::add("m.rs", "v1\n"), Mut::update("m.rs", "v1\n", "v2\n")],
    Expect::net_diff_is_add("m.rs", "v2\n"))]                     // (port: codex accumulates_add_then_update_as_single_add)
#[case::positive_repeated_edits_collapse_to_one_hunk(
    &[Mut::update("a.rs", "0\n", "1\n"),
      Mut::update("a.rs", "1\n", "2\n"),
      Mut::update("a.rs", "2\n", "3\n")],
    Expect::net_diff_one_hunk("a.rs", /*from=*/ "0\n", /*to=*/ "3\n"))] // (port: codex repeated_updates_only_rerender_the_touched_path)
#[case::positive_delete_reports_removed_lines(
    &[Mut::delete("old.rs", "fn old() {}\n")],
    Expect::net_diff_deletes("old.rs", "fn old() {}"))]           // (port: codex accumulates_delete)
#[case::positive_move_then_update_reads_as_rename_plus_hunk(
    &[Mut::rename("a.rs", "b.rs"), Mut::update("b.rs", "x\n", "y\n")],
    Expect::net_diff_rename_and_hunk("a.rs", "b.rs"))]            // (port: codex accumulates_move_and_update)
// ---- CORNER: create + delete same file in one turn → nets to NOTHING -------
#[case::corner_create_then_delete_nets_to_nothing(
    &[Mut::add("scratch.rs", "temp\n"), Mut::delete("scratch.rs", "temp\n")],
    Expect::net_diff_none())]                                     // (port: codex pure_rename_yields_no_diff family)
#[case::corner_pure_rename_no_content_change_no_hunk(
    &[Mut::rename("a.rs", "b.rs")],
    Expect::net_diff_rename_only("a.rs", "b.rs"))]                // (port: codex pure_rename_yields_no_diff)
#[case::corner_delete_then_readd_becomes_update(
    &[Mut::delete("f.rs", "old\n"), Mut::add("f.rs", "new\n")],
    Expect::net_diff_is_update("f.rs", "old\n", "new\n"))]        // (port: codex delete_then_readd_same_path_becomes_update)
// ---- BOUNDARY: empty turn → empty diff -------------------------------------
#[case::boundary_empty_turn_yields_none(
    &[],
    Expect::net_diff_none())]                                     // (new: agent-seddon; codex net-to-nothing)
// ---- BOUNDARY: net-unchanged content → None even after churn ---------------
#[case::boundary_edit_and_revert_within_turn_nets_none(
    &[Mut::update("a.rs", "same\n", "changed\n"),
      Mut::update("a.rs", "changed\n", "same\n")],
    Expect::net_diff_none())]                                     // (new: agent-seddon)
#[tokio::test]
async fn accumulate_cases(#[case] muts: &[Mut], #[case] expect: Expect) {
    // Fold `muts` into a fresh TurnDiff (in-memory, no workspace reread);
    // assert net_diff() matches (None, single-add, one-hunk, rename±hunk, …)
    // and that turndiff_tracked_total{kind} counted each fold.
}

// crates/agent-turndiff/src/apply.rs — apply the last turn diff onto a tree.
// Doubles: agent_testkit::tempdir() seeded as a git tree.

#[rstest]
// ---- POSITIVE: git-apply the net diff onto a clean tree --------------------
#[case::positive_apply_net_diff_to_clean_tree(
    /*seed=*/ &[("m.rs", "v1\n")],
    /*turn=*/ &[Mut::update("m.rs", "v1\n", "v2\n")],
    Expect::applied(&[("m.rs", "v2\n")]))]                        // (port: codex ApplyCommand git apply)
// ---- CORNER: applying an empty/None turn diff is a no-op --------------------
#[case::corner_apply_empty_turn_is_noop(
    &[("m.rs", "v1\n")],
    &[],  // empty turn
    Expect::noop())]                                              // (new: agent-seddon)
// ---- NEGATIVE: non-matching context → preflight fails, nothing lands -------
#[case::negative_apply_conflict_reports_and_leaves_tree(
    &[("m.rs", "DIFFERENT\n")],                                   // baseline drifted
    &[Mut::update("m.rs", "v1\n", "v2\n")],
    Expect::conflict_tree_unchanged())]                           // (port: codex apply_git_patch skipped/conflicted)
#[tokio::test]
async fn apply_cases(#[case] seed: &[(&str,&str)],
                     #[case] turn: &[Mut], #[case] expect: Expect) {
    // Seed a tempdir git tree; fold `turn`; apply net_diff() with --check first.
    // Assert applied bytes / no-op / conflict-with-tree-untouched, and
    // turndiff_apply_total{outcome} counted.
}

// ---- ADVERSARIAL (mandatory): apply must confine paths ---------------------
#[rstest]
#[case::adversarial_apply_path_escape_refused(
    "--- a/../../etc/evil\n+++ b/../../etc/evil\n@@\n+pwned\n")]  // (new: agent-seddon; reuse confine())
#[case::adversarial_apply_absolute_path_refused(
    "--- a/etc/passwd\n+++ b/etc/passwd\n@@\n+root::0:0\n")]      // (new: agent-seddon)
#[case::adversarial_apply_symlink_target_refused(
    /* diff whose path resolves through a symlink out of the workspace */ SYMLINK_ESCAPE)]
#[tokio::test]
async fn adversarial_apply_confines_paths(#[case] hostile_diff: &str) {
    // apply(hostile_diff, workspace_root): every target runs through confine();
    // a path resolving OUTSIDE the workspace root is REFUSED with NOTHING written
    // (git apply --check never runs the commit phase), matching apply_patch's
    // path-escape rejection (spec 02) and the fail-closed rule. Assert the target
    // outside root is untouched and turndiff_apply_total{outcome="refused"} += 1.
}
```

A small `apply`-**tool** table mirrors the CLI over `agent_core::Tool`: registered as
the single tool `apply`, it applies the session's last turn diff and returns a capped
per-file summary (`A/M/D` + applied/skipped/conflicted), exactly as `apply_patch`
returns its per-file summary — asserting the tool and the `--apply` subcommand share
one code path. The deferred `turndiff.proto` roundtrip (extend
[`crates/agent-grpc/tests/roundtrip.rs`](../../crates/agent-grpc/tests/roundtrip.rs))
is noted but **not** built here: `Track` a mutation, `NetDiff`, `Apply` over TCP+UDS,
asserting the seam is identical in-process vs. served (the pattern every seam's
roundtrip uses).

Prefix legend (repo convention): `positive_` expected success, `negative_` expected
error, `corner_` odd-but-valid, `boundary_` at a limit, `adversarial_` untrusted
input that **must** be rejected. `(port: codex)` names the peer a case was mined
from; `(new: agent-seddon)` marks the empty-turn boundary, no-op apply, metered
counters, and the path-confinement adversarial sweep that have no peer analogue.

## Harness obligations

The implementing PR must satisfy all (follows the #21–45 checklist):

- **Seam + registry:** `TurnDiff` trait in `agent-core`; the accumulator + apply impl
  in a new `agent-turndiff` crate behind a cargo feature (net baseline→current fold,
  content-addressed short-circuit, timed render fallback); one factory line in
  [`register_builtins`](../../crates/agent-runtime/src/registry.rs) (config-selected
  via `[turndiff] backend`); a `MeteredTurnDiff` in
  [`metered.rs`](../../crates/agent-runtime/src/metered.rs); the `apply` **tool** in
  `agent-tools` + an `agent --apply` subcommand sharing one apply path; doc in
  `docs/components/turndiff.md`.
- **Path safety (load-bearing):** every path in an applied diff resolves through
  [`confine()`](../../crates/agent-tools/src/lib.rs) before any write; a diff touching
  outside the workspace root is refused with nothing applied; `git apply --check`
  preflights so a bad diff never half-lands — the adversarial table is mandatory.
- **Wire up accumulation:** the tracker observes each file-mutating tool result
  (`edit`/`apply_patch`/`write_file`/`bash`-rm) at the runtime seam and folds it in
  **without rereading the workspace**; at turn end it records the net diff **alongside
  the `SessionStore` checkpoint** (spec 19) so a turn is conversation + file change set.
- **Metrics + OTel:** `turndiff_tracked_total{kind}`, `turndiff_net_files` /
  `turndiff_net_bytes` gauges, `turndiff_apply_total{outcome}` counter in
  [`agent-metrics`](../../crates/agent-metrics/src/lib.rs); a per-turn `turndiff.turn`
  span (attrs `turn_id`, `files_changed`, `net_bytes`, `applied`, `conflicts`) reusing
  [`agent-telemetry`](../../crates/agent-telemetry/) — the metered-turn-diff
  differentiator.
- **Bench:** an iai-callgrind bench for the genuine CPU hot path — **folding N
  mutations into the net diff + rendering the unified diff** (deterministic, the
  per-turn cost) — with an Ir ceiling in `nix/checks/bench.nix`; the `git apply` /
  gRPC / disk paths document the skip (I/O- and subprocess-bound), as `bash` did in
  [`04-shell-bash.md`](04-shell-bash.md).
- **Leak:** a dhat `tests/leak.rs` (`dhat-heap` feature) over the
  **track → net_diff → reset** path, asserting a turn frees its baseline/current maps
  and rendered-diff cache on `reset` and stays under budget under a churny turn
  (many edits to a few files) — the accumulator's maps are the leak-sensitive surface.
- **Proto (deferred, staged):** `crates/agent-proto/proto/agent/v1/turndiff.proto`
  (`Track`/`NetDiff`/`Reset`/`Apply`) + `build.rs` + server/client + `--serve-turndiff`
  + reflection; commit the `buf.image.binpb` bump (`nix run .#buf-image`); add the
  endpoint to `nix/constants.nix` → `nix run .#gen-constants`; extend the gRPC
  roundtrip — noted here, built in the follow-up increment.

## References

- **agent-seddon:**
  [`crates/agent-tools/src/patch.rs`](../../crates/agent-tools/src/patch.rs) (`ApplyPatchTool` — the model *authoring* a multi-file diff, the inverse of this accumulator),
  [`crates/agent-tools/src/edit.rs`](../../crates/agent-tools/src/edit.rs) / `write_file` (the per-call write tools whose results this folds),
  [`crates/agent-tools/src/lib.rs`](../../crates/agent-tools/src/lib.rs) (`confine()` — the path gate every apply target passes through),
  [`crates/agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs) (`WorkingSet` `messages: Vec<Message>` ~L2098 — checkpoints capture conversation, not the file change set; `CheckpointStore` seam to pair with),
  [`crates/agent-runtime/src/config.rs`](../../crates/agent-runtime/src/config.rs) (`auto_checkpoint` — the per-turn hook to pair with),
  [`crates/agent-git`](../../crates/agent-git) (git private-refs backing checkpoints; the `git apply` home),
  [`crates/agent-runtime/src/registry.rs`](../../crates/agent-runtime/src/registry.rs) (`register_builtins`),
  [`crates/agent-runtime/src/metered.rs`](../../crates/agent-runtime/src/metered.rs) (metered-seam pattern),
  [`crates/agent-metrics/src/lib.rs`](../../crates/agent-metrics/src/lib.rs) (gauges/counters to extend),
  [`crates/agent-telemetry/`](../../crates/agent-telemetry/) (per-turn span),
  [`crates/agent-grpc/tests/roundtrip.rs`](../../crates/agent-grpc/tests/roundtrip.rs) (roundtrip pattern),
  [`crates/agent-testkit/src/lib.rs`](../../crates/agent-testkit/src/lib.rs) (`tempdir`, doubles);
  dependencies: [`02-patch-diff-editing.md`](02-patch-diff-editing.md) (`apply_patch`), [`19-session-checkpoint.md`](19-session-checkpoint.md) (the per-turn checkpoint to pair with).
- **codex (anchor):** `codex-rs/core/src/turn_diff_tracker.rs` (`TurnDiffTracker`: `track_delta`/`get_unified_diff`, `baseline_by_path`/`current_by_path`, `apply_add`/`apply_update`/`apply_delete`, `git_blob_oid`, `DIFF_TIMEOUT`, *"without rereading the workspace filesystem"*),
  `codex-rs/core/src/session/turn.rs` (~L2737 emits `TurnDiffEvent { unified_diff }` at turn end),
  `codex-rs/chatgpt/src/apply_command.rs` (`ApplyCommand` / `run_apply_command` → `apply_git_patch`),
  `codex-rs/cli/src/main.rs` (~L176 `Apply(ApplyCommand)`, `#[clap(visible_alias = "a")]`),
  `codex-rs/git-utils/src/apply.rs` (`ApplyGitRequest`, `apply_git_patch`),
  `codex-rs/tui/src/slash_command.rs` (~L100 `SlashCommand::Diff` — *"show git diff (including untracked files)"*);
  tests: `codex-rs/core/src/turn_diff_tracker_tests.rs` (`accumulates_add_then_update_as_single_add`, `accumulates_delete`, `accumulates_move_and_update`, `pure_rename_yields_no_diff`, `delete_then_readd_same_path_becomes_update`, `add_over_existing_file_becomes_update`, `repeated_updates_only_rerender_the_touched_path`, `reuses_rendered_diffs_for_unchanged_paths`, `invalidated_tracker_suppresses_existing_diff`, `large_rewrite_returns_promptly_and_preserves_exact_content`).
- **opencode:** `packages/core/src/snapshot.ts` (`Snapshot.Service`: `capture` two trees into a `.snapshot` shadow git repo, `diff(from,to) → File.Diff[]`) — snapshot-pair diff, no per-turn accumulation, no `apply`; test `packages/core/test/git.test.ts` (`git.tree.diff`, ~L148).
- **hermes:** `agent/display.py` (`capture_local_edit_snapshot` per edit-call, `_diff_from_snapshot` / `extract_edit_diff` via `difflib.unified_diff`) — per-edit-call display diff, no turn accumulation, no `apply`; test `tests/agent/test_display.py`.
- **pi:** — (no session/turn diff or change-set `apply`; only `packages/coding-agent/src/core/tools/edit-diff.ts`, a per-edit renderer over the `diff` lib — no whole-turn accumulation, no `git apply`).
