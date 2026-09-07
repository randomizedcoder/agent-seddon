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
- **Control plane (inc 3b):** `ReviewFleetService` — the seam over gRPC (client `= "grpc"`)
- **Fleet process (inc 3c):** `agent --serve-fleet` — roster control plane + orchestrator
  (`ReviewNow` C8) + reconcile (C1) + per-session forge (C5)

> Increments 3a (roster) + 3b (gRPC control plane) + 3c (fleet process **skeleton**) are
> shipped. The FSM drives `triggered → cloning → reviewing`; the real triggers (forge
> poll C6 / Slack watch C7, inc 4), review skill/collectors (inc 5), and the
> draft→approve→post tail + head-oid dedup (C14, inc 6) are later increments.

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

## Control plane (`ReviewFleetService`, inc 3b)

The roster is editable at runtime over gRPC. `ReviewFleetService`
([`review_fleet.proto`](../../crates/agent-proto/proto/agent/v1/review_fleet.proto))
mirrors `ProviderRegistryService`: one process holds the roster while any number of
clients drive it (`List`/`Get`/`Put`/`Delete`/`SetEnabled`, plus the orchestrator-only
`ReviewNow` — see below). It wires in like every other seam on endpoint `constants::FLEET`
(`50086`/`fleet.sock`, metrics `9636`): it is served inside `agent --serve-all` (when a
`[review_fleet] store` is configured) and by the full `agent --serve-fleet` process, and
dialed from another process by setting `[review_fleet] store = "grpc"` (the `GrpcFleet`
client).

Two invariants hold *across the wire*, each with an `adversarial_` round-trip test in
[`roundtrip.rs`](../../crates/agent-grpc/tests/roundtrip.rs):

- **No token ever crosses the control plane.** The single `convert.rs` path copies
  `token_ref` verbatim (a reference, never resolved); `Get`/`List` return only the
  reference. There is nothing to resolve server-side, so a compromised control plane has
  no secret to exfiltrate.
- **Untrusted input fails closed at the seam.** A traversing/separator id, an unknown
  backend, an over-cap field, or a raw-secret `token_ref` is rejected in the store and
  surfaces as `InvalidArgument`; an unknown-id `Get`/`SetEnabled` maps to `NotFound`
  (the `not found` contract survives a chained `grpc → grpc` hop). Hostile numbers are
  clamped on decode before a row is ever used.

## Fleet process (`agent --serve-fleet`, inc 3c — skeleton)

`agent --serve-fleet` runs the full fleet: the roster control plane **plus** the
orchestrator ([`orchestrator.rs`](../../crates/agent-review-fleet/src/orchestrator.rs)).
(The bare Fleet *seam* served inside `--serve-all` is roster CRUD only; `--serve-fleet`
adds reconcile + the `ReviewNow` intake on top.)

**Reconcile (C1)** rebuilds the live session set from the roster — the source of truth —
so boot, re-run, and reacting to a control-plane edit all converge to the same set (a
crash-safe rebuild). For each **enabled** row it fail-closed-checks the row's forge
credential (C5) and then admits a capacity-capped (`[review_fleet] max_total`/
`max_per_user` → `SessionManager::with_limits`) placeholder **owner** session; a broken
credential, a bad id, or a full cap **skips** the row (logged), never admits a broken or
over-cap session.

**Per-session forge (C5)** — `build_session_forge` resolves the row's `token_ref` (the
same `env:`/`file:` grammar, fail-closed: a missing `file:` is a hard error) and builds
the row's `backend` forge. The `repo` safe-segment encodes the forge path — GitHub
`owner__name`, GitLab the project with `__` for `/`. A token is resolved only here, on
the fleet host — never at rest, never over the control plane.

**Orchestrator (C8)** turns a [`FleetTrigger`] `{session_id, pr_number}` into a review via
the state machine `triggered → cloning → reviewing`: fetch the PR head (C9), materialize a
read-only worktree, mint the PR-scoped `SessionKey` (`user = <org>`, `session =
encode_review_session_id(repo, pr)`), and start a review run on it (holding the
cancel-on-drop `RunHandle`). A bounded, **coalescing** `TriggerQueue` feeds it — a
duplicate or over-capacity trigger folds into the pending one and is logged, never
silently dropped — and one review runs per `(session, pr)` (head-oid–aware re-review is
inc 6, C14). Until the real triggers land (inc 4), the `ReviewNow` RPC injects triggers
manually; it is **opt-in** (only the `--serve-fleet` process wires the orchestrator's
sink — the bare seam answers `UNIMPLEMENTED`).

## Testing

Table-driven `rstest` with a `desc` + `expect` column on every row (`crud_contract` in
[`lib.rs`](../../crates/agent-review-fleet/src/lib.rs)), covering all four case classes
(`positive_`/`negative_`/`boundary_`/`corner_`) plus mandatory `adversarial_` cases
(traversal ids, raw-token refusal with no echo, the rows cap, out-of-band tampering).
A per-backend equivalence test asserts the memory/file/sqlite backends agree. The
orchestrator ([`orchestrator.rs`](../../crates/agent-review-fleet/src/orchestrator.rs))
adds hermetic, model-free tables over doubles: **reconcile** (admits enabled / skips
disabled / sheds over-capacity / idempotent re-run / fail-closed on an unresolvable
credential), the **FSM** (drives cloning→reviewing once, a duplicate is a no-op that does
not re-fetch, an unknown row errors, and dropping the run cancels it), and the **bounded
queue** (accept / coalesce-duplicate / coalesce-on-overflow / requeue-after-pop). The C5
forge builder + `resolve_token_ref` have their own env/file/missing/raw-refused table in
`agent-runtime`; the wire surface (CRUD, token-never-returned, `ReviewNow`
accepted/coalesced/unimplemented) round-trips over TCP + UDS in
[`roundtrip.rs`](../../crates/agent-grpc/tests/roundtrip.rs).

All of these run in `nix/checks/test.nix` (default features); the sqlite backend is
executed by the feature-scoped `nix/checks/fleet-sqlite.nix` gate. The opt-in real-wire
`nix run .#serve-smoke` additionally proves `ReviewFleetService` is registered on the live
binary (describe over reflection + a CRUD round-trip asserting the token reference — never
a resolved secret — comes back).

[`ApiKeyRef::parse`]: ../../crates/agent-core/src/lib.rs
[`FleetTrigger`]: ../../crates/agent-core/src/lib.rs
