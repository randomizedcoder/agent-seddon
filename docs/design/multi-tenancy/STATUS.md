# Multi-tenancy — status

Living tracker for the multi-tenancy platform (process / data / config planes). Extends the
[multi-session](../multi-session/) per-user tenancy to every remaining seam. Surfaced by, and
first consumed by, the [review-fleet](../review-fleet/) track. `nix flake check` is the gate.

Legend: ⬜ not started · 🟡 in progress · ✅ merged.

## Planes / increments

| # | Plane | Components | State | PR |
|---|---|---|---|---|
| 01 | Process isolation & multi-org boundaries | C23, C24, C25 | 🟡 C24 ✅ + C25 foundation 🟡; C23 ⬜ | #273/#274/#275 (C24), #276 (C25) |
| 02 | Data scoping & row-level security | C26, C27, C28 | ⬜ design; Tier-0 today | — |
| 03 | Config & seam-state tenancy | C29, C30, C31 | 🟡 **C30 built** for the shared-store seams (config C2) + the file-backed graph (config C2b); C29/C31 ⬜, scheduler designed | #302 (config C2), C2b (graph) |

**Plane 01 in progress** (via the review-fleet track): **C24 — execution chokepoint** is fully
merged (every child process — `bash`, `rg`, the whole `git` funnel — funnels through the
`Sandbox` seam, with argv-mode/no-shell + env-scrub + a no-raw-`Command` guard). **C25 — org
tenancy tier** has its *foundation* now: the `user = <org>` convention, the `repo@pr` session-id
encoder, and the per-org cap / metric-label semantics (see `SessionKey` docs). Still deferred: the
org *value* injection at the fleet mint-site (fleet core, inc 3), and **C23** strong-isolation
backends (bwrap/oci — real FS/network/cgroup teeth), which C24's chokepoint unblocks. Planes 02/03
remain **designed, build deferred** — except **C30**, which the config track built across two
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
