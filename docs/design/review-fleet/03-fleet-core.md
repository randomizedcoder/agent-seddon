# Increment 3 — fleet core + persisted registry

Components: **C1** (server) · **C2** (roster) · **C3** (control plane) · **C8** (orchestrator
skeleton). This stands up the process, its durable roster, the CRUD control plane, and the
state machine — with triggers (inc 4) and the draft/post tail (inc 6) filled in later.

## C2 — persisted roster (`FleetRegistry`)

Model exactly on `agent-registry` (a trait + a private `ops` core + memory/file/sqlite
backends over identical `ops`, so no backend can drift — `agent-registry/src/{lib.rs:149,
file.rs:24, sqlite.rs:31}`).

- New crate `agent-review-fleet`, module `registry`. `trait FleetRegistry { list, get, put,
  delete, set_enabled }`, all routed through `mod ops` for validation + caps.
- `FleetSession` row = the fields in `00-components.md#C2`. Backends: `MemoryRegistry`
  (tests), `FileRegistry` (one bundle on disk), `SqliteRegistry` (feature `fleet-sqlite`,
  the default; path from `[review_fleet] store`).
- `ops` validation: `safe_segment` on `id`/`user`/`repo` segments; `token_ref` scheme
  allow-list `{env:, file:}` (refuse a raw token, as the provider registry refuses raw
  `api_key`); `poll_secs` clamped to `[MIN_POLL, MAX_POLL]`; total-rows cap.

## C3 — `ReviewFleetService` (control plane)

New `review_fleet.proto` in `agent-proto` + a gRPC service, mirroring
`ProviderRegistryService` (model-router 03). RPCs `Put / Delete / SetEnabled / List / Get`.
Additive to the wire → `buf breaking` passes untouched; the buf baseline moves only if a
later edit is wire-incompatible.

- Every RPC runs its input through C2's `ops` (one validation path for wire + local edits).
- `Get`/`List` **never** return a resolved token — only the `token_ref`.

## C1 — fleet server (`agent --serve-fleet`)

Mirror `serve_sessions` (`agent-cli/src/grpc_server.rs:761`) and its flag/mode wiring
(`main.rs:596/357`). New `Mode::ServeFleet(listen)` + `serve_fleet(agent, listen)`:

1. Build `Arc<SessionManager>` **with `with_limits(max_total, max_per_user)` wired** from
   `[review_fleet]` config — this closes the unbounded-`SessionManager` TODO
   (`session_manager.rs:174`) and is the scale guard (README/scale).
2. Start the idle-GC reaper (as `serve_sessions` does).
3. Open the roster (C2), reconcile it into live per-session state: for each `enabled` row,
   admit a session (`SessionManager::admit`, `session_manager.rs:231`) with its confined
   workspace (C4) + scoped `Forge` (C5), and register its trigger tasks (inc 4).
4. Register `ReviewFleetService` (C3) and the `AgentSessionSvc::with_driver` used by
   `serve_sessions`.
5. On a `ReviewFleetService` mutation, reconcile the affected session (admit / drop /
   re-enable) without a restart.

Endpoint from `constants::FLEET` (new block in `nix/constants.nix`, next to `sessions` —
propose `port = 50081`, `metrics_port = 9631`, `socket = "$socketDir/fleet.sock"`) plus a
`[grpc.fleet] listen` override. Regenerate `constants.rs` (`nix run .#gen-constants`;
`constants-sync` check enforces it).

## C8 — orchestrator skeleton

The per-(session, PR) state machine, states in `00-components.md#C8`. This increment lands
the skeleton up to `reviewing`:

- A bounded per-session queue of `Trigger { session, pr_number }` (triggers filled in
  inc 4; a manual "review now" over C3 can feed it meanwhile for testing).
- `triggered → cloning`: `fetch_pr` + `worktree_add` (C9, inc 2).
- `cloning → reviewing`: admit/reuse the session and drive it with a review goal, the way
  `AgentSessionService.send` drives one (`agent_session.rs:132`; `RunHandle` drop =
  cancel). The `drafted → awaiting-approval → posted` tail is inc 6.
- One in-flight review per session (a per-(session,PR) guard); a duplicate trigger for a
  head already being/handled is a no-op (full dedup against C14 arrives in inc 6).
- Queue overflow **coalesces and logs** (no silent drop).

## Security

- Control-plane input untrusted → all validation in `ops`; `0o600` UDS per user; no auth
  layer (transport-trust, documented).
- `with_limits` prevents a roster (or a trigger storm) from exhausting the host.
- Reconciliation is idempotent and crash-safe: the roster is the source of truth; live state
  is rebuilt from it on restart.

## Test matrix

`FleetRegistry` (per-backend `#[case]` memory/file/sqlite, the agent-registry pattern):
- `positive_put_then_get_roundtrips`, `positive_list_returns_enabled_and_disabled`,
  `positive_set_enabled_toggles`, `positive_delete_removes`.
- `boundary_poll_secs_clamped_to_bounds`.
- `corner_put_same_id_updates_in_place`.
- `negative_raw_token_in_token_ref_refused`.
- `adversarial_traversal_id_rejected`, `adversarial_over_rows_cap_rejected`.
- `positive_sqlite_survives_reopen` (persistence).

Server / orchestrator:
- `positive_reconcile_admits_enabled_sessions`.
- `positive_disable_over_control_plane_drops_session`.
- `boundary_with_limits_rejects_over_capacity` (RESOURCE_EXHAUSTED, mirrors
  `session_manager` admit tests).
- `corner_duplicate_trigger_same_pr_is_noop`.
- `adversarial_control_plane_never_returns_token`.

## Done when

`nix flake check` green (incl. `buf breaking` additive + `constants-sync`); `agent
--serve-fleet` boots, loads a sqlite roster, exposes CRUD over `ReviewFleetService`, admits
one capped session per enabled row into a confined workspace with a scoped forge, and drives
a manually-queued PR through `cloning → reviewing`. Survives restart from the roster.
