# Multi-tenancy — status

Living tracker for the multi-tenancy platform (process / data / config planes). Extends the
[multi-session](../multi-session/) per-user tenancy to every remaining seam. Surfaced by, and
first consumed by, the [review-fleet](../review-fleet/) track. `nix flake check` is the gate.

Legend: ⬜ not started · 🟡 in progress · ✅ merged.

## Planes / increments

| # | Plane | Components | State | PR |
|---|---|---|---|---|
| 01 | Process isolation & multi-org boundaries | C23, C24, C25 | ✅ C23 (bwrap, 5 pillars) + C24; 🟡 C25 foundation | #454/#455 (C23), #273/#274/#275 (C24), #276 (C25) |
| 02 | Data scoping & row-level security | C26, C27, C28 | ✅ C26 (identity at source) + C27 (RLS + `user`-leading sort key); ✅ **C28 complete** — metrics tool (C28-1) + sqlite prompt (C28-2) + recall schema/redaction (C28-3a) + ClickHouse recall backend (C28-3c) + code-index per-tenant partition (C28-3d) | #315–#322 (C26), #456 (C27-1), C27-2 (sort key), #458 (C28-2), #459 (C28-1), #460 (C28-3a), #461 (C28-3c), C28-3d (code-index partition) |
| 03 | Config & seam-state tenancy | C29, C30, C31 | 🟡 **C30 built** for the shared-store seams (config C2) + the file-backed graph (config C2b); C29/C31 ⬜, scheduler designed | #302 (config C2), C2b (graph) |

**Plane 01 in progress** (via the review-fleet track): **C24 — execution chokepoint** is fully
merged (every child process — `bash`, `rg`, the whole `git` funnel — funnels through the
`Sandbox` seam, with argv-mode/no-shell + env-scrub + a no-raw-`Command` guard). **C25 — org
tenancy tier** has its *foundation* now: the `user = <org>` convention, the `repo@pr` session-id
encoder, and the per-org cap / metric-label semantics (see `SessionKey` docs). Still deferred: the
org *value* injection at the fleet mint-site (fleet core, inc 3). **C23** strong-isolation is now
**built**: the `bwrap` backend enforces all five pillars (FS/process/network/credential via rootless
namespaces, C23-1 #454; resource via cgroup limits, C23-2 #455), fail-closed, live-verified.

**Plane 02 building.** **C26 — identity at source** is done: a verified `user` (tenant == user at
this tier) rides every telemetry row/span/log, stamped from `current_identity()` at the emit funnel
(never a model payload). **C27 — ClickHouse RLS** now has its mechanism: a least-privilege
`agent_reader` credential + a `tenant_iso_*` `ROW POLICY` per tenant-bearing table
(`USING user = getSetting('SQL_tenant_id')`, nix/clickhouse/{schema.sql,users.xml}), and the
pure-read fleet-history seam binds `SQL_tenant_id` from the verified identity per connection
(`[telemetry] reader_user`; empty ⇒ Tier-0 writer credential, RLS off). C27-2 makes `user` the
**leading `ORDER BY` key** on the telemetry tables so the policy predicate prunes other tenants at the
primary index (security = performance; a table rebuild, guided in schema.sql). Enforcement is
live-verified (the hermetic gate has no ClickHouse). **C28 — shared-store / read-tool scoping** is
**complete**: the `metrics` tool scopes to the caller's `(session, user)` series + shared label-less
seam-health families (C28-1); the sqlite prompt store partitions per tenant by path under
`[tenancy] per_tenant` (C28-2); cross-session **recall** moved from per-tenant tantivy to reading
`agent_events` through the C27 RLS boundary — content redacted at the sink + a `tokenbf_v1` index
(C28-3a), and a `ClickHouseRecall` `SearchBackend` selected by `[recall] backend` (C28-3c) that derives
session titles in the query (the `agent_sessions` dim table is deferred); and the non-fleet
**code-index** is path-partitioned per tenant (C28-3d) — under `[tenancy] per_tenant` the `tantivy`
`search`/`structural_search` backend is wrapped in `PerTenant<dyn SearchBackend>`, each verified tenant
getting its own index at `…/index/tenants/<tenant>/tantivy` (the `local` tenant keeps the base path,
Tier-0 byte-identical), built lazily + warmed in the background and failing **closed** to an empty
index if a tenant's own index can't be opened (the tantivy recall corpus stays the Tier-0/offline
fallback; ClickHouse recall is the tenant-scoped path). Planes 02/03 otherwise remain
**designed, build deferred** — except **C30**,
which the config track built across two
increments: `PerTenant<S>` (`crates/agent-runtime/src/tenant.rs`) routes the converged shared-store
control-plane seams (provider-registry, review-fleet, prompt) per verified tenant (config C2), and — in
config C2b — the file-backed cognition graph, isolated per tenant by path (`tenants/<t>/…`). Both are
gated by `[tenancy] per_tenant` (default off = Tier-0). The **scheduler** is the one seam C30 does *not*
cover: it is process-bound (a job's executor is the owning process), so per-tenant scheduling is a
backend+driver change, designed in `docs/design/config/10-per-tenant-scheduler.md` and blocked in part on
plane-01 (per-tenant executor). This is the one implementation of C30 — the config track owns it; C29
(tenant-aware bootstrap) and C31 (control-plane operator-vs-tenant split, = config C40/E1) are still ⬜.
Tier 0 (single operator, one config) is today's behavior and needs nothing.

**Build order:** 01 (chokepoint C24 → backends C23) · 02 (identity-at-source C26 → RLS C27/C28)
· 03 (`PerTenant<Store>` C30 → control-plane scoping C31). C26 is cheapest and unblocks 02 +
fleet observability — land it early (during the fleet's Phase 1).

## Origin & decisions

- **2026-09-05** — Graduated into its own track from the review-fleet design (was review-fleet
  increments 9/10/11). Decision: 9/10/11 are system-wide (they make the whole agent
  multi-tenant), not fleet-local, so they live here; the fleet is the first consumer.
- **Tenant = org**, mapped onto `SessionKey.user`; single-level (no org→team→user in v1).
- **Structural enforcement**, bound to verified ambient identity — never a model-supplied value.
- **Tiered/pluggable** via one tier switch; Tier 0 = today's single-tenant behavior.
- **Split config by ownership** (operator-global `agent.toml` vs per-tenant data in
  registries/stores); **no per-tenant TOML**.
- Reuse `PerUserMemory`'s pattern (`PerTenant<Store>`, no trait change) and the already-shaped
  `Sandbox` seam.

## Dependencies

- On **multi-session**: `SessionKey`/`safe_segment`/`AGENT_IDENTITY`, `PerUserMemory` pattern,
  UDS-per-user, and the **auth follow-up** (deriving tenant from a verified token, not a
  transport label) — every boundary here is only as strong as that.
- On **review-fleet**: inc 1 (per-session workspace) auto-closes the code-index leak for the
  fleet; inc 6 (tenant-tagged review tables) is foundation for plane 02.

## Non-goals

Per-tenant `agent.toml` (operator config stays global); a third tenancy tier; auto/learned
policy; and the downstream trace-UI RBAC (we guarantee a trustworthy `tenant` attribute; wiring
HyperDX/ClickStack RLS is deployment work).
