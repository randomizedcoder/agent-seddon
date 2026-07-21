# Parity spec 19 — session checkpoint / branch / undo

Per-feature parity spec for a new **`SessionStore` seam** that turns today's flat
save/resume into a git-style, content-addressed history: immutable checkpoints of
conversation state, a branch tree, `undo`/`rollback` to a prior turn, and forking a
session into an independent line. Tracks what agent-seddon ships today, what the
peers assert, and the concrete behaviour + tests needed to be the most complete of
the four.

> **Status: spec (design of record).** Introduce a **`SessionStore` seam**
> (`agent-core` trait) with a git-style object model — a checkpoint is a
> **commit-like, immutable, content-addressed** object over the working set, linked
> to its parent to form a **branch tree**; `undo`/`branch`/`fork` are pointer moves,
> never rewrites. Served as a new `session.proto` gRPC service (`--serve-session`,
> reflection-introspectable) so a `= "grpc"` client can time-travel a remote
> session. **Differentiator:** reuse agent-seddon's existing
> [`RepoBackend`](../../crates/agent-core/src/lib.rs) object model — the same
> immutable-revision reads and `checkpoint()`-to-a-private-ref machinery that backs
> the git seam — so a conversation checkpoint is a real content-addressed object
> (dedup across turns/branches for free), and each checkpoint/restore is an OTel
> span, making the branch tree replayable and inspectable. No peer offers a seam
> that is *at once* content-addressed, branchable, gRPC-served, metered, and
> span-traced.

## Feature & why it matters

Coding agents go down bad paths: a wrong refactor plan, a tool loop that corrupts
state, a compaction that dropped the thread. Recovering by re-prompting from scratch
is expensive (re-reads, re-searches, lost context) and non-reproducible. The escape
hatch every serious agent grows is **cheap history**: checkpoint the conversation
each turn, `undo` back to the last good turn, `branch` to explore an alternative
without losing the current line, and `fork` a session into an independent copy. The
same machinery gives **reproducible replay** — restore an exact prior state and re-run
deterministically for debugging or evaluation.

The failure modes are all about *identity and immutability*: an `undo` must not
mutate the branch it came from (the old line stays restorable); a `fork` must be
independent (writes to the child never leak into the parent); a `restore` of an
unknown checkpoint must error, not silently produce an empty session; and retention
/ GC must never collect a checkpoint still reachable from a live branch head. A store
that gets these wrong is worse than no history at all — it hands the model a
corrupted past. Content-addressing (a checkpoint id = hash of its content + parent)
makes immutability structural rather than a convention, and makes dedup across turns
and branches automatic.

## agent-seddon today

- **Impl (flat):** [`crates/agent-runtime/src/session_store.rs`](../../crates/agent-runtime/src/session_store.rs)
  — free functions `save` / `load` / `list` / `most_recent`. Each session is a single
  `.agent/sessions/<id>.jsonl` file (one `Message` per line) **overwritten** after
  every turn; `list` powers a resume picker (turns + preview), `most_recent` powers
  `--continue`. This is genuinely flat: there is **no history within a session** — the
  previous transcript is clobbered on save, so there is no checkpoint, no branch, no
  `undo`/`rollback`, and no `fork`. It is also not a seam: it is a module of
  functions, not a trait behind the plugin registry, so it can't be swapped or
  gRPC-served.
- **Object model to reuse:** [`RepoBackend`](../../crates/agent-core/src/lib.rs)
  (trait at ~L834) already has exactly the primitives a git-style session history
  needs: immutable, revision-addressed **object reads** (`resolve`, `read_file`,
  `list_tree`, `diff`, `log`), a `checkpoint(worktree_id, name) -> Checkpoint`
  that commits state to a **private agent ref** ([`Checkpoint`](../../crates/agent-core/src/lib.rs)
  at ~L810 = `{ name, oid, ref_name }`), and worktree lifecycle. A `SessionStore`
  built on this treats a conversation checkpoint as a commit-like object: content →
  blob/tree, parent link → the commit graph, branch head → a ref. The
  [`FixtureRepo`](../../crates/agent-testkit/src/lib.rs) double already models this
  object DB for tests.
- **Gaps:** everything beyond flat save/resume. No `SessionStore` trait, no
  content-addressed checkpoint, no branch tree, no `undo`-to-turn, no `fork`, no
  `diff` between checkpoints, no retention/GC, no proto/gRPC service, no metrics or
  spans for session mutation.

