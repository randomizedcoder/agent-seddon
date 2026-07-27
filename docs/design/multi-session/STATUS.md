# STATUS — multi-session / multi-user

Authoritative tracker for the track. Where a shipped increment refines a detail in the
design docs, this file records the refinement (the design docs stay the rationale).

## Increments

| # | Increment | Design doc | State |
|---|---|---|---|
| 00 | Design docs | this directory | **written** (pre-implementation) |
| 01 | Identity foundations (`SessionKey`/`safe_segment`, metadata keys, task-local, inject/extract, span attrs) | [`01-identity.md`](01-identity.md) | **merged** (#151) |
| 02 | `Backend`/`Session`/`SessionManager` split; per-session cwd | [`02-runtime-split.md`](02-runtime-split.md) | **merged** (#152) |
| 03 | Hazard A: stateless `ContextStrategy::compact(switch)` | [`03-hazards.md`](03-hazards.md) | **in PR** (`feat/multi-session-03-hazards`) |
| 03b | Hazard B: `SessionEventsRegistry` + `session_id` selector on `AgentSessionService` | [`03-hazards.md`](03-hazards.md) | pending |
| 04 | Per-user memory/dimension tenancy; stateful-seam keying; confined cwd/root | [`04-tenancy.md`](04-tenancy.md) | pending |
| 05 | `SessionRegistry` lifecycle (server-mint, idle-GC) | [`05-lifecycle.md`](05-lifecycle.md) | pending |
| 06 | Session-scoped observability (span attrs, curated metric label, eviction) | [`06-observability.md`](06-observability.md) | pending |
| 07 | Security hardening (`bash` containment, UDS-per-user, adversarial sweep) | [`07-security.md`](07-security.md) | pending |

> Note: increments 04 and 06 map to two design docs each in some places (tenancy +
> lifecycle share the confinement-root story; observability spans traces + metrics).
> The PR breakdown may split or merge them; this table tracks the design docs.

## Decisions locked (from the design review)

- **Auth out of scope; isolation in scope.** Identity = routing/namespacing labels,
  "only as trustworthy as the transport." Filesystem namespacing enforces isolation
  even against a spoofed identity. Token→user_id auth is a named follow-up.
- **Wire = hybrid.** Ambient identity via gRPC metadata (`x-agent-session-id`,
  `x-agent-user-id`); typed fields only for `SessionRegistry`, `SessionStore`, and the
  per-call confined cwd/root. All changes additive → **no `buf.image.binpb` bump**.
- **Tenancy = per-user, cross-session within a user.** Memory/dimensions rooted at
  `<root>/<user>/…`; dimensions key by user.
- **Concurrency = shared `Backend` + owned `Session`s** in a `SessionManager` map
  (`Arc<tokio::Mutex<Session>>` per key); single-session path zero-overhead.

## Refinements (authoritative over the design docs)

- **Increment 03 split into 03 (hazard A) + 03b (hazard B).** The two hazards are
  independent, so they ship as separate PRs for reviewability. **03 (hazard A)** made
  `ContextStrategy` stateless: `compact(&self, working, budget, switch: Option<(TaskMode,
  TaskMode)>) -> Result<CompactAction>` — the armed switch is a **caller-owned
  parameter** (a `Session.pending_switch` field), so one shared strategy `Arc` serves
  many concurrent sessions without racing; `on_mode_switch`/`last_compact_action` are
  deleted. **No proto change was needed** — `CompactRequest` already carried
  `from_mode`/`to_mode` and `CompactResponse` the action (from adaptive-cognition 02),
  so the wire already threads the switch and returns the action; `GrpcContext`/
  `ContextService` just drop their `pending`/`last_action` `Mutex`es. `chosen`: the
  stateless option over `fork()` (recommended in 03-hazards.md), and it's free here
  *and* correct for a `context = "grpc"` deployment. **03b (hazard B, pending)** is the
  `SessionEvents` → keyed-registry + `session_id` selector work.

- **Increment 02 shape.** `Session` now **owns `Arc<Agent>`** (field still named `agent`,
  so the loop's `self.agent.<seam>` calls are unchanged via `Deref`) and carries its
  `SessionKey`; `Session` dropped its `<'a>` lifetime. A `SessionManager`
  (`HashMap<SessionKey, Arc<tokio::Mutex<Session>>>`) owns the map. **`Agent` plays the
  `Backend` role directly** — the cosmetic rename to a separate `Backend` type is
  deferred; the ownership split (not the rename) is what delivers concurrent sessions.
  `Agent::run`/`session` take `self: &Arc<Self>`; **`build_agent`/`build_agent_with`
  now return `Arc<Agent>`** (a public API change). The identity **carrier**
  (`AGENT_IDENTITY` task-local + `scope`/`current_identity`) **moved to `agent-core`**
  (which gained a `tokio` dep) from `agent-grpc`, because `agent-grpc` is an optional
  dep of `agent-runtime` and the loop's `Session::send` must scope identity
  unconditionally; `agent-proto::identity` (the wire keys) is unchanged.
  `Session::send` scopes the turn's identity and stamps `session_id`/`user_id` on the
  `agent.turn` span. **Per-user cwd namespacing is deferred to increment 04** (it pairs
  with the memory/dimension rooting); 02 keeps the single process `working_dir`.

- **Increment 01 scope.** The identity *mechanism* landed: `SessionKey`/`UserId`/
  `SessionId`/`safe_segment` in `agent-core`; `agent-proto::identity` metadata keys +
  inject/extract; the `AGENT_IDENTITY` `tokio::task_local` in `agent-grpc`; injection
  in `client::outbound()`; extraction + `safe_segment`-validated span attributes in
  `server::span()`; and the origin scope in `agent-cli/main.rs`. **Fail-closed
  *rejection* of absent identity on stateful RPCs is deferred** to when the client
  side reliably sends identity (increments 02+/04): enforcing it in 01 would break the
  single-process gRPC path and the round-trip suite, which send no identity yet. 01
  validates-when-present and never rejects, so it is fully backward-compatible.

## Open decisions (settle at implementation)

- **Concurrency shape** — `Arc<Mutex<Session>>` map (recommended) vs.
  actor-per-session. Settle in **02**.
- **`ContextStrategy` fix** — stateless `compact(switch)` (recommended; ripples the
  `ContextService` proto additively) vs. `fn fork()` (no proto change, keeps
  per-instance mutable state). Settle in **03**.
- **Metric retention policy** — `remove_label_values` on session end **and/or** an LRU
  cap; confirm the cap value. Settle in **06**.

## Deferred / follow-ups (explicitly out of scope)

- **Authentication interceptor** (token → verified `user_id`, ignoring client-supplied
  header) — the prerequisite that upgrades identity labels to a real tenancy boundary.
  Composes with the existing mTLS follow-up in [`grpc.md`](../../grpc.md#possible-follow-ups).
- **Multi-tenant job *execution*** for `--serve-scheduler` (needs the off-trait
  executor-claim protocol; today it gains storage only).
- **Container/multi-host deployment** — unchanged non-goal (README "Running seams as
  services").
- **Cross-user memory sharing / team spaces** — the tenancy model is per-user; a shared
  team namespace is a future extension of the rooting scheme.

## Verification gate (per increment)

- Table-driven tests with **mandatory `adversarial_`** cases for the identity/isolation
  surface (traversal, spoofing, absent-identity fail-closed).
- Isolation e2e: two sessions (`alice`/`bob`) over one `--serve-all` gateway assert no
  cross-tenant read of memory/checkpoints/events/files.
- Round-trip new/changed services over **TCP and UDS**.
- `nix flake check` green (clippy `-D warnings`, tests, bench/leak); **confirm
  `buf breaking` passes with no baseline bump** — `nix run .#buf-image` should not be
  needed.
