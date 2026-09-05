# Multi-tenancy — status

Living tracker for the multi-tenancy platform (process / data / config planes). Extends the
[multi-session](../multi-session/) per-user tenancy to every remaining seam. Surfaced by, and
first consumed by, the [review-fleet](../review-fleet/) track. `nix flake check` is the gate.

Legend: ⬜ not started · 🟡 in progress · ✅ merged.

## Planes / increments

| # | Plane | Components | State | PR |
|---|---|---|---|---|
| 01 | Process isolation & multi-org boundaries | C23, C24, C25 | ⬜ design; Tier-0 today | — |
| 02 | Data scoping & row-level security | C26, C27, C28 | ⬜ design; Tier-0 today | — |
| 03 | Config & seam-state tenancy | C29, C30, C31 | ⬜ design; Tier-0 today | — |

All three are **designed, build deferred**. Tier 0 (single operator, one config) is today's
behavior and needs nothing. Build when a deployment crosses trust domains (multi-org).

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