## Peer implementations & their tests

| Peer     | Impl path | Test path | Framework |
| -------- | --------- | --------- | --------- |
| pi       | `pi/packages/coding-agent/src/core/session-manager.ts` (v2 tree: `id`/`parentId` entries), `pi/packages/coding-agent/src/core/agent-session.ts` (fork); format `pi/packages/coding-agent/docs/session-format.md` | `pi/packages/coding-agent/test/session-manager/tree-traversal.test.ts`, `.../test/agent-session-branching.test.ts` | vitest |
| opencode | `opencode/packages/core/src/session/revert.ts` (revert-to-message + snapshot restore), `opencode/packages/opencode/src/snapshot/index.ts` (content-addressed shadow-git `track`/`patch`), `opencode/packages/core/src/session/info.ts` (`parentID` lineage) | `opencode/packages/core/test/session-runner.test.ts` (context-snapshot decode), `.../test/integration.test.ts` | bun:test + Effect |
| hermes   | `hermes-agent/tools/checkpoint_manager.py` (`CheckpointManager`: shared shadow-git store, `list_checkpoints`/`restore`/`prune`), `hermes-agent/hermes_state.py` (`parent_session_id` chains, compression-driven session split) | `hermes-agent/tests/tools/test_checkpoint_manager.py`, `hermes-agent/tests/integration/test_checkpoint_resumption.py` | pytest |

**pi** models the session itself as a **tree** (session-format v2): every entry has
an `id` and a `parentId`, so branching is "append a new child off an older parent" —
no new file, all history preserved in one JSONL. `tree-traversal.test.ts` asserts:

