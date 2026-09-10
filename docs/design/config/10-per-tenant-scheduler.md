# 10 — Per-tenant scheduling (design of record)

**Status: ⬜ designed, not built.** This is the design of record for making the
`Scheduler` seam multi-tenant. It was split out of the per-tenant plane increment
**C2b**, which shipped per-tenant routing for the *file-backed cognition graph* but
**deliberately left the scheduler alone**, because — unlike every other seam the
per-tenant plane wraps — the scheduler cannot be made multi-tenant by a thin
`PerTenant<S>` routing layer. This document says why, and what a real
implementation would take.

## Why the scheduler is not a `PerTenant` wrap

The three shared-store seams (`ProviderRegistry`, `FleetRegistry`, `PromptStore`)
and the cognition graph are all **passive stores**: a call reads or writes a record,
and isolation is achieved by choosing the caller's own view — a `(collection,
tenant, id)` key for the shared store, or a `tenants/<tenant>/` path for the graph
file. `PerTenant<S>` (`crates/agent-runtime/src/tenant.rs`) does exactly that: it
resolves the verified tenant on each call and delegates to a per-tenant view.

The scheduler is **not** a passive store. It has two halves:

1. A **registry** — `schedule` / `list` / `cancel` / `history`, on the `Scheduler`
   trait (`agent-core/src/lib.rs`). This half *could* be routed per tenant.
2. A **driver** — `LocalScheduler::tick_with(exec)` ticks on an interval and fires
   each due job by **running it as a fresh headless turn of the owning agent**.
   `tick_with` is **deliberately not on the `Scheduler` trait**, and `Agent`
   holds the **concrete** `Arc<LocalScheduler>`, not a trait object
   (`agent-runtime/src/agent.rs` — `with_scheduler`, `scheduler`,
   `scheduler_seam`). The doc comment there states the contract outright: *"a job's
   executor is this agent. So a remote client can manage the registry … but only
   the process that owns the scheduler can fire its jobs."*

The two halves are inseparable in the current design: **the jobs live in the same
in-memory `LocalScheduler` the driver ticks.** So if the registry half were wrapped
in `PerTenant` (giving each tenant its own in-memory job map), the single concrete
driver would tick only the **base (`local`) instance** — every job a non-`local`
tenant scheduled would be accepted, listed, and then **silently never fire**. That
is precisely the "oversold isolation" failure `CLAUDE.md` warns against: a control
surface that looks tenant-isolated but whose writes quietly do nothing. A partial
wrap here is worse than no wrap.

True per-tenant scheduling therefore needs the jobs to be **durable and
tenant-keyed**, and the driver to **fan out over tenants** — a genuine
architectural change, not a routing decorator.

## Design

### D1. A durable, tenant-keyed scheduler backend

Add a `StoreScheduler` over the shared `agent-config-store` `Backend` (the same
transactional store `agent-registry` / `agent-review-fleet` / `agent-prompt` A3'd
onto), keyed `(collection = "scheduler", tenant, job_id)`, mirroring `StoreRegistry`:

- A `SchedulerCard` (prost blob) holding the job's `spec`, `goal`, `Schedule`,
  `next_fire_ms`, `claim` (owner + `claim_ttl` deadline), and run history (or a
  bounded tail; full history to a sibling collection).
- `with_tenant(backend, tenant)` and `new(backend)` constructors, exactly like the
  other store seams, so `PerTenant` can wrap the **registry** half for free.
- Claims are a compare-and-set on `next_fire_ms`/`claim` inside one store
  transaction (`agent-config-store` already exposes atomic multi-card txns, config
  decision #5) — this is what lets more than one driver run without double-firing,
  and replaces `LocalScheduler`'s in-memory `claim_ttl_ms` bookkeeping.

`LocalScheduler` stays as the default single-tenant, in-memory tier (Tier-0
unchanged); `StoreScheduler` is opt-in behind a `[scheduler] store = "postgres"`
arm and the existing `[tenancy] per_tenant` switch, matching the store-seam pattern.

### D2. A tenant-fanning driver

`tick_with` becomes tenant-aware. The driver enumerates **tenants with due jobs**
(a cheap indexed query over the durable backend — `next_fire_ms <= now`, distinct
`tenant`), claims each due job transactionally, and dispatches it **under that
tenant's scoped identity** (`scope(SessionKey::parse(tenant, …))`, the same
task-local `PerTenant` routes on) so the fired turn reads that tenant's registries,
prompts, memory, and graph. Two open sub-questions, each notable but not blocking
the design:

- **Executor identity.** A fired job runs "as" the tenant. The executor must build
  (or reuse) an agent view bound to that tenant — trivial once the other seams are
  per-tenant (they already are, after C2 + C2b), but it means the driver process is
  effectively multi-tenant. If stronger isolation is required (a tenant's job must
  not run in a shared process), this composes with **plane-01 process isolation**
  (`docs/design/multi-tenancy/` C23/C24): the driver dispatches into a per-tenant
  sandbox instead of an in-process turn. That is the *right* long-term shape and is
  called out as a dependency, not duplicated here.
- **Fairness / caps.** `[scheduler] max_jobs` becomes **per tenant**; add a global
  ceiling and a round-robin/priority tick order so one tenant's backlog cannot
  starve others. Hostile inputs (`spec`, `goal`, job counts) stay clamped exactly
  as `LocalScheduler` clamps them today.

### D3. The `--serve-scheduler` seam

With a durable backend, the served registry seam (`--serve-scheduler`) becomes a
true `PerTenant<dyn Scheduler>` wrap over `StoreScheduler` — `schedule`/`list`/
`cancel`/`history` isolate per verified tenant, and (unlike today) the jobs are
picked up by any driver ticking that backend. The sync `name()` returns a static
label (`"per-tenant"`), as the `GrpcScheduler` client already does.

## Scope boundary

- **In scope (this design):** durable tenant-keyed job store; transactional claims;
  a tenant-fanning driver; per-tenant caps; the per-tenant served seam.
- **Out of scope / dependencies:** strong per-tenant *process* isolation of fired
  jobs (plane-01, C23/C24) — the driver dispatches in-process until then; learned
  scheduling; cross-process cron replacement.

## Relationship to the tracks

- **config C2b** built per-tenant Graph; this doc is its scheduler counterpart,
  intentionally deferred to its own track because it is a backend + driver change,
  not a config-plane wrap. See [`STATUS.md`](STATUS.md).
- **multi-tenancy plane 03 (C30/C31)** — a per-tenant scheduler is part of making
  "the whole agent" multi-tenant; the executor-isolation sub-question is where this
  meets **plane 01**. See [`../multi-tenancy/STATUS.md`](../multi-tenancy/STATUS.md).
- **config C41** — reuses the shared transactional store and its atomic txns for
  claims.
