# Status — unified configuration architecture

Legend: ⬜ designed, not built · 🟡 partially built · ✅ built + merged.

**Track state: 🟡 building.** The design-of-record was written 2026-09-08; the phased build is now under
way (see the live tracker below). Both keystones are merged — A1 `agent-config-store` (#294) and B1 the
auth tower layer (#295), A2 (the Postgres tier, #296), C1 (the RBAC enforcement core, #297), A3
(`agent-registry` onto the shared store, #298), A3b (`agent-review-fleet`, #299), and A3c
(`agent-prompt`, the outlier, #300) — **the whole store convergence is now merged**, C1b (RBAC role
cards + the `RoleService` seam, #301), C2 (the per-tenant config plane, #302), and C2b (per-tenant
routing for the file-backed cognition graph, #303) with it. The scheduler half of C2b turned out
**not** to be a `PerTenant` wrap (it is process-bound — a job's executor is the owning process), so it
was split into its own design-of-record, [`10-per-tenant-scheduler.md`](10-per-tenant-scheduler.md)
(durable tenant-keyed backend + tenant-fanning driver). **C2c shipped in two PRs:** C2c-1 landed the
durable **foundation** — a `StoreScheduler` over the shared store (the durable twin of `LocalScheduler`)
plus `Backend::tenants` (the driver's tenant-discovery primitive) — as library + tests, deliberately not
selectable in config so nothing could silently no-op; **C2c-2 (this PR)** wires it up: the
`[scheduler] store` config arm, `resolve_scheduler`, the tenant-fanning driver (`StoreDriver`), the
per-tenant served registry (`impl Scheduler for PerTenant<dyn Scheduler>`), and identity-scoped firing so
each tenant's jobs run as that tenant. **Track D is now under way: D1 (this PR)** lands C36 — a `ForgeCard`
+ the `ForgeRegistryService` seam + an in-crate store, dropping the hardcoded `""|github|gitlab` allow-list
so the valid kinds are "whatever forge impls are built in" (an unknown kind now fails closed at build time,
listing the known kinds), and lifting the per-kind default `base_url` and per-host `repo_encoding` onto the
card. Both forge build paths (the in-loop `[forge]` factory and `build_session_forge`) now route through
one card builder. Only **D2** (C37 transport registry) and **E1** (C40 control-plane consolidation) remain.

## Components

| C# | Component | State | Notes |
|---|---|---|---|
| C32 | Config-card pattern (meta) | ⬜ | Convention; formalizes the shipped `ProviderRegistryService` shape. |
| C33 | Authentication interceptor (OIDC/JWT) | ⬜ | **Keystone.** Concretizes multi-session 07-security. No proto change. |
| C34 | RBAC model | ⬜ | Roles/permissions as cards; gates control-plane RPCs (not the tool `Policy`). |
| C35 | Per-tenant config plane | ⬜ | = multi-tenancy C30 applied to config stores. |
| C36 | Forge registry | 🟡 | Forge cards + `ForgeRegistryService` seam (D1); allow-list dropped, kind/base-url/repo-encoding lifted into the card and resolved at build time. |
| C37 | Message-transport registry | ⬜ | Bidirectional `MessageTransport`; Slack = one impl. |
| C38 | Per-tenant prompt storage | ⬜ | `PerTenant` wrap of existing `PromptStore`; no trait change. |
| C39 | LLM upstream/pool config | 🟡 | Reference impl **shipped** (model-router); only convergence onto C41/C35 pending. |
| C40 | Control-plane consolidation | ⬜ | Composes C33/C34/C35 over all control services; = multi-tenancy C31. |
| C41 | Transactional config data layer | ⬜ | **Keystone.** Postgres/sqlite/file behind one store; atomic multi-card txns. |

## Proposed increment ordering

Two keystones with no dependency on each other, then the layers that need them:

1. **C41 — transactional store** (postgres tier + shared `agent-config-store` + migrations). Existing
   single-tenant installs keep `file`/`sqlite`. Converge `agent-registry`/`agent-prompt`/
   `agent-review-fleet` onto it (behavior-preserving).
2. **C33 — auth interceptor** (OIDC/JWT; `mode=none` preserves today). Independent of C41.
3. **C34 — RBAC** (needs C33's verified roles + the resource model).
4. **C35 — per-tenant** (needs C33's verified tenant; wraps the C41 stores).
5. **C36 / C37 / C38** — forge, transport, per-tenant prompt cards (need C41; per-tenant needs C35).
6. **C40 — control-plane consolidation** (composes C33/C34/C35 over every service).

Each is a **gated PR off `main`, never stacked** (the established cadence); new card protos are additive
(no `buf.image.binpb` bump).

### Executable increments (phase-by-phase)

The full build sequence — eleven gated PRs, each with scope, anchored key files, tests, and a definition
of done — is [`09-increments.md`](09-increments.md). This table is the **live tracker**: **State** moves
⬜ not started → 🟡 in review → ✅ merged, and **PR** carries the number as each phase opens.

| Phase | Component | State | PR | Depends on |
|---|---|---|---|---|
| A1 | `agent-config-store` crate: file+sqlite tiers + txn API | ✅ | [#294](https://github.com/randomizedcoder/agent-seddon/pull/294) | — |
| A2 | Postgres tier + `[config_store]` bootstrap + opt-in DB harness | ✅ | [#296](https://github.com/randomizedcoder/agent-seddon/pull/296) | A1 |
| A3 | Converge `agent-registry` (behavior-preserving) | ✅ | [#298](https://github.com/randomizedcoder/agent-seddon/pull/298) | A2 |
| A3b | Converge `agent-review-fleet` | ✅ | [#299](https://github.com/randomizedcoder/agent-seddon/pull/299) | A2 |
| A3c | Converge `agent-prompt` (outlier) | ✅ | [#300](https://github.com/randomizedcoder/agent-seddon/pull/300) | A2 |
| B1 | `AuthInterceptor` tower layer + JWKS/JWT + `[auth]` | ✅ | [#295](https://github.com/randomizedcoder/agent-seddon/pull/295) | — |
| C1 | C34 RBAC enforcement core (`authorize` + gate all control-plane RPCs) | 🟡 | [#297](https://github.com/randomizedcoder/agent-seddon/pull/297) | B1, A1 |
| C1b | RBAC role cards + `RoleService` seam (needs the shared store) | ✅ | [#301](https://github.com/randomizedcoder/agent-seddon/pull/301) | C1, A3 |
| C2 | C35 per-tenant plane (+ C38 prompt) | ✅ | [#302](https://github.com/randomizedcoder/agent-seddon/pull/302) | B1, A3 |
| C2b | Per-tenant **Graph** (file path-namespaced) | ✅ | [#303](https://github.com/randomizedcoder/agent-seddon/pull/303) | C2 |
| C2c-1 | Durable **Scheduler** foundation (`StoreScheduler` + `Backend::tenants`) | ✅ | [#304](https://github.com/randomizedcoder/agent-seddon/pull/304) | A3 |
| C2c-2 | Per-tenant Scheduler **driver + serve** (tenant-fanning) | ✅ | [#305](https://github.com/randomizedcoder/agent-seddon/pull/305) | C2c-1, plane-01 |
| D1 | C36 forge registry | 🟡 | [#306](https://github.com/randomizedcoder/agent-seddon/pull/306) | A1 (+C2 per-tenant) |
| D2 | C37 message-transport registry | ⬜ | — | A1 (+C2 per-tenant) |
| E1 | C40 control-plane consolidation | ⬜ | — | B1, C1, C2 |

## Dependencies (cross-track)

- **multi-tenancy C29–C31** — C35/C40 are the config-surface application of C30 (`PerTenant<Store>`) and
  C31 (tenant-scoped control plane), and adopt C29's operator-global-vs-tenant split. **One
  implementation**; whichever track builds it first, the other references it. See
  [`../multi-tenancy/`](../multi-tenancy/README.md).
- **multi-session 07-security** — C33 **is** that follow-up (verified identity replacing the trusted
  header). See [`../multi-session/07-security.md`](../multi-session/07-security.md).
- **model-router** — C39/C32 reference its shipped registry as the exemplar. See
  [`../model-router/`](../model-router/README.md).
- **review-fleet** — C36/C37 generalize the forge + Slack config it uses; C37 is the seam its C18
  progress feed posts through. See [`../review-fleet/`](../review-fleet/README.md).

## Decisions of record (2026-09-08)

1. **Two-tier model** — operator-global bootstrap stays TOML; domain + per-tenant config → protobuf
   cards. No TOML rip-out.
2. **Design docs only** this pass.
3. **Fully specify now:** auth+RBAC, per-tenant config, multi-forge, generic messaging. (LLM/pool +
   per-tenant prompt storage sketched lighter — they largely exist.)
4. **Auth = OIDC/JWT bearer** — interceptor verifies the token, derives tenant+roles, ignores the client
   header.
5. **Config storage = transactional SQL (OLTP), separate from telemetry.** file → sqlite → **postgres**
   behind one store trait; **ClickHouse never used for config**; atomic multi-card transactions required.
6. **Topology = per-domain typed services over one shared SQL store** — no generic mega-service.

**Refined at implementation-planning time (2026-09-08, grounded against the tree — see
[`09-increments.md`](09-increments.md)):**

7. **Two SQL code paths, one trait.** Today's SQLite is `rusqlite` (bundled); there is no `sqlx`. Keep
   `rusqlite` for the `file`/`sqlite` tiers (untouched, still hermetic in-gate) and add `postgres` as a
   new `sqlx` tier. This **supersedes decision #5's "sqlx one code path"** — it is behavior-preserving
   and keeps the hermetic gate intact.
8. **Postgres is opt-in, never in `nix flake check`.** A real DB server can't run in the check sandbox
   (no docker/network), so the `postgres` tier is exercised only via `nix run .#integration` (mirroring
   the ClickHouse harness); `file`+`sqlite` remain hermetic in-gate via bundled SQLite.
9. **Auth is a tower `Layer`, not a tonic interceptor** — no interceptors exist today; C33 stacks a
   second `Layer` beside `AdmissionLayer` and widens the `ServeRouter` alias.

## Non-goals

- Removing TOML (bootstrap stays TOML).
- A concrete IdP product choice (beyond "OIDC/JWT bearer").
- A third identity tier (`org → team → user`) in v1 — noted extension only.
- Using ClickHouse for config.
- Re-specifying multi-tenancy C29–C31 (referenced, not duplicated).
