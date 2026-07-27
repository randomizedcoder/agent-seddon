# 05 — Session lifecycle: `SessionRegistry`

A session needs a beginning (mint an id, associate a user + working dir, let servers
pre-allocate) and an end (free per-session state: PTYs, handle maps, event sinks). We
add an **explicit `SessionRegistry` service for association**, but keep per-seam state
**lazily allocated on first use and idle-GC'd**, so the lifecycle RPCs are an
optimization/cleanup signal — **not** a correctness dependency.

Why both: an explicit registry gives one place to (a) validate identity + working dir
once, (b) let exec seams deliberately allocate (PTYs especially), and (c) free state
promptly. But a *purely* explicit model is brittle across a distributed system — if
the agent crashes between `Open` and `Close`, or a seam server restarts, orphaned
state accrues. So `Close` is best-effort and **idle-GC is the real guarantee**.

## The wire (new `session_registry.proto`)

A brand-new file/service (distinct from `agent_session.proto` = the live-event
`SessionSource`, and `session.proto` = the content-addressed `SessionStore`). Being
all additions, `buf breaking` sees only new symbols → **no baseline bump.**

```proto
package agent.v1;

service SessionRegistry {
  rpc Open(OpenRequest)   returns (OpenResponse);
  rpc Close(CloseRequest) returns (CloseResponse);
  rpc Heartbeat(HeartbeatRequest) returns (HeartbeatResponse);  // resets idle timer
}

message OpenRequest {
  string user_id     = 1;   // advisory until auth (07); server MAY override from a token
  string working_dir = 2;   // the confinement root; server confines/validates
  // session_id is server-minted and returned — NOT client-supplied.
}
message OpenResponse   { string session_id = 1; }
message CloseRequest   { string session_id = 1; }
message CloseResponse  {}
message HeartbeatRequest  { string session_id = 1; }
message HeartbeatResponse {}
```

Key points:

- **`session_id` is server-minted** (the registry owns the `Uuid::new_v4()` that
  `main.rs` mints today), returned to the client, and thereafter carried in the
  `x-agent-session-id` **metadata** on every subsequent RPC
  ([`01-identity.md`](01-identity.md)). This removes client-chosen-id
  collision/prediction and makes the metadata value trustworthy *as routing*.
- **Working dir validated once** at `Open` via `confine`/`safe_segment`, establishing
  the per-user confinement root that [`04-tenancy.md`](04-tenancy.md) relies on for
  tool/search/repo workers.
- **Idle-GC is the backstop.** Each seam server tracks `last_used` per key; a
  background reaper frees memory handles, closes PTYs, and drops backend maps after an
  idle window. `Heartbeat` keeps a long-lived session warm. This is what bounds
  resource use under the "low hundreds" constraint and survives a lost `Close`.
- **Optional for stateless seams** — they ignore it. Only stateful/rooted seams
  consult the registered root/state.

## Deployment shapes

- **`--serve-all` (one process):** the registry lives on the shared router; other
  seams read the same in-process session table.
- **Split (one seam per process):** each server keeps its **own** lazy table keyed by
  the incoming metadata `SessionKey`, and `Open` is a per-server hint. This is exactly
  why lazy-on-first-use must remain the correctness path — a seam may receive its
  first request for a session it never saw an `Open` for (a different process handled
  `Open`), and must allocate on demand.

## Amplification guard

A hostile client must not be able to force unbounded per-`SessionKey` allocation by
spraying new session_ids (mirroring the existing `grpc-retry-pushback-ms` hardening
against a hostile *server*). Bound the live-session map — the "low hundreds"
constraint gives a concrete cap (reject/evict beyond N per user) — and idle-GC the
rest. See [`07-security.md`](07-security.md).

## Relationship to the in-process `SessionManager`

The in-process [`SessionManager`](02-runtime-split.md) is the *local* owner of live
`Session` objects; `SessionRegistry` is its *wire* face for remote clients (the
portal). `Open` → `manager.get_or_create`; `Close` → `manager.remove`; `Heartbeat`
touches the session's `last_used`. The single-session CLI needs neither RPC — it mints
its default key locally.

## What lands where

| File | Change |
|---|---|
| `crates/agent-proto/proto/agent/v1/session_registry.proto` (new) | the lifecycle service. |
| `crates/agent-proto/{build.rs, src/lib.rs, src/convert.rs}` | compile + descriptor-set test + conversions. |
| `crates/agent-core/src/lib.rs` | a `SessionRegistry` trait (mint/close/touch). |
| `crates/agent-grpc/src/{server,client}/session_registry.rs` (new) | service + client. |
| `crates/agent-runtime/src/agent.rs` | `SessionManager` implements the registry ops; idle-GC reaper task. |
| `crates/agent-cli/src/{main.rs,grpc_server.rs}` | server-mint on `Open`; `SEAMS` row + `--serve-session-registry`; `--serve-all` shared table. |
| `nix/constants.nix` (+ `gen-constants`) | a port/socket row for the new seam. |

## Tests

- `positive_` — `Open` mints a server-side id; a subsequent metadata-carried call
  finds the session; `Close` frees its state.
- `adversarial_` — a client-supplied `session_id` on `Open` is ignored (server mints);
  spraying `Open` beyond the per-user cap is rejected; a lost `Close` is reclaimed by
  idle-GC.
- `boundary_` — first request for an un-`Open`ed session (split deployment) allocates
  lazily; `Heartbeat` prevents reaping.
- Round-trip over **TCP and UDS**; confirm additive → no baseline bump.
