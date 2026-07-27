# 01 — Identity on the wire

The foundational primitive: a `(user_id, session_id)` pair that every seam — local
or remote — can see, validated once and namespaced everywhere. This increment adds
**no wire proto change** (identity rides metadata) and is a prerequisite for all the
others.

## The types (`agent-core`)

Three newtypes plus a shared validator, in
[`crates/agent-core/src/lib.rs`](../../../crates/agent-core/src/lib.rs) beside
`confine`:

```rust
pub struct UserId(String);
pub struct SessionId(String);
pub struct SessionKey { pub user: UserId, pub session: SessionId }

/// The ambient identity carried per request/turn.
pub struct SessionIdentity { pub user: UserId, pub session: SessionId }
```

`safe_segment` — currently duplicated in `agent-git` (`safe ref/segment`) and
`agent-review` — is **promoted into `agent-core`** next to `confine`, because it
becomes a security-critical validator every seam shares. It rejects `..`, path
separators, leading `-`, and non-`[A-Za-z0-9._-]`. Both `user_id` and `session_id`
pass `safe_segment` **before** being used as a `HashMap` key or a path component, so
a crafted `user_id = "../bob"` is rejected, not honored ([`07-security.md`](07-security.md)).

`SessionKey::path_under(root) -> Result<PathBuf>` returns
`confine(root, <user_seg>)?.join(<session_seg>)` — the canonical on-disk namespace,
symlink-escape-proof via `confine`.

## The wire mechanism (hybrid)

Identity is a **cross-cutting transport concern with the exact lifecycle and
choke-points as trace context**, so it reuses that path rather than editing 26 proto
messages.

### Ambient identity → gRPC metadata

Two ASCII metadata keys, defined as `const`s (with docs) in a new
[`crates/agent-proto/src/identity.rs`](../../../crates/agent-proto/src/identity.rs)
module mirroring [`trace.rs`](../../../crates/agent-proto/src/trace.rs) — **not** in
any `.proto`, so `buf breaking` never sees them:

```rust
pub const SESSION_ID_KEY: &str = "x-agent-session-id";
pub const USER_ID_KEY:    &str = "x-agent-user-id";
```

- **Client** — [`agent-grpc/src/client/mod.rs::outbound()`](../../../crates/agent-grpc/src/client/mod.rs)
  already injects W3C trace context into `req.metadata_mut()`. Add three lines that
  read the ambient `SessionIdentity` (below) and insert the two keys. **All 25 client
  seams inherit it for free** — no per-seam edit.
- **Server** — [`agent-grpc/src/server/mod.rs::span()`](../../../crates/agent-grpc/src/server/mod.rs)
  already builds the `grpc.server` span from metadata. Extract the two keys,
  `safe_segment`-validate them, **fail closed** if absent/malformed on a *stateful*
  seam RPC (see below), stamp `session.id`/`user.id` as span attributes, and run the
  handler inside the identity task-local.

buf impact: **none.** No baseline bump.

### The ambient carrier — `tokio::task_local`, not OTel baggage

`outbound()` cannot take identity as a parameter without touching every seam, so it
reads from ambient state. Use a dedicated

```rust
tokio::task_local! { pub static AGENT_IDENTITY: SessionIdentity; }
```

**Not OTel baggage.** Baggage is inert until a propagator is installed by telemetry
init ([`trace.rs`](../../../crates/agent-proto/src/trace.rs) is explicit about this).
Binding a *security boundary* to whether OTLP happens to be configured is a fail-open
trap: with telemetry off, a baggage-carried `user_id` would silently vanish and every
request would become "identity absent." The task-local flows regardless of telemetry.

It is set in two places:
- **Agent root** ([`agent-cli/src/main.rs`](../../../crates/agent-cli/src/main.rs)) —
  where `session_id` is minted today. Run the agent inside
  `AGENT_IDENTITY.scope(identity, async { … })`. This is the one place identity
  *originates* trustworthy (the local process's own session).
- **Server side** (in/next to `span()`) — extract from metadata, build a
  `SessionIdentity`, run the handler inside `AGENT_IDENTITY.scope(…)`. This makes a
  **server-as-client** (e.g. `--serve-all`, where `ContextService` calls the provider)
  forward the caller's identity transparently via `outbound()` — the multi-hop story,
  mirroring how the current span becomes the parent for onward trace context.

### The typed exceptions

Metadata is the default everywhere, but three surfaces model the session *typedly*,
because there the session is part of the operation, not ambient context:

- The new **`SessionRegistry`** service ([`05-lifecycle.md`](05-lifecycle.md)) — its
  whole job is to mint/associate a session.
- **`SessionStore`** — already takes `session: &str` as a typed method argument; keep
  it (and add an ownership check, [`04-tenancy.md`](04-tenancy.md)).
- A per-call **confined cwd/root** on tool/search/repo requests
  ([`04-tenancy.md`](04-tenancy.md)) — the working *position*, which is operation
  semantics, always re-`confine`d server-side.

We do **not** retrofit typed identity fields onto the other 24 seams.

## Fail-closed rules (at extraction)

| Situation | Behaviour |
|---|---|
| Identity absent on a **stateful** seam RPC | Reject: `unauthenticated` / `invalid_argument`. **Never** invent a default user (a default user is shared-tenancy fail-*open*). |
| Identity malformed (`safe_segment` fails) | Reject: `invalid_argument`. |
| Identity absent on a purely **stateless/functional** seam (Tokenizer, Embedder, Web, Policy-eval) | Allowed — no per-session state to mis-route (but see the cwd exception in [`04-tenancy.md`](04-tenancy.md)). |

Preserve the per-seam failure severity already documented in
[`grpc.md`](../../grpc.md#streaming--errors): Policy/Session/Search/Repo already fail
hard, so rejecting is consistent; Scanner/Reference fail open but are stateless, so
identity is moot for them.

## What lands where

| File | Change |
|---|---|
| `crates/agent-core/src/lib.rs` | `UserId`/`SessionId`/`SessionKey`/`SessionIdentity`; promote `safe_segment` beside `confine`; `SessionKey::path_under`. |
| `crates/agent-proto/src/identity.rs` (new) | metadata key consts + doc contract; optional injector/extractor helpers. |
| `crates/agent-grpc/src/client/mod.rs` | inject identity in `outbound()` from `AGENT_IDENTITY`. |
| `crates/agent-grpc/src/server/mod.rs` | extract + `safe_segment`-validate + fail-closed + span attrs + task-local scope in/near `span()`. |
| `crates/agent-cli/src/main.rs` | mint identity into `AGENT_IDENTITY.scope` at the agent root. |
| `docs/grpc.md` | document the metadata-key contract next to the trace-context section. |

## Tests (table-driven, per repo convention)

- `positive_` — identity round-trips client→server; server-as-client forwards it.
- `adversarial_` (**mandatory**) — `user_id`/`session_id` containing `..`, `/`,
  leading `-`, control chars, over-length → rejected by `safe_segment`; absent
  identity on a stateful RPC → `unauthenticated`; a spoofed but well-formed identity
  still cannot escape its namespace (via `SessionKey::path_under` + `confine`).
- `boundary_` — empty identity on a stateless seam is allowed; exactly-at-limit
  segment length.
- Confirm `buf breaking` passes untouched (no proto change).