- `appendMessage` builds a correct `parentId` chain (`entries[n].parentId === entries[n-1].id`; the root's `parentId` is `null`).
- Non-message entries (`thinking_level_change`, `model_change`, `compaction`) integrate into the tree with the right `parentId`, and the next message chains off *them* (the tree carries side-channel events, not just messages).
- Leaf tracking: `getLeafEntry()` returns the current tip (the branch head).

`agent-session-branching.test.ts` asserts **forking**: fork from a single message,
fork from the middle (`getUserMessagesForForking` returns the fork-able ancestors),
and **in-memory forking** in `--no-session` mode (a fork that never touches disk).
pi's `git-checkpoint.ts` extension pairs the conversation fork with a *code* checkpoint
(`git stash create`) so `/fork` can also restore the filesystem — the same
conversation-history-plus-repo-object idea this spec makes first-class.

**opencode** stores lineage (`parentID` on `session/info.ts`) and implements
**revert-to-message** (`session/revert.ts`): pick a message boundary, and everything
after it — assistant turns **and the file mutations they made** — is rolled back, the
file state restored from a **content-addressed snapshot** (`snapshot/index.ts` uses a
shadow git repo keyed by content `Hash`; `track` returns a hash, `patch(hash)` yields
the diff). A `MessageNotFoundError` is raised when the boundary message id is unknown.
`revert.stage`/`clear`/`commit` (`session.ts`) make the revert a two-phase move
(stage → commit or clear), and `session-runner.test.ts` pins the failure path
(`ContextSnapshotDecodeError` when a stored snapshot can't be decoded).

**hermes** takes checkpoints in a **single shared content-addressed shadow-git store**
(`~/.hermes/checkpoints/store/`, one object DB across all projects so git dedups
across turns *and* projects) via `CheckpointManager` — `list_checkpoints`, `restore`
(whole tree or a single `file_path`), and `prune` (orphan/stale sweep + GC + size
cap). `/rollback <N>` restores to checkpoint N **and undoes the last chat turn** (the
conversation-and-filesystem coupling again). Separately, `hermes_state.py` chains
sessions with `parent_session_id` (with a recursive `_BRANCH_CHILD_SQL` walk), and a
**compression-driven session split** starts a child session that points back at its
parent — a lineage tree at the *session* granularity. `test_checkpoint_manager.py`
covers the store/restore/prune unit surface; `test_checkpoint_resumption.py` covers
interruption/crash resumption end-to-end.

Taken together, the peers each own *part* of this: pi the in-file branch tree + fork,
opencode the content-addressed revert-to-turn, hermes the shared content-addressed
store + GC + parent chains. **None expose it as a swappable, gRPC-served,
span-traced seam** — that is the gap this spec closes.

## Completeness gaps

Behaviour agent-seddon must add to be the most complete (spec only — do **not**
implement here):

- **`SessionStore` seam.** New `agent-core` async trait, roughly:
  `checkpoint(session, working_set, label) -> CheckpointId` (immutable, returns a
  content-addressed id), `list(session) -> Vec<CheckpointMeta>` (the branch tree,
  each meta = `{ id, parent, branch, turn, label, created }`), `restore(id) ->
  WorkingSet` (rehydrate exact prior state), `branch(from_id, name) -> BranchId`
  (a new head off an older checkpoint — divergent, non-destructive to the source
  line), `fork(session) -> SessionId` (independent copy; writes to the child never
  touch the parent), `diff(a, b) -> CheckpointDiff` (message/turn delta between two
  checkpoints, reusing the `RepoBackend` diff shape).
- **Content-addressing.** A checkpoint id is the hash of `(serialized working set +
  parent id + metadata)`; identical content under the same parent yields the **same
  id** (dedup); any change yields a new id. Immutability is structural — restore is a
  pure read of an immutable object, never a mutation.
- **Branch tree, not a line.** Checkpoints form a DAG via `parent`. `list` returns
  the tree (parent links intact); a `branch`/`undo`/`fork` adds a node/head without
  rewriting existing nodes. The current tip per branch is a movable **head**
  (pi's `getLeafEntry`).
- **`undo` / `rollback` to a prior turn.** `undo(n)` moves the branch head back `n`
  turns to an existing checkpoint (a pointer move — the skipped checkpoints remain
  reachable and restorable). `rollback` = `restore` the working set to that head.
  Optionally couple to a `RepoBackend` code checkpoint so filesystem state rolls back
  too (opencode/hermes/pi all pair the two).
- **`restore`-unknown is an error.** An unknown/typo'd checkpoint id ⇒ a distinct
  `CheckpointNotFound` (mirrors opencode's `MessageNotFoundError`), never a silent
  empty session.
- **Retention / GC.** A `prune` that GCs checkpoints **unreachable** from any live
  branch head, honoring a retention policy (keep-N-per-branch / max-age / size cap,
  like hermes' sweep) — and **never** collects a reachable checkpoint. Because the
  store is content-addressed and reuses the `RepoBackend` object DB, GC is git-style
  reachability, and dedup keeps the store small across turns/branches.
- **Migration from flat.** The existing `.agent/sessions/<id>.jsonl` flat sessions
  import as a **single linear branch** (each saved turn → a checkpoint), so
  `--continue`/resume keep working; the new store is a strict superset.
- **Seam, served, observed.** Wired via the plugin registry (config selects the impl:
  `flat` = today's behaviour re-expressed, `repo` = the git-backed default); a
  `session.proto` gRPC service (`--serve-session`, reflection); Prometheus metrics
  (checkpoints created, restores, branch/fork counts, GC reclaimed) and an OTel span
  per `checkpoint`/`restore`/`branch`/`fork` carrying `session`, `checkpoint_id`,
  `parent`, `turn` attributes (the #44 span-attribute pattern) so the branch tree is
  time-travel-inspectable in the trace.

**Harness obligations** (the implementing PR must satisfy all — the #21–45 checklist):

- **Seam + registry:** `SessionStore` trait in `agent-core`; impls (`flat`, `repo`)
  in a sibling crate behind a cargo feature; one factory line in
  [`register_builtins`](../../crates/agent-runtime/src/registry.rs), config-selected
  (mirrors the `RepoBackend` factory block). Doc in `docs/components/`.
- **Proto + gRPC + reflection:** add `crates/agent-proto/proto/agent/v1/session.proto`
  (`Checkpoint`/`List`/`Restore`/`Branch`/`Fork`/`Diff` RPCs) + `build.rs` entry +
  server/client in `agent-grpc` + `--serve-session` + reflection; commit the
  `buf.image.binpb` bump via `nix run .#buf-image`; add the endpoint constant to
  `nix/constants.nix` → `nix run .#gen-constants`; extend the gRPC roundtrip test.
- **Metrics + OTel:** metric families in `agent-metrics`, a metered decorator in
  `agent-runtime/src/metered.rs`, and a `session.<op>` span per
  checkpoint/restore/branch/fork with attributes.
- **Bench:** an iai-callgrind bench for the genuine CPU hot path — **checkpoint
  serialization + content-hash of a conversation working set** (deterministic; the
  per-turn cost) — with an Ir ceiling in `nix/checks/bench.nix`. gRPC/disk paths
  document the skip.
- **Leak:** a dhat `tests/leak.rs` (`dhat-heap` feature) over the checkpoint path
  (serialize → hash → write object → update head), asserting the hot path frees what
  it allocates and stays under budget.

## Table-driven test plan

New crate/module for the seam (e.g. `crates/agent-session/src/lib.rs` for the trait
tests, next to the `repo`-backed impl). Match the repo/tools style: table-driven
`#[rstest]` `#[case::...]`, `agent_testkit::tempdir()` for the object DB, and the
existing [`FixtureRepo`](../../crates/agent-testkit/src/lib.rs) double as the
`RepoBackend` the `repo`-backed `SessionStore` sits on (add a `RecordingSession`
fixture double if a pure in-memory store is wanted for the flat impl). Build working
sets from small `Message` vecs (reuse `Message::user/assistant/system`, as
`session_store.rs`'s tests do).

Case-prefix key: `positive_` the operation succeeds, `negative_` an error/guard,
`corner_` odd-but-valid, `boundary_` an edge (empty/at-limit). `(port: peer)` names
the peer the case came from; `(new: agent-seddon)` marks cases with no peer origin.

```rust
// crates/agent-session/src/lib.rs — SessionStore seam tests.
// Doubles: agent_testkit::{tempdir, FixtureRepo}; local ws(&[(Role, &str)]) helper.

#[rstest]
// --- checkpoint → restore roundtrip: exact prior state comes back ------------
#[case::positive_checkpoint_restore_roundtrip(
    Plan::new()
        .checkpoint("t1", &[(User, "goal"), (Assistant, "ok")])   // -> id A
        .restore_last(),
    Expect::working_set(&[(User, "goal"), (Assistant, "ok")]))]   // (port: hermes|opencode)
// --- content-addressing: same content+parent ⇒ same id; change ⇒ new id ------
#[case::corner_content_addressed_dedup(
    Plan::new()
        .checkpoint("t1", &[(User, "a")])                          // -> id A
        .checkpoint_same_parent("t1", &[(User, "a")]),             // identical
    Expect::same_id())]                                            // (new: agent-seddon)
#[case::positive_distinct_content_distinct_id(
    Plan::new()
        .checkpoint("t1", &[(User, "a")])
        .checkpoint("t2", &[(User, "b")]),
    Expect::distinct_ids())]                                       // (new: agent-seddon)
// --- branch creates a divergent tree, source line untouched ------------------
#[case::positive_branch_diverges_non_destructive(
    Plan::new()
        .checkpoint("t1", &[(User, "a")])                          // -> A (main)
        .checkpoint("t2", &[(User, "a"), (Assistant, "b")])        // -> B (main head)
        .branch_from("A", "alt")                                   // new head off A
        .checkpoint_on("alt", &[(User, "a"), (Assistant, "c")]),   // -> C (alt head)
    Expect::tree(&[("main", "B"), ("alt", "C")])                   // two heads
        .and_reachable("B"))]                                      // main still restorable // (port: pi)
// --- undo N turns: head moves back, skipped checkpoints stay reachable -------
#[case::positive_undo_two_turns(
    Plan::new()
        .checkpoint("t1", &[(User, "a")])                          // -> A
        .checkpoint("t2", &[(User, "a"), (Assistant, "b")])        // -> B
        .checkpoint("t3", &[(User, "a"), (Assistant, "b"), (User, "c")]) // -> C
        .undo(2),                                                  // head -> A
    Expect::head("A").and_reachable("C"))]                         // (port: hermes|opencode)
// --- fork independence: child writes never touch the parent ------------------
#[case::positive_fork_is_independent(
    Plan::new()
        .checkpoint("t1", &[(User, "a")])                          // parent session
        .fork()                                                    // -> child session
        .checkpoint_on_fork("t2", &[(User, "a"), (Assistant, "z")]),
    Expect::parent_head_unchanged("A")                             // parent untouched
        .and_fork_head_has(&[(User, "a"), (Assistant, "z")]))]     // (port: pi)
// --- restore unknown id → distinct error ------------------------------------
#[case::negative_restore_unknown(
    Plan::new().restore_id("deadbeef-not-a-checkpoint"),
    Expect::err("not found"))]                                     // (port: opencode MessageNotFoundError)
// --- GC retention: unreachable pruned, reachable kept ------------------------
#[case::boundary_gc_keeps_reachable_prunes_orphan(
    Plan::new()
        .checkpoint("t1", &[(User, "a")])                          // -> A (reachable via main)
        .checkpoint("t2", &[(User, "a"), (Assistant, "b")])        // -> B (main head)
        .branch_from("A", "alt")                                   // alt head = A
        .undo_on("alt", 1)                                         // orphan a checkpoint off alt (unreachable)
        .prune(RetainPolicy::keep_reachable()),
    Expect::present(&["A", "B"]).and_absent_orphans())]            // (port: hermes prune)
// --- migration: flat .jsonl imports as one linear branch --------------------
#[case::corner_import_flat_session_as_linear_branch(
    Plan::from_flat_jsonl(&[(User, "old"), (Assistant, "reply")]),
    Expect::single_branch_len(1))]                                 // (new: agent-seddon)
// --- diff between two checkpoints -------------------------------------------
#[case::positive_diff_two_checkpoints(
    Plan::new()
        .checkpoint("t1", &[(User, "a")])                          // -> A
        .checkpoint("t2", &[(User, "a"), (Assistant, "b")])        // -> B
        .diff("A", "B"),
    Expect::added_turns(1))]                                       // (port: opencode snapshot patch)
#[tokio::test]
async fn session_store_cases(#[case] plan: Plan, #[case] expected: Expect) {
    // Build a repo-backed SessionStore over FixtureRepo in tempdir(); execute the
    // plan of ops; assert on the resulting tree / head / restored working set /
    // error per Expect. `Plan`/`Expect` are a tiny in-test DSL so each case reads
    // as "do these ops, assert this invariant".
}
```

A sibling table pins the **flat-impl parity** (today's behaviour re-expressed through
the seam) so migration is safe: `save`→`checkpoint`, `most_recent`→branch head,
`list` preview/turns unchanged. The existing `preview_cases` / `save_load_round_trip`
tables in [`session_store.rs`](../../crates/agent-runtime/src/session_store.rs) stay
as-is and become the flat backend's regression floor.

## References

- **agent-seddon:** [`crates/agent-runtime/src/session_store.rs`](../../crates/agent-runtime/src/session_store.rs) (flat save/load/list/most_recent — the seam generalizes it), [`crates/agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs) (`RepoBackend` ~L834, `Checkpoint` ~L810, `Oid`/`Revision`/`DiffResult` — the object model to reuse), [`crates/agent-runtime/src/registry.rs`](../../crates/agent-runtime/src/registry.rs) (`register_builtins`, the `RepoBackend` factory block to mirror), [`crates/agent-testkit/src/lib.rs`](../../crates/agent-testkit/src/lib.rs) (`tempdir`, `FixtureRepo`, `RecordingMemory`), [`docs/components/git.md`](../components/git.md).
- **pi:** `pi/packages/coding-agent/src/core/session-manager.ts` (v2 `id`/`parentId` tree, `getLeafEntry`), `pi/packages/coding-agent/src/core/agent-session.ts` (fork), `pi/packages/coding-agent/docs/session-format.md`, `pi/packages/coding-agent/examples/extensions/git-checkpoint.ts`; tests `pi/packages/coding-agent/test/session-manager/tree-traversal.test.ts`, `pi/packages/coding-agent/test/agent-session-branching.test.ts`.
- **opencode:** `opencode/packages/core/src/session/revert.ts` (revert-to-message, `MessageNotFoundError`, `revert.stage`/`clear`/`commit`), `opencode/packages/opencode/src/snapshot/index.ts` (content-addressed shadow-git `track`/`patch`), `opencode/packages/core/src/session/info.ts` (`parentID` lineage); tests `opencode/packages/core/test/session-runner.test.ts` (`ContextSnapshotDecodeError`), `opencode/packages/core/test/integration.test.ts`.
- **hermes:** `hermes-agent/tools/checkpoint_manager.py` (`CheckpointManager`: shared content-addressed shadow-git store, `list_checkpoints`/`restore`/`prune`), `hermes-agent/hermes_state.py` (`parent_session_id` chains, `_BRANCH_CHILD_SQL`, compression-driven session split), `hermes-agent/website/docs/user-guide/checkpoints-and-rollback.md`; tests `hermes-agent/tests/tools/test_checkpoint_manager.py`, `hermes-agent/tests/integration/test_checkpoint_resumption.py`.
