# 03 — Config & per-seam state tenancy (multi-tenancy track; DESIGN + deferred build)

Multi-org isolation is not a property of the fleet's *data* alone — it must hold for **every
feature**: the LLM upstream configs, the routing / load-balancing policy, the cognition
graphs, the prompt/skill library, scheduler jobs — all of it. If org A can read or edit org
B's upstreams or routing, that is the same breach class as reading its review rows. This doc
makes tenancy a **system-wide property of every seam that holds config or state**.

It is the config/state counterpart to plane 01 (attacker *code*) and plane 02 (attacker
*reads*); together **planes 01 + 02 + 03 are the multi-tenancy platform** — system-wide,
extending the per-user tenancy the multi-session track began. The fleet is the forcing
function, not the owner. Status: designed, build deferred; Tier 0 (one operator, one
config) is today and needs nothing.

## The starting point (audit, grounded)

The system is **single-global-config, one `Arc<dyn Trait>` per seam per process**:

- One `Config` parsed from one `agent.toml` at startup (`agent-cli/src/main.rs:33`); no
  layering, no per-tenant file. `FactoryCtx` carries **no identity** (`registry.rs:34`); each
  seam is built exactly once (`builder.rs`).
- The **only** per-tenant seams are **memory** and **dimensional memory**, via
  `PerUserMemory`/`PerUserDimensions` — a single `Arc` that reads the ambient `AGENT_IDENTITY`
  task-local and routes to `<base>/<user>/…` (`agent-memory/src/tenant.rs`). **No trait change
  was needed** — the wrapper reads the task-local and lazily builds a per-user store
  (`store_for`).
- **Global today, must become per-tenant:** provider registry (upstreams + routing `Policy`,
  `agent-registry`, trait takes no identity), cognition graph (`GraphStore`, one document),
  prompt/skill library (`PromptStore`, one dir/db), scheduler (`Scheduler`, one in-memory job
  table), and the stateful bits of cache/reference.
- `GraphSvc`/`ConfigSvc` **already** wrap calls in `run_scoped(identity_key(...))`, but their
  stores ignore it — the scoping hook exists and is a no-op. `ProviderRegistryService` doesn't
  even scope.
- Only tenancy vocabulary is `(user, session)`; "tenant" == "user" (org, per the doc-09
  mapping). No auth — identity is a transport-trusted namespacing label (multi-session 07).

## Principle 1 — split config by ownership (don't fork `agent.toml`)

- **Operator config stays global** (`agent.toml`): listen ports, `[sandbox]` tier/backends,
  `[telemetry]` connection, `[grpc]`, feature enablement, resource caps, the fleet roster.
  The tenant never edits these.
- **Tenant config is data in per-tenant stores, not a per-tenant TOML**: LLM upstreams + keys
  and routing/LB policy → the **provider registry**; prompts/skills → the **prompt store**;
  cognition graphs → the **graph store**; scheduled jobs → the **scheduler**; memory →
  already per-user. Making those stores per-tenant makes the tenant-facing configuration
  per-tenant **without** a per-tenant `Config` struct or a per-tenant builder path.

This is the key simplification: the tenant-relevant config surface is *already* expressed as
registry/store contents; scope the stores, and routing/LB/prompts/graphs follow for free
(the registry-backed router reads the registry — per-tenant registry ⇒ per-tenant routing).

## Principle 2 — one mechanism: `PerTenant<Store>` internal routing

Generalize the proven `PerUserMemory` pattern into a reusable wrapper:

- `PerTenant<T>` implements the seam trait, reads `current_identity()`, and lazily
  builds+caches a **per-tenant backing instance** keyed by the verified tenant (mirroring
  `store_for`, `tenant.rs:81`). **No trait signature change** — the wrapper reads the ambient
  task-local, exactly as memory does.
