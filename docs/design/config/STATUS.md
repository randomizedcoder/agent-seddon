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
one card builder. **Track D is complete: D2 (#307)** landed C37 — the messaging twin of D1: a
**bidirectional** `MessageTransport` seam (adding an outbound `post` half) with neutral message types, a
`TransportCard` + `TransportRegistryService` seam + in-crate `StoreTransports`, and `agent-slack` recast
as one impl (`SlackMessageTransport` posts via `chat.postMessage`; the live Socket-Mode adapter carries
inbound) selected by `kind` at build time (unknown kind fails closed, endpoint SSRF-screened), plus the
pure `RateLimiter` + soft-fail `announce` primitives. As in D1, lifting the `slack_*`/`FleetSlackCfg`
fields **out** of `FleetSession` into a card-by-id is deferred (D2b). **Track E — the last phase — is now
under way: E1 (this PR)** lands C40, the control-plane consolidation: the **operator-global vs tenant
write split** (C29) rides inside `authorize` — a mutating write to an operator-global resource
(`ResourceType::is_operator_global`, the bootstrap `Config` surface behind `ConfigService`) is granted
**only to a host-global role**, so a tenant `org_admin` is denied even in its own tenant, while every
tenant-owned card surface is unaffected; and the two Track-D card registries (**forge** C36/D1 +
**transport** C37/D2) are brought onto the same `PerTenant` routing the A3*/C2b seams already use, so
`Get/List/Put/Delete` scope to the caller's verified tenant on every CRUD service (proven in the hermetic
gate over the file/memory tier). The portal admin surface (tenants/roles/forges/transports) remains a
noted future. **With E1, the config-architecture build is complete** — only the explicitly-deferred tails
remain. **D1b is now complete** across three PRs: the **gitea**
([#309](https://github.com/randomizedcoder/agent-seddon/pull/309)) and **bitbucket**
([#310](https://github.com/randomizedcoder/agent-seddon/pull/310)) host impls (proving the C36 "add a host =
a new impl + a factory line" recipe, `04-forge-registry.md`), and **card-by-id**
([#311](https://github.com/randomizedcoder/agent-seddon/pull/311) — an additive
`FleetSession.forge_id` referencing a persisted `ForgeCard`, with the `ForgeRegistry` threaded into all
three fleet build paths). **D2b is now under way** (the transport twin of D1b, a 3-PR split): the
**matrix** host impl ([#312](https://github.com/randomizedcoder/agent-seddon/pull/312) — an opt-in
second `MessageTransport` proving the C37 "add a host = a new impl + a factory line" recipe,
`05-message-transport.md`) landed first, then **card-by-id + Socket-Mode inbound unification**
([#313](https://github.com/randomizedcoder/agent-seddon/pull/313) — an additive
`FleetSession.transport_id` referencing a persisted `TransportCard`; the fleet's Slack watch now runs one
Socket-Mode connection **per resolved app token**, a card row taking its token + `trigger`-purpose
channels from the card, a legacy row keeping the inline `slack_*` + `[review_fleet.slack]` default). The
last D2b PR, the **C18 progress feed** as a live `announce()` caller
([#314](https://github.com/randomizedcoder/agent-seddon/pull/314) — a `FleetProgress` seam the orchestrator
and the approver post lifecycle beats through: `reviewing` + `drafted` from the FSM's per-review task,
`posted` from the approve path, each rendered to the card's `progress`-purpose channels via
`build_transport_from_card` + `announce`; announce-only and soft-fail, so a broken channel never blocks a
review), **completes Track D**. teams/irc/signal host impls and a live Matrix `/sync` inbound are noted
further-deferred.

## Components

| C# | Component | State | Notes |
|---|---|---|---|
| C32 | Config-card pattern (meta) | ⬜ | Convention; formalizes the shipped `ProviderRegistryService` shape. |
| C33 | Authentication interceptor (OIDC/JWT) | ✅ | Auth tower layer (B1 #295). Concretizes multi-session 07-security. No proto change. |
| C34 | RBAC model | ✅ | `authorize` + role cards (C1 #297, C1b #301); gates control-plane RPCs (not the tool `Policy`). E1 adds the operator/tenant write split. |
| C35 | Per-tenant config plane | ✅ | `PerTenant<Store>` (C2 #302); = multi-tenancy C30 applied to config stores. E1 extends it to forge/transport. |
| C36 | Forge registry | 🟡 | Forge cards + `ForgeRegistryService` seam (D1); allow-list dropped, kind/base-url/repo-encoding lifted into the card and resolved at build time. D1b complete: gitea #309, bitbucket #310, card-by-id (`FleetSession.forge_id`) #311. |
| C37 | Message-transport registry | 🟡 | Bidirectional `MessageTransport` (recv + new outbound `post`) + `TransportRegistryService` seam (D2); Slack = one impl. D2b complete: matrix host impl #312; `FleetSession.transport_id` card-by-id + Socket-Mode inbound unification #313; C18 progress feed (`FleetProgress` announce caller) #314. |
| C38 | Per-tenant prompt storage | ✅ | `StorePrompt::with_tenant` + `PerTenant` wrap (C2 #302); no trait change. |
| C39 | LLM upstream/pool config | 🟡 | Reference impl **shipped** (model-router); only convergence onto C41/C35 pending. |
| C40 | Control-plane consolidation | 🟡 | Operator/tenant write split + per-tenant forge/transport (E1, [#308](https://github.com/randomizedcoder/agent-seddon/pull/308)); composes C33/C34/C35 over all control services; = multi-tenancy C31. |
| C41 | Transactional config data layer | ✅ | **Keystone.** `agent-config-store`: Postgres/sqlite/file behind one store (A1 #294, A2 #296, A3* #298–#300); atomic multi-card txns. |

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
| C1 | C34 RBAC enforcement core (`authorize` + gate all control-plane RPCs) | ✅ | [#297](https://github.com/randomizedcoder/agent-seddon/pull/297) | B1, A1 |
| C1b | RBAC role cards + `RoleService` seam (needs the shared store) | ✅ | [#301](https://github.com/randomizedcoder/agent-seddon/pull/301) | C1, A3 |
| C2 | C35 per-tenant plane (+ C38 prompt) | ✅ | [#302](https://github.com/randomizedcoder/agent-seddon/pull/302) | B1, A3 |
| C2b | Per-tenant **Graph** (file path-namespaced) | ✅ | [#303](https://github.com/randomizedcoder/agent-seddon/pull/303) | C2 |
| C2c-1 | Durable **Scheduler** foundation (`StoreScheduler` + `Backend::tenants`) | ✅ | [#304](https://github.com/randomizedcoder/agent-seddon/pull/304) | A3 |
| C2c-2 | Per-tenant Scheduler **driver + serve** (tenant-fanning) | ✅ | [#305](https://github.com/randomizedcoder/agent-seddon/pull/305) | C2c-1, plane-01 |
| D1 | C36 forge registry | ✅ | [#306](https://github.com/randomizedcoder/agent-seddon/pull/306) | A1 (+C2 per-tenant) |
| D2 | C37 message-transport registry | ✅ | [#307](https://github.com/randomizedcoder/agent-seddon/pull/307) | A1 (+C2 per-tenant) |
| E1 | C40 control-plane consolidation | 🟡 | [#308](https://github.com/randomizedcoder/agent-seddon/pull/308) | B1, C1, C2 |

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

## Follow-on: Postgres as a first-class, production backend (PG-01 … PG-11)

C41 shipped the Postgres *tier* (opt-in, dev-only nix module). A follow-on track hardens it into the
first-class, production-default backend for scaled multi-tenant deployments: a versioned migration
runner, a schema/index fix for the `pos` write-serialization bottleneck, a production config profile +
first-class build features, a NixOS-native `services.postgresql` module exposed from the flake, and
extended coverage (a Postgres digest ledger + a durable post-lease over the shared store's
compare-and-swap, retiring the legacy `*-sqlite` impls). Eleven gated PRs off `main`.

- **PG-01 (this increment) — versioned migration runner.** `agent-config-store`'s Postgres tier moves
  from a single idempotent `raw_sql(0001)` applied on every connect to a small **versioned runner**
  (`PgBackend::run_migrations`): the embedded `migrations/*.sql` set is applied exactly once, in order,
  recorded in a `_schema_migrations` ledger — so a later non-idempotent `ALTER` is safe — with the whole
  run serialized by a transaction-scoped advisory lock so concurrent starters can't race a step. The
  SQLite tier stamps `PRAGMA user_version` (baseline 1) as its version anchor. Behavior-preserving;
  existing deployed DBs re-record `0001` (inert `CREATE TABLE IF NOT EXISTS`) on first upgrade.
  - **Security note (deviation from the plan's `sqlx::migrate!` choice):** `sqlx`'s `macros` feature —
    which the `migrate!` macro needs — pulls in *every* driver crate (`sqlx-mysql`, `sqlx-sqlite`)
    regardless of the one in use, and `sqlx-mysql` drags in `rsa`, which carries the unfixable
    timing-sidechannel advisory **RUSTSEC-2023-0071** that `cargo audit` (a gate check) rejects. Rather
    than depend on — and then have to suppress an advisory for — a MySQL driver we never speak, the
    hand-rolled runner over the base `sqlx` API gives the same exactly-once/versioned guarantee while
    keeping the dependency graph to the Postgres driver alone (no lockfile churn, `cargo audit` clean).

- **PG-02 — schema + index + `pos` write-path fix (migration `0002`).** The Postgres `pos` moves from a
  global `(SELECT MAX(pos) + 1 FROM cards)` subquery on every Put/CAS — a full-table aggregate that
  serializes writers — to a `GENERATED BY DEFAULT AS IDENTITY` column (the runner seeds its sequence past
  the highest existing `pos`, so v1 rows never collide), and a composite `cards_list_idx (collection,
  tenant, pos)` backs the per-tenant `list ... ORDER BY pos` as an index range scan. The Put/CAS INSERTs
  drop the subquery and omit `pos` so the identity default fills it. SQLite mirrors the index, keeps its
  `MAX(pos)+1` (single-writer under a `Mutex`, no contention), and bumps `PRAGMA user_version` to 2. A
  hermetic schema-mirror test (D7) asserts the SQLite index exists live *and* the Postgres `0002` text
  creates the same-named index, so the two tiers can't silently diverge. New ordering scenarios
  (insertion order, update keeps position, re-insert after delete gets a new slot, hostile fields don't
  perturb order) run across every backend tier.

- **PG-03 — production profile + first-class default features (D1/D2).** The shipped `agent` binary is
  Postgres-capable out of the box: `agent-runtime` gains a `postgres` umbrella feature (the seven
  `*-postgres` shared-store arms), and `agent-cli`'s `default` turns it on (decision **D1 Option B** — the
  binary is batteries-included while a library consumer of `agent-runtime` stays lean and links no `sqlx`
  unless it opts in). The compiled `ConfigStoreCfg::default()` stays `""` (decision **D2**), so a minimal
  or `file`-backed config still opens no DB — "first-class Postgres" is the shipped *profile*, not the
  compiled default. New `config/multi-tenant.toml` runs the WHOLE OLTP control plane (provider registry,
  review-fleet roster, prompts, RBAC roles, scheduler, forge/transport cards) on one Postgres with
  per-tenant isolation; secrets stay references (`dsn_ref = "env:…"`, `api_key_env`). The
  `config-roundtrip` gate check gains a Postgres-profile fixture that mirrors it and must resolve every
  domain's `store = "postgres"` through the real factory chain (`pg_backend` connects lazily, so
  `--check-config` stays hermetic — no server dialed), plus an adversarial twin proving an INLINE DSN in
  the profile is rejected at build. `[role]` is omitted from the hermetic fixture because its RBAC catalog
  loads eagerly (needs a live server); the full profile including role is validated by
  `nix run .#integration`. Fail-closed-without-feature is a compile-time property (each resolver keeps its
  `#[cfg(not(feature = "…-postgres"))] "postgres" => bail!` arm), reachable via a `--no-default-features`
  build, outside the default-feature hermetic gate.

- **PG-04 — hardened container module (persistence, tuning, creds, logs) + host `psql`.** The
  `nix/postgres` container apps go from a throwaway dev toy to a production-shaped local/CI server: a
  PERSISTENT named data volume (`postgresDataVolume`) so `postgres-down` (container-only removal) no
  longer discards the database; server tuning applied as startup flags (`shared_buffers`,
  `max_connections`, `work_mem`, `effective_cache_size` — all pinned in `nix/versions.nix`, mirrored by
  the PG-05 NixOS module later); a run-time password (`$AGENT_PG_PASSWORD`, dev default otherwise) instead
  of a baked-in constant; and a new `postgres-logs` follower (from the shared container-app factory). Host
  `psql` (pinned `postgresql_16`) joins the dev shell, with `pg-up/pg-down/pg-client/pg-logs` helpers and
  an `agent-help` Postgres section. `pg-integration` is unaffected — it resets tables per test and removes
  the container on cleanup, so the persistent volume doesn't leak between runs. The container's role
  password is fixed at first volume init (standard postgres-image behaviour), documented alongside the
  volume knob.

## Non-goals

- Removing TOML (bootstrap stays TOML).
- A concrete IdP product choice (beyond "OIDC/JWT bearer").
- A third identity tier (`org → team → user`) in v1 — noted extension only.
- Using ClickHouse for config.
- Re-specifying multi-tenancy C29–C31 (referenced, not duplicated).
