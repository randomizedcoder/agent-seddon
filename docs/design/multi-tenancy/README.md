# Multi-tenancy platform

Turning the agent from "one operator's tool" into a system that can serve **many mutually
distrustful organizations** — safely and at speed. This track extends the per-user tenancy the
[multi-session](../multi-session/) track began (memory/session/metrics keyed by `user`) to
**every remaining seam**: process execution, the analytics/search datastores, and all
feature *configuration and state* (LLM upstreams, routing/load-balancing, cognition graphs,
prompts, scheduler).

It was surfaced by the [review-fleet](../review-fleet/) design (a fleet reviewing different
orgs' repos is the forcing function), but it is **system-wide**, not fleet-specific — hence its
own track. The fleet is its first consumer.

## The three planes

A tenant boundary has to hold against three distinct adversaries; each plane closes one:

| Plane | Doc | Adversary | Mechanism |
|---|---|---|---|
| **01 — process isolation** | [`01-process-isolation.md`](01-process-isolation.md) | attacker **code** (a malicious PR the agent executes) | pluggable `Sandbox` backends (bwrap → OCI → microVM): namespaces + seccomp + cgroups + netns; the five isolation pillars |
| **02 — data scoping / RLS** | [`02-data-scoping-and-rls.md`](02-data-scoping-and-rls.md) | attacker **reads** (a prompt-injected agent querying shared stores) | verified `tenant` on every row/span/series + ClickHouse `ROW POLICY` bound to a per-tenant credential + per-tenant search indexes |
| **03 — config & seam state** | [`03-config-and-state-tenancy.md`](03-config-and-state-tenancy.md) | attacker **config** (reading/editing another tenant's upstreams, routing, prompts, graphs) | operator-global vs per-tenant config split; `PerTenant<Store>` wrapper over registry/graph/prompt/scheduler; tenant-scoped control plane |

The component catalogue (C23–C31, continuing the review-fleet numbering) is in
[`00-components.md`](00-components.md); the tracker is [`STATUS.md`](STATUS.md).

## Organizing principles (shared across the three planes)

1. **Tenant = organization**, mapped onto `SessionKey.user` (reusing multi-session's per-user
   machinery); `session` is a sub-scope. No third tier (org→team→user) in v1.
2. **Structural, not trusted.** Every boundary is bound to the *verified ambient identity*
   (`AGENT_IDENTITY`), never a value the prompt-injectable model supplies. A tenant holds no
   credential/handle that can reach another tenant. (Only as strong as the transport/auth
   boundary — auth is the multi-session 07 follow-up.)
3. **Tiered & pluggable.** One `[tenancy]`/`[sandbox]` tier switch: **Tier 0** (single operator,
   today's behavior — nothing wrapped) → **Tier 1** (semi-trusted) → **Tier 2** (mutually
   distrustful SaaS). Deployment dials it; the code doesn't fork.
4. **Reuse the proven pattern.** `PerUserMemory` already does per-tenant internal routing with
   no trait change; planes 02/03 generalize it (`PerTenant<Store>`), and the `Sandbox` seam
   already carries the fields plane 01 needs.
5. **Security = performance, not a tax.** `tenant` as the leading ClickHouse sort key makes
   scoped reads *prune* other tenants (faster, not slower); path-partitioned indexes and
   per-tenant stores keep each tenant's working set small.

## Current state (why this is real work)

Grounded audits (recorded in the plane docs) found: the system is **single-global-config, one
`Arc<dyn Trait>` per seam**; the `Sandbox` fields exist but no backend enforces them and only
`bash` runs through the seam; all 7 telemetry tables carry `session_id` but **no tenant**;
tantivy indexes are shared; and only memory/dimensions are per-tenant. So this track is
additive-behind-a-tier where possible, but touches the execution path, the telemetry schema,
and every store-backed seam — the widest rework in the combined design.

## Build order

`01` (chokepoint C24 first, then backends) · `02` (identity-at-source C26 first — cheapest,
also unblocks fleet observability — then RLS) · `03` (`PerTenant<Store>` over the registry/graph/
prompt/scheduler). C26 and the exec-`NetworkPolicy::Off`/`EnvPolicy::Scrub` intent are worth
landing *during* the fleet build (Tier 0) so the enforcement here is drop-in — see the
review-fleet [`IMPLEMENTATION.md`](../review-fleet/IMPLEMENTATION.md) "foundation now /
enforcement later" split.

## Relationship to other tracks

- **[multi-session](../multi-session/)** — established `(user, session)` identity + per-user
  memory tenancy + the UDS-per-user boundary + the auth follow-up. This track is its
  continuation to the remaining seams.
- **[review-fleet](../review-fleet/)** — the forcing function and first consumer; its increment 1
  (per-session workspace) and increment 6 (tenant-tagged review tables) lay foundation this
  track's enforcement builds on.
