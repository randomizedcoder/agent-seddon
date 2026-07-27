# 07 — Security & trust boundary

This design applies the repo rule — **the model and every provider/server/client
value is untrusted; fail closed** ([`CLAUDE.md`](../../../CLAUDE.md)) — to a new class
of untrusted input: the wire identity.

## The trust boundary, stated plainly

> The wire `user_id`/`session_id` are **attacker-controllable** and MUST NOT be
> trusted as *authentication*. They are trusted only as *routing/namespacing* labels,
> and only as far as the transport's own access control (UDS file perms / loopback)
> already trusts the peer.

A malicious client on the same UDS can present any `user_id`. Therefore **isolation
must hold even against a spoofed identity** — and it does, structurally, not by trust:

1. **Filesystem namespacing is the defense that works today, with no auth.** Every
   rooted-store construction runs both identity segments through `safe_segment` (reject
   `..`, separators, leading `-`, non-`[A-Za-z0-9._-]`) and builds the path via
   `confine(user_root, session_seg)`. Because `confine` canonicalizes and rejects
   symlink escape, even `session_id = "../../etc"` or a symlink attack resolves to an
   *error*, not a traversal. So a spoofer can *claim* to be user X (an auth problem,
   deferred) but cannot *reach outside* the namespace they name (a traversal problem,
   solved). `safe_segment` is promoted into `agent-core` so all seams share one audited
   validator ([`01-identity.md`](01-identity.md)).

2. **Fail closed on absent/malformed identity** for stateful seams
   (`unauthenticated`/`invalid_argument`) — **never** synthesize a default user (that
   is shared-tenancy fail-*open*). See the table in [`01-identity.md`](01-identity.md).

3. **Amplification guard.** A hostile client must not force unbounded per-`SessionKey`
   allocation by spraying session_ids. Bound the live-session map (per-user cap) +
   idle-GC ([`05-lifecycle.md`](05-lifecycle.md)) — mirroring the existing
   `grpc-retry-pushback-ms` hardening against a hostile server.

4. **Cross-user read oracle closed.** `SessionStore.restore`/`diff` take a bare
   content-addressed id; the server verifies the checkpoint is reachable from a head
   the caller's user owns ([`04-tenancy.md`](04-tenancy.md)).

## The `bash` containment gap (must not be hand-waved)

Per-session cwd + `confine` isolates `edit`/`read`/`write`/`search`. It does **not**
contain `bash`, which is the deliberate *unconfined* escape hatch
([`CLAUDE.md`](../../../CLAUDE.md)). In multi-user mode, `bash` under one session can
read another user's files (or the host). This is a real containment gap, not a
plumbing detail. Options for multi-user deployments (increment 08):
- Run `bash` under the `sandbox` seam rooted at the session cwd, or
- disable `bash` per-user by `Policy` in multi-user configs.

The same caveat applies with more force to `--serve-sandbox`/`--serve-pty`/
`--serve-forge`, which host arbitrary execution / authenticated writes and whose
`Policy` gate stays *client-side* ([`grpc.md`](../../grpc.md#--serve-sandbox-and---serve-pty-are-a-different-class-of-grant)).

## Deployment hardening before auth exists

For the exec-capable modes, the recommended hardening *before* a real auth layer is
**one UDS socket per user** with OS file permissions (`0o600` in `0o700`), so the
transport itself enforces the user boundary and `x-agent-user-id` becomes advisory.
This composes with §1: even within one user's socket, session namespacing still
isolates that user's sessions.

## The named auth follow-up (out of scope, prerequisite for real tenancy)

Turning the identity labels into a real tenancy boundary requires an **authentication
interceptor** (a follow-up, not this design):
- Reject unauthenticated calls, and **derive `user_id` from a verified token**,
  *ignoring* any client-supplied `x-agent-user-id`.
- Slots into the same `server::span()` extraction point; once present, the "identity is
  only as trustworthy as the transport" caveat is lifted for the TCP transport too
  (composes with mTLS, the existing [`grpc.md`](../../grpc.md#possible-follow-ups)
  follow-up).

This boundary is documented in [`docs/grpc.md`](../../grpc.md) next to the
metadata-key contract, so nobody mistakes isolation for authentication.

## Adversarial test sweep (mandatory, increment 08)

- Traversal/injection in `user_id`/`session_id` (`..`, `/`, leading `-`, symlink,
  control chars, over-length) → rejected by `safe_segment`/`confine`.
- Absent/malformed identity on stateful RPCs → fail closed.
- Bob cannot read Alice's memory / checkpoint / session events / files.
- A spoofed handle id (Pty) or checkpoint id (SessionStore) → `NOT_FOUND` / ownership
  error.
- Session-id spray beyond the per-user cap → rejected/evicted.
- `bash` containment behaviour matches the configured multi-user policy.
