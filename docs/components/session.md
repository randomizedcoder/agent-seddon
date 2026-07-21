# session history — the `SessionStore` seam

Turns flat save/resume into a git-style, content-addressed history: immutable
checkpoints of the conversation, a branch tree, `undo`/`rollback` to a prior turn,
and `fork` into an independent line — so the agent can recover from a bad path
cheaply and replay a prior state reproducibly. See parity spec
[`19-session-checkpoint.md`](../parity/19-session-checkpoint.md).

**Differentiator:** the peers each own *part* of this (pi the in-file branch tree +
fork, opencode the content-addressed revert-to-turn, hermes the shared store + GC);
none expose it as a swappable, metered, span-traced seam. agent-seddon does.

- **Trait:** `agent_core::SessionStore` ([`agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs)) —
  `checkpoint(session, ws, label) -> CheckpointId`, `list(session)` (the branch
  tree), `restore(id)`, `branch(session, from, name)`, `undo(session, n)`,
  `fork(session)`, `diff(a, b)`, `prune(session)`. Types: `CheckpointMeta` `{ id,
  parent, branch, turn, label, created_ms }`, `CheckpointDiff`.
- **Impl crate:** [`agent-session`](../../crates/agent-session). **Shipped backend:**
  `file` (`session-file`) — dependency-free. Each checkpoint is an **immutable** JSON
  object at `objects/<id>.json`; the id is a hex hash of `(messages + parent +
  label)`, so **immutability is structural** and identical content **dedups** across
  turns and branches. Each session's branch heads live in `sessions/<id>.json`. The
  object store is shared across sessions (dedup); heads diverge per session/branch.
- **Content-addressing:** a checkpoint id is independent of wall-clock time, so
  re-checkpointing identical content under the same parent yields the **same id**
  (write is idempotent, preserving the original `created_ms`); any change yields a
  new id. `restore(id)` is a pure read of an immutable object — never a mutation.
- **Branch tree, not a line:** checkpoints form a DAG via `parent`. `branch` adds a
  head off an older checkpoint (non-destructive — the source line stays restorable);
  `undo(n)` moves the current head back `n` turns (a pointer move — the skipped
  checkpoints remain restorable **by id**, since restore is reachability-independent);
  `fork` copies a session's heads into a new session (independent heads, shared
  immutable objects — writes to the child never touch the parent).
- **GC:** `prune` collects objects **unreachable from any live head of any session**
  (git-style global reachability over the shared store) and **never** collects a
  reachable checkpoint. Dedup keeps the store small.
- **Wiring:** reached via `Agent::checkpoint` / `restore_checkpoint` /
  `list_checkpoints`; the builder builds the config-selected store, meters it, and
  attaches it. **Config:** `[session] backend = "file"`, `dir` (empty ⇒
  `<working_dir>/.agent-seddon/session`).
- **Observability:** `agent_session_ops_total{op}` +
  `agent_session_gc_reclaimed_total` metrics (via `MeteredSession`) + a
  `session.<op>` span (`session`/`id`/… attrs) per checkpoint/restore/branch/undo/
  fork/prune, so the branch tree is time-travel-inspectable in the trace.

## Tests, bench, leak

- **Seam** (`agent-session`, over `tempdir`): checkpoint→restore roundtrip,
  content-addressing (dedup + distinguish), branch diverges non-destructively, undo
  moves the head while the skipped checkpoint stays restorable, fork independence,
  restore-unknown error, diff turn-delta, and prune (keeps reachable, collects the
  orphan).
- **Bench:** `benches/checkpoint.rs` — serialize + content-hash a 100-message
  working set (the per-turn CPU cost; deterministic Ir ceiling). The disk-write +
  gRPC paths are I/O-bound and not benched.
- **Leak:** `tests/leak.rs` runs repeated checkpoints under dhat.

## Deferred (staged like the prior seams)

- The `SessionService` gRPC (`agent --serve-session`, reflection) so a `= "grpc"`
  client can time-travel a remote session.
- A **`RepoBackend`-backed** impl (dedup via real git objects, reusing the git
  seam's object DB) as a second, capability-equivalent backend.
- **Loop auto-checkpoint** (a checkpoint each turn) + coupling `undo`/`rollback` to
  a `RepoBackend` code checkpoint so the filesystem rolls back too; a retention
  policy (keep-N-per-branch / max-age / size cap) on `prune`; and importing the
  existing flat `.agent/sessions/<id>.jsonl` as a single linear branch.
