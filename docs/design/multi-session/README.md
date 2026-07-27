# Design: Multi-session / multi-user

Status: **design / pre-implementation.** This directory is the design of record for
making `agent-seddon` serve **many users and many concurrent sessions** over its
existing seam + gRPC architecture. It is a draft to refine; where shipped code later
refines a detail, [`STATUS.md`](STATUS.md) becomes authoritative — the same
convention as [`../adaptive-cognition/`](../adaptive-cognition/README.md),
[`../code-review/`](../code-review/README.md), and [`../portal/`](../portal/README.md).

## The idea

Every capability in `agent-seddon` is a [seam](../../architecture.md) — an `async`
trait in `agent-core`, swappable by config, individually instrumented, and (for 26
of them) hostable as its own gRPC service over TCP/UDS. That distributed shape means
the components *could* serve many users at once — imagine Alice and Bob both driving
the agent from the Flutter portal, each authenticated, each issued a session ID that
their client passes on every gRPC call.

But the harness is **single-session today.** Multi-user serving was an explicit v1
non-goal ([`DESIGN.md`](../../../DESIGN.md) §1); §12 lifted only the *distribution*
part (a seam can run in another process), not session *isolation* (two sessions
sharing one process/service without leaking into each other). A `session_id` UUID
already exists but is used only as a telemetry stamp, a git-worktree directory name,
and a `SessionStore` key.

This design adds session/user isolation as a **foundational** capability across the
whole system, in three parts:

> **Identity that rides the wire.** A `(user_id, session_id)` pair travels with every
> gRPC call, so any seam — local or remote — knows which tenant it is serving.
>
> **State that no longer leaks.** Every place that holds per-conversation state today
> (the transcript, the working memory, dimensional memory, live terminals, the event
> feed, the compaction strategy) is scoped so Alice's session cannot read or clobber
> Bob's.
>
> **Observability per tenant.** Traces and metrics are attributed to a session and a
> user, so a run's cost, latency, and span tree can be filtered by who it was for —
> without a cardinality explosion.

## Scope decisions

Four forks were settled up front (see the [plan](../../../DESIGN.md) history); the
rest of this directory elaborates them.

| Decision | Choice | Why |
|---|---|---|
| **Authentication** | **Out of scope; isolation in scope** | Wire `user_id`/`session_id` are attacker-controllable and trusted **only as routing/namespacing labels, only as far as the transport (UDS perms / loopback) already trusts the peer**. Isolation is enforced *structurally* — filesystem namespacing via `confine`/`safe_segment` — so even a spoofed identity cannot escape the namespace it names. A real `auth-token → user_id` interceptor is a **named prerequisite/follow-up** ([`07-security.md`](07-security.md)). |
| **Wire mechanism** | **Hybrid** | Ambient transport identity rides as **gRPC metadata** (like trace context — one client + one server chokepoint, zero proto edits, no buf baseline bump). **Typed fields** are added only where the session is genuinely part of the operation: a new `SessionRegistry` service, `SessionStore`'s existing `session` arg, and a per-call confined cwd/root on tool/search/repo. ([`01-identity.md`](01-identity.md)) |
| **Memory tenancy** | **Per-user, cross-session within a user** | A user's episodic/semantic/dimensional memory is shared across *their own* sessions (learning accumulates) but isolated from other users. ([`04-tenancy.md`](04-tenancy.md)) |
| **Concurrency** | **Shared `Backend` + owned `Session`s** | One process runs N sessions over one shared, mostly-functional backend; a per-session lock serializes turns *within* a session while distinct sessions run in parallel. ([`02-runtime-split.md`](02-runtime-split.md)) |

## Architecture at a glance

```
                         ┌───────────────── one agent process ─────────────────┐
  Alice ─(x-agent-user-id: alice, x-agent-session-id: S1)─▶ SessionManager       │
  Bob   ─(x-agent-user-id: bob,   x-agent-session-id: S2)─▶  ├─ Session(alice,S1)─┐
                                                             │  └─ Session(bob,S2)─┤
                                                             ▼                     │
                                                        Arc<Backend>  (shared)     │
                                          provider · tools · policy · context …    │
                                          memory/dims rooted at <root>/<user>/…    │
                         └─────────────────────────────────────────────────────────┘
                                   │ = "grpc" seam call carries the same metadata
                                   ▼
                         ┌─ agent --serve-<seam> (another process) ─┐
                         │ server::span() extracts (user,session),   │
                         │ scopes a task-local, keys its state,      │
                         │ fails closed if identity absent/malformed │
                         └───────────────────────────────────────────┘
```

The identity is set once at the agent root (or minted by `SessionRegistry.Open`),
carried in a `tokio::task_local`, injected into gRPC metadata by the client
chokepoint every seam already routes through, and extracted + validated + re-scoped
by the server chokepoint. A server that itself calls further seams (e.g. `--serve-all`
where Context calls Provider) forwards the caller's identity transparently — exactly
how trace context already flows.

## The documents

- [`01-identity.md`](01-identity.md) — the `(user_id, session_id)` model, the
  `SessionKey`/`safe_segment` primitives in `agent-core`, the metadata keys, the
  `tokio::task_local` carrier (and why **not** OTel baggage), and the two-chokepoint
  propagation.
- [`02-runtime-split.md`](02-runtime-split.md) — splitting `Agent` into a shared
  `Backend` + owned `Session` + `SessionManager`; how the single-session CLI/REPL
  path stays zero-overhead and the portal's many-session path reuses it.
- [`03-hazards.md`](03-hazards.md) — the two Agent-global single-loop hazards
  (`ModeAwareWindow` mutable state; `SessionEvents` single broadcast) and their fixes.
- [`04-tenancy.md`](04-tenancy.md) — how each stateful seam keys its state by session
  (and user): memory/dimensions, `SessionStore`, `TaskTracker`, `Pty`, `Scheduler`,
  and the tool/search/repo confinement root.
- [`05-lifecycle.md`](05-lifecycle.md) — the `SessionRegistry` service
  (`Open`/`Close`/`Heartbeat`), server-minted ids, lazy-alloc + idle-GC.
- [`06-observability.md`](06-observability.md) — session/user span attributes, the
  curated per-session Prometheus label set, series eviction, and why `MetricsProxy`
  needs no change.
- [`07-security.md`](07-security.md) — the trust boundary, fail-closed rules, the
  `bash` escape-hatch containment gap, and the named auth follow-up.
- [`STATUS.md`](STATUS.md) — the increment tracker (authoritative where it refines a
  detail here).

## Non-goals

- **No authentication mechanism.** This design *isolates* tenants; it does not
  *authenticate* them. See [`07-security.md`](07-security.md) for the boundary and the
  named follow-up.
- **No new deployment story.** Like every seam today, this runs over loopback / UDS;
  container images and multi-host orchestration remain out of scope (README's
  "Running seams as services").
- **No cardinality engineering.** The design targets **low hundreds** of concurrent
  sessions, where a `session` metric label is within budget. It bounds series growth
  (eviction on session end) but does not build cardinality-reduction machinery.
- **No multi-tenant job *execution*.** `--serve-scheduler` stays management-only
  (driving needs the off-trait executor closure); it gains multi-tenant job *storage*,
  not execution. See [`04-tenancy.md`](04-tenancy.md).