- Backing partition per seam (reuse memory's "the path is the boundary"): provider registry →
  per-tenant sqlite file / textproto bundle; graph → `<graph_dir>/<tenant>/graph.textproto`;
  prompts → `<prompt_dir>/<tenant>/…` (with operator defaults as a read-through base, below);
  scheduler → a persisted per-tenant job table (also lands scheduler persistence, currently
  in-memory only).
- Applied at the **builder**: where a global `Arc<dyn ProviderRegistry>` / `GraphStore` /
  `PromptStore` / `Scheduler` is built today, wrap in `PerTenant<…>` when a per-tenant tier is
  configured (Tier 1/2); at Tier 0 (no fleet_root / single operator) it stays the single
  global instance — unchanged behavior.

### Operator defaults + tenant override (read-through)
For prompts/skills (and optionally routing), a tenant should inherit **operator-provided
defaults** and override only what it customizes. `PerTenant` resolves as **tenant layer ⊕
operator base**: a `get`/`select` checks the tenant partition, falling back to the operator
default set; a `put` writes only the tenant layer. (The default `code-review` skill ships
operator-side; a tenant can override it without copying the rest.)

## Principle 3 — control-plane services scope by verified identity

The gRPC services that edit tenant config must scope every op to the caller's tenant,
server-side — several already capture the identity and just need the store to honor it:

- `ProviderRegistryService` (50084), `GraphService`, `PromptService`, `ConfigService`
  (50085), `ReviewFleetService`: `Put/Get/Delete/List` operate only on the caller's tenant
  partition (via `PerTenant` + `run_scoped`). A tenant's `Put` can never write another
  tenant's upstreams/graph/prompts; `List`/`Get` never return another tenant's.
- `ConfigService` is special: it edits the **operator** `agent.toml`, so it must **reject
  tenant callers** for operator-scoped keys (and, if a per-tenant config UI is wanted later,
  route tenant edits to the per-tenant stores, not the TOML). At minimum: only the operator
  identity may write operator config.

## Principle 4 — structural enforcement (same as 9/10)

- Routing is by **verified ambient identity**, never a value the injectable model supplies;
  `safe_segment` on the tenant segment before it becomes a path/partition; a tenant cannot set
  another tenant's identity (transport trust, only as strong as the auth layer — the
  multi-session 07 follow-up).
- Per-tenant **secrets**: each tenant's registry holds its own `api_key_ref`/`token_ref`;
  resolution stays local in the synthesizer (never from registry payload,
  `registry_router.rs:20`), so a tenant's keys are never constructed for or visible to another.
- The router's **snapshot + provider cache** (`registry_router.rs:42`) must be **keyed by
  tenant** (today one global snapshot) so tenant A's fleet view can't serve tenant B.

## What stays global (operator-owned)

Sandbox tier/backends (plane 01), telemetry ClickHouse connection (plane 02 scopes the *rows*,
not the connection), listen ports, the fleet roster, feature enablement, and the stateless /
operator-infra seams (tokenizer, embed, LSP, web, AST) — these are host config, not tenant
data.

## Per-seam inventory → target

| Seam | Holds | Current | Target | Mechanism |
|---|---|---|---|---|
| Config (`agent.toml`) | everything | global | **operator-global** (unchanged) | ownership split |
| Provider registry | upstreams, keys, routing/LB policy | global | **per-tenant** | `PerTenant<ProviderRegistry>` + per-tenant snapshot/cache |
| Router | selection | global | per-tenant (follows registry) | keyed snapshot |
| Graph store | cognition graphs | global | **per-tenant** | `PerTenant<GraphStore>` + `<dir>/<tenant>/` |
| Prompt store / skills | prompts, skills | global | **per-tenant + operator defaults** | `PerTenant<PromptStore>` read-through |
| Scheduler | jobs | global, in-memory | **per-tenant + persisted** | per-tenant persisted table |
| Memory / dimensions | episodic/semantic/dims | **per-user ✓** | per-tenant (=org) ✓ | already `PerUser*` |
| Forge | token, backend cfg | global + per-session token (C5) | per-tenant | C5 + per-tenant backend cfg |
| Cache / reference | derived state | global | per-tenant where it holds tenant data | `PerTenant` as needed |
| Telemetry / search rows | data | global | per-tenant | plane 02 |
| Sandbox | isolation | operator tier | operator-global | plane 01 |

