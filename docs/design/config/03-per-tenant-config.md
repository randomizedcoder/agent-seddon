# 03 — Per-tenant config plane (C35)

How 10–50 orgs each get isolated config, and how operator-global vs tenant-owned keys are split. This
**coordinates with and subsumes** multi-tenancy C29–C31 — that track owns the enforcement mechanics; we
reference them and apply them to the config-card surface. Nothing here changes a wrapped store's trait.

## The tenant model (inherited, not reinvented)

- **Tenant = organization = `SessionKey.user`** (`crates/agent-core/src/identity.rs:176`). `session` is
  a sub-scope; the hierarchy is `host ⊃ org(tenant) ⊃ team? ⊃ user`, **single-level in v1** (`team`/`user`
  as a third tier is the noted extension, `identity.rs:179`).
- The tenant is **derived from the verified token** (C33), carried in the `AGENT_IDENTITY` task-local
  (`identity.rs:240`), read via `current_identity()` (`identity.rs:251`). Never a model- or
  client-supplied value.

## The reference wrapper: `PerUserMemory`

The pattern already exists for one seam — `crates/agent-memory/src/tenant.rs:57` `PerUserMemory`:
- resolves `current_identity().user` on **each call** (`current_user()`, `tenant.rs:47`),
- lazily builds + caches a per-user store rooted at `<base>/<user>/…` (`tenant_path`, `tenant.rs:33`),
- routes the call into it,
- maps the `local` user to the un-namespaced base, so single-tenant deployments are unchanged.

C35 **generalizes this into `PerTenant<Store>`** (multi-tenancy C30) and applies it across the config
seams.

## `PerTenant<Store>` — the generalization

```rust
// conceptual; multi-tenancy C30. No change to the wrapped trait.
pub struct PerTenant<S> {
    base: /* how to build one tenant's store */,
    cache: Mutex<HashMap<TenantId, Arc<S>>>,
}
impl<S: Store> PerTenant<S> {
    fn route(&self) -> Arc<S> {
        let tenant = current_identity().map(|k| k.user).unwrap_or(LOCAL);
        // lazily build + cache the per-tenant store view, then delegate
    }
}
```

Wrapped seams (each already has file/sqlite/grpc backends — C41 adds postgres, C35 adds the wrap):

| Seam | Trait anchor | Per-tenant meaning |
|---|---|---|
| `ProviderRegistry` | `crates/agent-core/src/lib.rs:2808` | each org's own LLM upstreams/routing |
| `PromptStore` | `crates/agent-core/src/lib.rs:2302` | each org's own prompt DB (C38) |
| `FleetRegistry` | `crates/agent-core/src/lib.rs:2952` | each org's own review roster |
| `GraphStore` | `[graph]` / `GraphService` | each org's own cognition graphs |
| `Scheduler` | `[scheduler]` / `SchedulerService` | each org's own scheduled jobs |
| messaging | C37 | each org's own transport cards |

Wrapping is **applied only when a per-tenant tier is enabled** (bootstrap flag); otherwise the single
global store is used unchanged (Tier-0 = today).

## The operator-global vs tenant split (C29)

Not everything is tenant-editable. Following multi-tenancy C29:

- **Operator-global** (bootstrap TOML, host-owned): ports/wiring, the store `backend`/DSN/credentials,
  auth issuer, telemetry endpoints, sandbox/isolation tier. A **tenant write to an operator-global key
  is rejected at the control plane** (C31/C40) — there is no per-tenant TOML (multi-tenancy non-goal).
- **Tenant-owned** (cards on the shared store, per-org): upstreams, forges, transports, prompts, the
  roster, roles/permissions, graphs, jobs.

The control plane (C40) enforces the split: `ConfigService` refuses tenant edits to operator keys;
the card services scope `Get/List/Put/Delete` to the caller's tenant.

> **Status: ✅ built (E1).** The split lives in `agent_core::authorize`: `ResourceType::is_operator_global()`
> is `true` only for the bootstrap `Config` surface, and a mutating write to an operator-global key is
> granted **only to a host-global role** (`RoleDef::crosses_tenants`) — so a tenant `org_admin` is denied
> `ConfigService::put` even in its own tenant (`negative_tenant_write_to_operator_key_denied`), while every
> tenant-owned card registry (registry/fleet/prompt/graph/scheduler/role/**forge**/**transport**) is
> unaffected and scoped to the caller's verified tenant via `PerTenant`. E1 brought the last two
> single-tenant registries — forge (D1) and transport (D2) — onto that routing.

## Isolation guarantees + threat model

- **Structural, not advisory.** A per-tenant store view is rooted/scoped by the verified tenant; a
  request can only address rows within its own subtree, so cross-tenant read/write is *unrepresentable*,
  not merely denied.
- **Bound to verified identity.** The routing key is `current_identity().user` from C33's verified
  token — never a header, never a model value. Before C33, isolation is only as strong as the transport;
  **C33 is the prerequisite** (stated in `../multi-tenancy/README.md`).
- **`safe_segment` on every id** that becomes a path/row key (`identity.rs`), so a hostile tenant id
  can't traverse (`adversarial_hostile_tenant_id_confined`).
- **`local` is the single-tenant escape hatch** — un-namespaced base, so nothing changes for a
  one-org/dev deployment.
- Data-plane isolation (ClickHouse RLS, per-tenant search indexes) is multi-tenancy plane 02 (C26–C28),
  **out of scope here** — this doc is the *config* plane only.

## Relationship to multi-tenancy C29–C31 (no duplication)

| This design | Owned by | Relationship |
|---|---|---|
| C35 `PerTenant<Store>` | multi-tenancy **C30** | C35 = C30 applied to the config-card stores; we don't re-specify the wrapper |
| operator-global vs tenant split | multi-tenancy **C29** | we adopt C29's rule; state which config keys fall where |
| tenant-scoped control plane | multi-tenancy **C31** | C40 is C31 applied to `ConfigService` + the card services |

If C30/C31 land first in the multi-tenancy track, C35/C40 are their config-surface application; if this
track lands first, it *is* the C30/C31 implementation for config. Either way, one implementation — the
`STATUS.md` dependency notes track which track builds it.

## C35 test matrix

| Class | Case | Expect |
|---|---|---|
| positive | `positive_two_tenants_isolated_stores` | tenant A `Put` invisible to tenant B `Get` |
| positive | `positive_cached_store_reused` | second call same tenant → reuses the built view |
| negative | `negative_tenant_cannot_read_other` | A reading B's card ref → not found (unrepresentable) |
| negative | `negative_tenant_write_to_operator_key_denied` | tenant editing a bootstrap key → denied (C40) |
| boundary | `boundary_local_tenant_uses_base_path` | `local` → un-namespaced base, single-tenant unchanged |
| corner | `corner_first_write_creates_tenant_view` | first write for a new tenant lazily creates its store |
| corner | `corner_no_identity_defaults_local` | no ambient identity → `local` view (dev) |
| adversarial | `adversarial_hostile_tenant_id_confined` | tenant id with `..`/`/` → `safe_segment` reject, no escape |
| adversarial | `adversarial_identity_from_token_not_header` | routing key is the verified tenant (C33), not a header |

Integration: the **per-tenant isolation** wire test (tenant A cannot read/write tenant B over gRPC) in
[`08-testing-and-integration.md`](08-testing-and-integration.md).
