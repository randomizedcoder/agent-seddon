# 07 — Existing config + prompt storage + migration path (C38, C39)

The lighter components: what already exists and only needs convergence, and the staged, additive path
from today's config to the target. Nothing here is new mechanism — it's application of C32/C35/C41.

## C38 — Per-tenant prompt storage

The user's "different orgs store prompts in different databases" is C35 applied to an existing seam.

- **What exists.** `trait PromptStore` (`crates/agent-core/src/lib.rs:2302`) with `FilePromptStore` /
  `SqlitePromptStore` (`crates/agent-prompt/`) + a `GrpcPrompts` client, selected by `[prompts] backend`
  (`crates/agent-runtime/src/config.rs:2254`), built once in `crates/agent-runtime/src/builder.rs:959`.
- **The gap.** It's **process-global** — one `Arc<dyn PromptStore>` shared by all sessions. Unlike
  memory, there's no per-org routing.
- **The fix.** A `PerTenantPromptStore` wrapper (C35 / multi-tenancy C30 applied to `PromptStore`),
  resolving `current_identity().user` per call + a per-tenant connection card, on the shared C41 backend.
  **The trait needs no change** — `sqlite`/`postgres`/`grpc` already prove multiple DBs are
  constructible; only the per-tenant selection layer is missing.
- **Tests.** The C35 matrix instantiated for `PromptStore`: `positive_two_tenants_isolated_prompt_dbs`,
  `negative_tenant_cannot_read_other_prompts`, `adversarial_hostile_tenant_id_confined`.

## C39 — LLM upstream/pool config (already built — the exemplar)

This is the reference implementation of the whole design; it needs no new mechanism, only convergence.

- **What exists.** `Upstream` / `ModelRouterConfig` / `ProviderRegistryService`
  (`crates/agent-proto/proto/agent/v1/upstream.proto`); `LlmPoolService` + member/health messages
  (`llm_pool.proto`); file/sqlite/grpc stores (`crates/agent-registry/`); textproto file
  (`config/model-router/example.textproto`); live refresh (`RegistryRouter`, `[registry] refresh_secs`).
- **Convergence deltas (only):**
  1. Move its store onto the shared C41 backend (gain postgres + cross-card transactions).
  2. `PerTenant`-wrap it (C35) so each org has its own upstreams/routing.
  3. Align ingest-clamp/naming conventions with the C32 checklist (already ~conformant).
- **Tests.** Existing model-router tests stand; add the shared-store + per-tenant rows on convergence.

## The remaining config-card catalogue (sketch)

Other existing dynamic stores that are already cards and simply converge onto C41 + C35:

| Card | Service | Convergence |
|---|---|---|
| Cognition graphs | `GraphService` (`graph.proto`) | shared store + `PerTenant` |
| Scheduled jobs | `SchedulerService` (`scheduler.proto`) | shared store + `PerTenant` |
| Fleet roster | `ReviewFleetService` (`review_fleet.proto`) | shared store + `PerTenant`; forge/transport refs → C36/C37 |

## Migration path (staged, additive, no big-bang)

The two-tier rule ([`01-config-card-pattern.md`](01-config-card-pattern.md)) plus the C-number
dependencies ([`STATUS.md`](STATUS.md)) give a safe order. Every step is additive; **no TOML is
removed** and each existing config keeps working throughout.

1. **Land the keystones.** C41 (shared transactional store, postgres tier) and C33 (auth) — independent
   of each other, both prerequisites for tenancy. Existing single-tenant deployments run unchanged
   (`store = "file"`/`sqlite`, `auth mode = "none"`).
2. **Converge existing cards onto C41.** Point `agent-registry` / `agent-prompt` / `agent-review-fleet`
   at the shared store. Behavior-preserving; TOML sections become **seeds** for the operator-global
   default (as model-router already treats `[route]`).
3. **Add RBAC + per-tenant** (C34, C35) — wrap the stores, gate the control plane (C40). Enabled only
   when a per-tenant tier is switched on in bootstrap; Tier-0 (today) is untouched.
4. **New cards** (C36 forge, C37 transport) — each a new additive proto (no baseline bump), a new store
   table, and the corresponding TOML section demoted to an operator-global seed.
5. **Per-tenant everything** — as orgs need isolated forges/transports/prompts, the `PerTenant` wrap +
   per-tenant cards light up with no further schema churn.

At no point is there a flag day: bootstrap stays TOML, domain config becomes cards incrementally, and a
single-operator single-tenant install can ignore the whole tenancy apparatus.

## What stays TOML forever (recap)

Ports/wiring (`[grpc]` + nix `constants.rs`), the store `backend`/DSN/credentials (`[config_store]`),
auth issuer (`[auth]`), telemetry endpoints (`[telemetry]`/`[metrics]`), sandbox/isolation tier,
process working-dir. One host, one operator, needed before serve — never a card. (Full bucket map in
[`01-config-card-pattern.md`](01-config-card-pattern.md).)