## Components (see also [`00-components.md`](00-components.md) plane 03)

- **C29 — config ownership model**: the operator-global vs per-tenant split; mark each config
  section's ownership; `ConfigService` rejects tenant writes to operator keys.
- **C30 — `PerTenant<Store>` wrapper**: generalize `PerUserMemory`'s internal-routing pattern;
  apply to provider registry, graph, prompts (read-through defaults), scheduler; builder wraps
  when a per-tenant tier is on.
- **C31 — tenant-scoped control plane**: services honor the caller identity end-to-end;
  per-tenant router snapshot/cache; per-tenant secret resolution.

## Build (deferred; ordered)

1. **C30 wrapper + backing partitions** — `PerTenant<T>`; per-tenant provider registry
   (+ router snapshot keyed by tenant), graph, prompts (read-through), scheduler
   (+ persistence). Reuse the registry `ops` core so validation/caps don't drift.
2. **C31 control-plane scoping** — `ProviderRegistryService`/`GraphService`/`PromptService`/
   `ReviewFleetService` scope to caller; `ConfigService` operator-only for operator keys.
3. **C29 ownership doc + guards** — annotate config sections; a test asserting no tenant
   caller can mutate an operator-scoped key or another tenant's partition.

Config: a `[tenancy]` block (operator) selecting Tier 0 (single global) vs per-tenant
resolution, reusing the doc-09 tier. Builder wraps seams in `PerTenant` only when on.

### Test matrix (adversarial mandatory)
- `positive_per_tenant_registry_isolates_upstreams` — tenant A's `list()` never shows B's.
- `adversarial_tenant_cannot_put_into_another_tenants_registry/graph/prompts`.
- `adversarial_tenant_a_api_key_never_resolved_for_tenant_b`.
- `positive_router_snapshot_is_per_tenant` — A's routing can't select B's upstream.
- `positive_prompt_read_through_falls_back_to_operator_default`.
- `positive_tenant_override_shadows_default_without_copying_the_rest`.
- `adversarial_tenant_cannot_write_operator_config_key` (ConfigService).
- `positive_scheduler_jobs_are_per_tenant_and_persist`.
- `boundary_tier0_single_operator_behaves_exactly_as_today` (no wrap when off).
- `adversarial_identity_from_transport_not_model` — a model-supplied tenant string can't
  redirect routing.

### Done when (deferred)
`nix flake check` green; with a per-tenant tier on, each tenant sees only its own upstreams /
routing / graphs / prompts / jobs; a tenant inherits operator prompt defaults but overrides in
isolation; the router serves each tenant its own fleet; no tenant can read/write another's
config or the operator's; Tier 0 is byte-for-byte today's behavior.

## Organizational note

Planes 01 (process isolation), 02 (data RLS), and 03 (config/state tenancy) are **system-wide**,
not fleet-local — they turn the agent into a multi-tenant platform and extend the multi-session
per-user tenancy. They were graduated (2026-09-05) from the review-fleet design (formerly its
increments 9/10/11) into this dedicated **multi-tenancy** track, with the fleet as their first
consumer; see [`README.md`](README.md) and [`../review-fleet/`](../review-fleet/).

## Non-goals / residual risk

Auth (a verified token → tenant, not a transport label) remains the multi-session 07
follow-up; all of this plane's routing is only as strong as that boundary. A full per-tenant
`agent.toml` (per-tenant sandbox tiers, ports) is explicitly **not** built — operator config
stays global; only the store-backed tenant surface is per-tenant. Per-tenant seam instances
add memory/handle cost (N tenants × M cached stores) — bounded by an idle-eviction cache like
`PerUserMemory`'s, sized with the fleet's `with_limits`.
