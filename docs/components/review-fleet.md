# Review fleet — the `FleetRegistry` seam

An unattended code-review fleet is a set of long-lived review sessions, each owning a
repository, watching it for non-draft PRs, and running an isolated review. The
**roster** is the durable list of *who reviews what* — the source of truth the fleet
server reconciles its live sessions from (review-fleet C2,
[`docs/design/review-fleet/03-fleet-core.md`](../design/review-fleet/03-fleet-core.md)).

- **Trait:** `agent_core::FleetRegistry` ([`agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs))
- **Row type:** `agent_core::FleetSession`
- **Impl crate:** [`agent-review-fleet`](../../crates/agent-review-fleet)
- **Shipped backends:** `MemoryFleet` (base), `FileFleet` (JSON bundle),
  `SqliteFleet` (feature `fleet-sqlite`)
- **Config:** `[review_fleet] store`, `file`, `path`, `root`, `max_total`,
  `max_per_user`

> This is the roster only (increment 3a). The gRPC control plane
> (`ReviewFleetService`) and the `agent --serve-fleet` process that admits + drives
> sessions from the roster arrive in increments 3b and 3c.

## The trait

```rust
#[async_trait]
pub trait FleetRegistry: Send + Sync {
    async fn list(&self) -> Result<Vec<FleetSession>>;          // every row, enabled or not
    async fn get(&self, id: &str) -> Result<FleetSession>;
    async fn put(&self, session: FleetSession) -> Result<FleetSession>; // upsert, returns sanitized
    async fn delete(&self, id: &str) -> Result<bool>;           // unknown id ⇒ Ok(false)
    async fn set_enabled(&self, id: &str, enabled: bool) -> Result<FleetSession>;
}
```

The CRUD discipline mirrors [`ProviderRegistry`](router.md) exactly: every
argument is untrusted (an `id` may become a storage-path segment), so stores validate
fail-closed and clamp numbers on ingest. `get`/`set_enabled` of an unknown id is an
`Err` whose message starts with `not found` (the wire layer maps it to gRPC
`NotFound`); `delete` of an unknown id is `Ok(false)`, not an error.

## The row (`FleetSession`)

One roster row carries a review session's identity, the repo it watches, how it reaches
its forge, and its triggers — the full C2 shape, so later increments only *read* the
extra fields:

| Field | Meaning |
|---|---|
| `id`, `user`, `repo` | Path-safe segments (`safe_segment`); the workspace is `root/<user>/<id>` |
| `backend`, `base_url` | Forge kind (`github`/`gitlab`/`""`) + API base URL |
| `token_ref` | `env:NAME` / `file:/path` — **never** a raw token (see below) |
| `skill` | Review skill/prompt selector |
| `slack_trigger_channel`, `slack_progress_channel` | Slack triggers / progress (inc 4/7) |
| `poll_secs` | Forge poll interval, clamped to `[MIN_FLEET_POLL_SECS, MAX_FLEET_POLL_SECS]` |
| `enabled` | Whether the fleet server admits/drives it |
| `created_at`, `updated_at` | Unix-seconds metadata (clamped non-negative) |

`FleetSession::sanitize()` clamps hostile/unset numbers fail-soft (the `poll_secs` bounds,
`0 ⇒ DEFAULT_FLEET_POLL_SECS`, negative timestamps ⇒ `0`); `FleetSession::validate()`
fails closed on structural problems (a non-path-safe id/user/repo, an unknown backend,
an over-long field, or a malformed `token_ref`).

### `token_ref` is a reference, never a secret

The forge token is stored and served as a kind-prefixed **reference** (`env:NAME` or
`file:/path`), resolved to a real token only on the host that builds the concrete
`Forge` (review-fleet C5, inc 3c). A raw value here is exactly the "secret in the
config" mistake the type exists to prevent, so `validate()` reuses the audited
[`ApiKeyRef::parse`] and fails closed — and the error never echoes the value. No secret
is ever at rest in a store or on the wire.

## Config

```toml
[review_fleet]
store        = ""                          # "file" | "sqlite" (feature) | "" (off, default)
file         = ".agent/review-fleet.json"  # JSON roster bundle (file backend)
path         = ".agent/review-fleet.sqlite3"  # sqlite roster (sqlite backend)
root         = ""                          # per-session workspace root (R1a); empty ⇒ shared cwd
max_total    = 0                           # cap on total admitted sessions (0 = unbounded; inc 3c)
max_per_user = 0                           # cap per owning org (0 = unbounded; inc 3c)
```

## Storage backends

All three backends funnel every mutation through one shared `ops` module
(sanitize → validate → cap `MAX_FLEET_ROWS` on insert), so validation, clamps, and caps
can never drift between them:

- **`MemoryFleet`** — one roster snapshot behind a mutex; the base for tests and a
  serve-only process with no backing file.
- **`FileFleet`** — one JSON bundle (`Vec<FleetSession>`) on disk; hand-editable *or*
  rewritten by a control-plane `Put`. An absent file is an empty roster; a present-but-
  invalid one (bad JSON, oversized, or a row that fails validation) is an error on every
  operation — never a partially-loaded roster. Writes are validate-then-persist via a
  same-directory temp file + atomic rename; reads re-validate every row (defending
  against out-of-band edits).
- **`SqliteFleet`** (feature `fleet-sqlite`, off by default) — each row as its JSON form
  in an embedded-SQLite BLOB (the same at-rest shape as the file backend). Ids reach SQL
  only as bound parameters; reads re-decode + re-validate, failing closed on tampering.

## Testing

Table-driven `rstest` with a `desc` + `expect` column on every row (`crud_contract` in
[`lib.rs`](../../crates/agent-review-fleet/src/lib.rs)), covering all four case classes
(`positive_`/`negative_`/`boundary_`/`corner_`) plus mandatory `adversarial_` cases
(traversal ids, raw-token refusal with no echo, the rows cap, out-of-band tampering).
A per-backend equivalence test asserts the memory/file/sqlite backends agree. The
default-feature tests run in `nix/checks/test.nix`; the sqlite backend is executed by
the feature-scoped `nix/checks/fleet-sqlite.nix` gate (the review-fleet twin of
`prompt-sqlite`).

[`ApiKeyRef::parse`]: ../../crates/agent-core/src/lib.rs
