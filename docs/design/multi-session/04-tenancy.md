# 04 — Keying stateful seams by session (and user)

Most seams are functional — one instance serves everyone. This document covers the
ones that hold state, and how each keys it by the wire identity so Alice's session
cannot read or clobber Bob's.

**Tenancy model (locked):** *per-user, cross-session within a user.* A user's
memory/dimensions accumulate across their own sessions but are isolated from other
users. `SessionKey = (user, session)`; the on-disk namespace is
`<root>/<user_seg>/<session_seg>/…`, both segments `safe_segment`-validated and joined
via `confine` ([`01-identity.md`](01-identity.md)).

## The general rule

> **Path-segment the root** where state is file-backed — it inherits filesystem
> isolation *and* survives a restart. Use an in-memory `HashMap<SessionKey, _>` only
> for **live handles** that cannot be a path (a PTY fd). Allocate lazily on first use
> and **idle-GC** ([`05-lifecycle.md`](05-lifecycle.md)), so a lost `Close` never
> leaks. `SessionStore` already proves the path-segment model.

## Per seam

### MemoryStore / EpisodicStore / SemanticStore — the actual leak

Today `.agent/episodic.jsonl` + `.agent/memory` are **global**, and `session_id` is
merely a *column* on `MemoryEvent` — so two sessions interleave writes and, worse,
*recall* returns another tenant's memories. The `session_id` column is not an
isolation mechanism.

**Fix:** root by **user** (cross-session within a user):
`<root>/<user_seg>/episodic.jsonl` and `<root>/<user_seg>/memory/`. The `session_id`
column stays (cheap intra-user audit / filtering) but the **path is the boundary**.
The synchronous `registry.rs` factories that bake a fixed path today become
identity-aware: the server holds a `HashMap<UserId, Arc<FileEpisodic>>` (and semantic)
opening the right rooted store lazily, idle-GC'd. `FileEpisodic`/`FileSemantic` accept
a per-user root instead of a process-global one.

### DimensionStore

Today global `<semantic_dir>/dimensions/*.md`. Dimensions are *per learning-context*,
which — per the tenancy model — is the **user**: a coding user's "testing" dimension
is cross-session knowledge. Root at `<root>/<user_seg>/dimensions/`. Same lazy handle
map as memory, keyed by `UserId`.

### SessionStore — already keyed, but a read oracle to close

`SessionStore` already takes `session: &str` over a shared content-addressed object
store. Two gaps:
- The server must ensure the `session` argument is confined under the caller's
  `<user_seg>/` namespace.
- `restore(id)`/`diff(id)` take a bare content-addressed `CheckpointId` — **no session
  scope** — so a naive server is a **cross-user read oracle** (guess/enumerate an id,
  read another user's checkpoint). Content-addressed ids are capability-like; either
  that is the deliberate model *or* `restore`/`diff` must verify the checkpoint is
  reachable from a head the caller's user owns. This design requires the **ownership
  check**. It is a trait/impl change, **not** a wire change.

### TaskTracker (todos)

Small mutable list, soft failure. `HashMap<SessionKey, TodoList>` on the server +
idle-GC (or a per-session file for restart survival). Low stakes.

### Pty — the live-handle case

State is a live OS process/fd, not a path, so it **must** be an in-memory
`HashMap<SessionKey, Vec<PtyHandle>>`. Handle ids are **namespaced by `SessionKey`**
server-side, so a spoofed handle id from another session resolves to "not found," not
another user's shell. Reaping on `Close`/idle is mandatory — leaked fds are real.
This is where explicit lifecycle ([`05-lifecycle.md`](05-lifecycle.md)) matters most.

### Scheduler — storage, not execution

Constraint from [`grpc.md`](../../grpc.md#serving-a-seam-is-not-always-the-same-as-using-one-remotely):
`--serve-scheduler` is **management-only** — driving a job needs the off-trait
executor closure (`tick_with`), which is per-process. So the wire surface is CRUD.
Namespace the job store by **user** (jobs likely outlive a session). State this
explicitly: `--serve-scheduler` gains multi-tenant job *storage*, **not** multi-tenant
job *execution*. Cross-user execution isolation is a local concern of whoever hosts
the driver.

### Summary

| Seam | Mechanism | Natural key | Failure (preserve) |
|---|---|---|---|
| Episodic / Semantic | path-segment root; server handle map | `user` | soft-ish (memory) |
| DimensionStore | path-segment root | `user` | — |
| SessionStore | already keyed + ownership check on `restore`/`diff` | `(user, session)` | hard |
| TaskTracker | in-mem map + idle-GC | `(user, session)` | soft |
| Pty | in-mem map of live handles + reap | `(user, session)` | — |
| Scheduler (mgmt) | namespaced job store | `user` | — |

## Stateless seams that secretly hold a working dir

`ToolContext { cwd }`, `SearchBackend`, and `RepoBackend` are functional *except* that
they are implicitly scoped to one working dir / repo / confinement root. Serving many
sessions from one worker means the root must be per-session, not per-process.

**Hybrid model (recommended):**
- Register the **confinement root** per session at `SessionRegistry.Open`
  ([`05-lifecycle.md`](05-lifecycle.md)) — the trusted boundary, validated once via
  `confine`/`safe_segment`.
- Let a **relative** cwd/sub-path travel **per call** (an additive field on the
  tool/search/repo request — the typed exception from [`01-identity.md`](01-identity.md)),
  always re-`confine`d server-side against the registered root. This supports the
  worktrees/subdirs this repo already uses, and exactly matches the existing two-layer
  `confine(root, path)` design — you are just lifting the "fixed boundary" from
  process-global to per-`SessionKey`.

A client that sends `cwd = /etc` is confined to the user root, not honored.
Search/Repo servers become `HashMap<SessionKey, Arc<Backend>>` opened against the
registered root — the same lazy handle-map pattern as memory.

## What lands where

| File | Change |
|---|---|
| `crates/agent-memory/src/{file.rs,dimensions.rs}` | per-user rooted `FileEpisodic`/`FileSemantic`/`FileDimensions`. |
| `crates/agent-runtime/src/registry.rs` | identity-aware lazy handle-map factories for memory/dimensions (and search/repo). |
| `crates/agent-core/src/lib.rs` | `SessionStore` `restore`/`diff` ownership semantics; note the Scheduler storage-vs-execution boundary. |
| `crates/agent-session/src/*` | ownership check implementation. |
| `crates/agent-proto/proto/agent/v1/{tool,search,repo}.proto` | additive per-call confined cwd/root. |
| Pty/TaskTracker server impls | `HashMap<SessionKey, _>` + idle-GC hooks. |

## Tests

- `adversarial_` (**mandatory**) — Bob cannot `recall` Alice's memory; Bob cannot
  `restore`/`diff` a checkpoint id he doesn't own; a spoofed Pty handle id →
  `NOT_FOUND`; `cwd = ../../etc` on a tool call → confined/rejected.
- `positive_` — the same user's two sessions *do* share memory/dimensions
  (cross-session learning).
- `boundary_` — idle-GC frees a user's handle after the window; a reaped Pty is gone.
