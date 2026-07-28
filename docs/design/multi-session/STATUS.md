# STATUS — multi-session / multi-user

Authoritative tracker for the track. Where a shipped increment refines a detail in the
design docs, this file records the refinement (the design docs stay the rationale).

## Increments

| # | Increment | Design doc | State |
|---|---|---|---|
| 00 | Design docs | this directory | **written** (pre-implementation) |
| 01 | Identity foundations (`SessionKey`/`safe_segment`, metadata keys, task-local, inject/extract, span attrs) | [`01-identity.md`](01-identity.md) | **merged** (#151) |
| 02 | `Backend`/`Session`/`SessionManager` split; per-session cwd | [`02-runtime-split.md`](02-runtime-split.md) | **merged** (#152) |
| 03 | Hazard A: stateless `ContextStrategy::compact(switch)` | [`03-hazards.md`](03-hazards.md) | **merged** (#153) |
| 03b | Hazard B: `SessionEventsRegistry` + `session_id` selector on `AgentSessionService` | [`03-hazards.md`](03-hazards.md) | **merged** (#154) |
| 04 | Per-user memory/dimension tenancy (in-process) | [`04-tenancy.md`](04-tenancy.md) | **merged** (#155) |
| 04b | Served-handler identity scoping (`MemoryService`/`DimensionSvc` route the caller's tenant) | [`04-tenancy.md`](04-tenancy.md) | **merged** (#156) |
| 04c | `SessionStore` `restore`/`diff` ownership check (close the checkpoint-id read oracle) | [`04-tenancy.md`](04-tenancy.md) | **merged** (#157) |
| 04d | Remaining stateful-seam keying: `Pty`/`TaskTracker` maps, per-call confined cwd/root | [`04-tenancy.md`](04-tenancy.md) | pending |
| 05 | `SessionRegistry` lifecycle (server-mint, idle-GC) | [`05-lifecycle.md`](05-lifecycle.md) | pending |
| 06 | Session-scoped observability — `(session, user)` on the curated metric families + retire | [`06-observability.md`](06-observability.md) | **in PR** (`feat/multi-session-06-observability`) |
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

- **Increment 06 — session-scoped observability (metrics half).** The **tracing** half
  (the `session_id`/`user_id` span attrs on `agent.turn` and `grpc.server`) already
  landed in 01/02, so this increment is the **Prometheus** half. Twelve curated
  loop-level families now carry a `(session, user)` label pair: the counters
  `runs`/`tokens`/`cost_usd`/`cache_tokens`/`tool_calls`/`api_calls`/`iterations`/
  `mode_switches` gained the two labels; the three plain gauges
  `active`/`context_tokens`/`context_messages` became **`IntGaugeVec`**; `runs`/
  `iterations`/`run_seconds` were promoted from label-less `IntCounter`/`Histogram` to
  `*Vec`. The seam-health families (`metered.rs`, ~120 of them) stay **label-less** — a
  regression test guards that. Recording goes through a new **`SessionMetrics`** view
  (`Metrics::for_session(session, user)`): the ~10 curated recorder methods **moved off
  `Metrics` onto `SessionMetrics`** (which binds the pair once); `Session` holds one,
  built in `session_with` and threaded into `run_loop` as `&SessionMetrics` (the same
  pattern as `events`). **Retire:** `SessionManager::remove` calls
  `SessionMetrics::retire()`, which `remove_label_values` the three **gauge** series —
  the ones whose staleness is a *correctness* bug (a dead session's `agent_active = 1`
  over-counts live sessions). The cumulative **counter** series are intentionally left
  (frozen, not removed): per the locked **low-hundreds-sessions** constraint their
  accumulation is within budget, and `remove_label_values` on a multi-label counter
  needs the full `(model, kind, …)` tuple the session didn't track. `MetricsProxy` and
  the two histograms `api_call_seconds`/`mode_switch_confidence` are unchanged (health).
  Bench ceilings held (`new_registry` 1.12M < 1.15M, `record_and_encode` 1.92M < 2.0M) —
  **no bump needed**. **Deferred:** the Grafana tenant dashboard row (generated
  provisioning; visual, no code path); full counter-series LRU/tuple-retirement (not
  needed at low-hundreds).

- **Increment 04c — `SessionStore` `restore`/`diff` ownership check.** `restore(id)` and
  `diff(a, b)` took a bare content-addressed `CheckpointId` with **no scoping** — and ids
  are guessable FNV hashes of content, so a stray id let any caller read any checkpoint
  (a cross-tenant read oracle). Fix (**no trait or proto change** — the design's "trait/
  impl, not wire" claim holds via the *ambient* identity): `FileSessionStore` gained an
  `authorize_read(id)` gate that, **when an identity is scoped**, requires the id
  reachable from the caller's session's branch heads (`reachable_from_session`, the same
  head-chain walk `list`/`prune` already do). The two served handlers (`server/session.rs`
  `restore`/`diff`) adopt 04b's `run_scoped(identity_key(meta), …)` so the store sees the
  caller's identity. **Denial returns the same `checkpoint not found` as a truly-absent
  id** (no existence oracle), and an **unscoped** in-process caller (the single-user CLI,
  or a client that sends no identity) is unaffected (back-compat).
  - **Residual (documented, deferred to auth follow-up / 07-security):** the check gates
    on the caller's `session` id, which is *itself* attacker-controlled metadata "only as
    trustworthy as the transport." So this closes the easy "obtain a stray id and read it"
    oracle but does **not** stop an attacker who knows a victim's (server-minted, 05)
    session id from spoofing it — that needs the token→user auth interceptor. Ownership is
    keyed by `session` (not `user`): checkpoints are inherently session-scoped snapshots,
    so per-user cross-session sharing (the memory model) does **not** apply. Namespacing
    the checkpoint store by `user` and aligning the runtime's bare `settings.session_id`
    checkpoint namespace with the tenant `SessionKey` remain follow-ups.

- **Increment 04b — served-handler identity scoping.** Completes 04's tenancy across the
  wire: the served `MemoryService` and `DimensionSvc` handlers now `scope` the caller's
  `(user, session)` around the store call, so a per-user backend (`PerUserMemory`/
  `PerUserDimensions`) routes a *remote* `recall`/`append` to **the caller's** tenant
  rather than the default `local` one. Two `pub(crate)` helpers in `agent-grpc`'s
  `server/mod.rs`: **`identity_key(meta)`** extracts `(user, session)` and returns a
  `SessionKey` **only when both segments are `safe_segment`-valid** (fail closed to
  `None` on absent/malformed — the selector is attacker-controlled); **`run_scoped(key,
  fut)`** runs the handler future under `agent_core::scope` when a key is present, else
  unscoped (the default tenant — a client that sends no identity keeps today's behaviour;
  per-seam *rejection* of absent identity stays deferred). Compute the key from
  `request.metadata()` *before* `into_inner()`. This is the reusable seam other served
  stateful seams (tools/search/repo/session-store) will adopt in 04c/05. **No proto
  change.** The standalone `--serve-episodic`/`--serve-semantic` layer services wrap
  non-per-user backends, so they're left unscoped (scoping is a no-op there). Tested:
  `identity_key` unit incl. adversarial malformed/one-segment/absent → `None`; and an
  end-to-end roundtrip (TCP+UDS) where a scoped client's identity rides the wire, the
  handler re-scopes it, and a probe store observes the caller's tenant (and `None` when
  unscoped).

- **Increment 04 scoped to *in-process* per-user memory/dimension tenancy.** The
  design's 04-tenancy.md enumerates every stateful seam; this PR ships the headline —
  **the actual `recall` leak** — and defers the rest to **04b**. Mechanism: because the
  loop shares one store `Arc` across sessions, per-user isolation is resolved **at call
  time** from the ambient identity, not baked at construction. New
  **`PerUserMemory`** (`MemoryStore`) and **`PerUserDimensions`** (`DimensionStore`) in
  `agent-memory` read `agent_core::current_identity()` on each call and delegate to a
  lazily-built, cached per-user file store rooted at `<base>/<user>/…`. **The default
  `local` user maps to the un-namespaced base path**, so the single-user CLI is
  byte-identical (no silent relocation of an existing `.agent/episodic.jsonl`); a named
  user U roots at `.agent/<U>/episodic.jsonl`, `.agent/<U>/memory/`,
  `.agent/<U>/memory/dimensions/`. The user segment is `safe_segment`-re-validated in
  the wrapper (defense in depth over the wire validation) and a malformed segment falls
  back to the base tenant, never escaping via `..`. Wired by swapping two `builder.rs`
  factory sites (composed memory + the inline dimension build); the file backend only —
  sqlite/grpc memory backends and the standalone `--serve-episodic`/`--serve-semantic`
  layers are unchanged. **Deferred to 04b** (explicitly): the served
  `MemoryService`/`DimensionSvc` handlers still don't `scope` the caller's identity into
  the store, so a *remote* call resolves to the `local` tenant (same shared behaviour as
  today — **no regression**, just not-yet-isolated); plus `SessionStore`
  `restore`/`diff` ownership, `Pty`/`TaskTracker` keying, and per-call confined
  cwd/root. **Note (`tokio::fs` gotcha):** `FileEpisodic::append` uses `tokio::fs`, whose
  `write_all().await` returns before the bytes are durably flushed — a test must read
  back through the store's own tokio reader (`recent`), not `std::fs`, or it flakes.

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
  *and* correct for a `context = "grpc"` deployment. **03b (hazard B)** is the
  `SessionEvents` → keyed-registry + `session_id` selector work (below).

- **Increment 03b (hazard B).** `SessionEvents` is now minted **per session** by a
  `SessionEventsRegistry` (`Mutex<HashMap<session_id, Arc<SessionEvents>>>`) held on the
  shared `Agent` backend; `SessionEvents` itself is **unchanged**. `Session` draws its
  own sink from the registry at construction (`session_with`) and **owns it as a field**,
  and the loop threads `&SessionEvents` into `run_loop`/`complete_streaming` — so the hot
  token-delta path does **no per-publish map lookup** (the design doc's stated goal). A
  new **`SessionSourceRegistry`** trait in `agent-core` (impl'd by
  `SessionEventsRegistry`) keeps the `agent-grpc` crate boundary: `AgentSessionSvc` now
  holds `Arc<dyn SessionSourceRegistry>` and `resolve`s the request's `session_id` —
  **empty ⇒ the sole live session** (single-session observer), empty-with-{zero,many} ⇒
  `INVALID_ARGUMENT`, a `safe_segment`-**malformed** id ⇒ `INVALID_ARGUMENT` (fail closed,
  validated before any lookup — the selector is attacker-controlled), a well-formed but
  **unknown** id ⇒ `NOT_FOUND`. `SubscribeRequest`/`SnapshotRequest` gained
  `string session_id = 1` — **additive, no `buf.image.binpb` bump**. `SessionManager::remove`
  **retires** the sink (eviction). Keyed by the **session** segment alone (the wire
  selector is a bare `session_id`; ids are unique per run, server-minted in 05); a
  colliding id is a transport-trust question, consistent with the locked stance. Used
  `Mutex` (not the doc's illustrative `RwLock`) to match the codebase's other lazy maps.

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
