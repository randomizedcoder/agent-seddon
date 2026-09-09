# 09 — Implementation increments (C32–C41)

The design-of-record ([README](README.md), [00-components](00-components.md), 01–08) says *what* to build
and *why*. This doc is the **build sequence**: eleven gated PRs across five tracks, each with a **scope**,
**key files** (anchored to current code), **tests**, and an explicit **definition of done (DoD)**. It
supersedes the coarse ordering in [STATUS.md](STATUS.md) with executable increments; [IMPLEMENTATION.md](IMPLEMENTATION.md)
remains the cross-cutting touch/blast-radius map.

**Nothing here is built yet.** Each increment is a **PR off `main`, never stacked**, ending green under
`nix flake check --max-jobs 8 --cores 4`. New card protos are **additive → no `buf.image.binpb` bump**.
Every phase ships a table-driven suite (four classes + `adversarial_`, each row `desc`+`expect`,
`#[cfg(test)] mod` at file end) per [08-testing-and-integration.md](08-testing-and-integration.md).

## Decisions that shape these increments (grounded 2026-09-08)

Three facts, verified against the tree, refine the design sketch:

1. **Postgres is opt-in, never in-gate.** A real DB server can't run inside `nix flake check` (the
   sandbox has no docker/network) — ClickHouse already runs only as `nix run` apps
   (`nix/clickhouse/default.nix`, pinned `nix/versions.nix:282`). In-gate DB testing uses **in-process
   bundled SQLite** (`rusqlite` `bundled`). So the **`file` + `sqlite` tiers are hermetic in-gate; the
   `postgres` tier is exercised only via `nix run .#integration`**, mirroring ClickHouse.
2. **Two SQL code paths, one trait.** Today's SQLite is `rusqlite` 0.32 (bundled); there is **no `sqlx`**.
   `agent-registry` and `agent-review-fleet` are structural twins (`Memory/File/Sqlite` triad + shared
   `mod ops` + `check_id`/`not_found` + `Mutex<()>` file RMW + `Mutex<Connection>` sqlite
   full-rewrite-in-txn). **Decision: keep `rusqlite` for `file`/`sqlite` (untouched, still hermetic); add
   `postgres` as a new `sqlx` tier.** This supersedes the design sketch's "sqlx one code path" — it is
   behavior-preserving and keeps the hermetic gate intact.
3. **Auth is a tower `Layer`, not a tonic interceptor.** No tonic interceptors exist; the only middleware
   is the tower `AdmissionLayer` in `base_router_with_observer` (`crates/agent-grpc/src/server/health.rs:124`).
   C33 stacks a second `Layer` there and widens the `ServeRouter` type alias
   (`crates/agent-grpc/src/server/mod.rs:26`).

Reused seams: `*_ref` parse `ApiKeyRef::parse` (`crates/agent-core/src/lib.rs:2607`) + resolve
`resolve_token_ref` (`crates/agent-runtime/src/registry.rs:1217`); ingest clamp `Upstream::sanitize`
(`lib.rs:2526`); selectors are string `match`es in `crates/agent-runtime/src/builder.rs` `resolve_*`
(registry `:2696`, fleet `:2739`); CRUD template `crates/agent-grpc/src/server/provider_registry.rs`
(registered in `add_seam_service`, `grpc_server.rs:685`); nix helpers `nix/lib/{contract.sh,serve-wire.sh}`
+ `nix/lib/mk-container-app.nix`; check template `nix/checks/fleet-sqlite.nix`; aggregator
`nix/integration.nix` (register a harness = arg + `runtimeInputs` + `run_step`).

## Track / increment overview

| Track | Phase | Component | Depends on |
|---|---|---|---|
| A (store keystone) | A1 | `agent-config-store` crate: file+sqlite tiers + txn API | — |
| A | A2 | Postgres tier + `[config_store]` bootstrap + opt-in DB harness | A1 |
| A | A3 | Converge `agent-registry` (behavior-preserving) | A2 |
| A | A3b | Converge `agent-review-fleet` | A2 |
| A | A3c | Converge `agent-prompt` (outlier) | A2 |
| B (auth keystone) | B1 | `AuthInterceptor` tower layer + JWKS/JWT + `[auth]` | — |
| C (layers) | C1 | C34 RBAC (roles/permissions as cards) | B1, A1 |
| C | C2 | C35 per-tenant plane (+ C38 prompt) | B1, A3 |
| D (cards) | D1 | C36 forge registry | A1 (+C2 for per-tenant) |
| D | D2 | C37 message-transport registry | A1 (+C2 for per-tenant) |
| E (consolidation) | E1 | C40 control-plane consolidation | B1, C1, C2 |

The two keystones (A, B) are **independent** and may land in either order or in parallel PRs.

---

## Track A — C41 transactional store (keystone; no auth dependency)

### Phase A1 — `agent-config-store` crate: file + sqlite tiers + transaction API (hermetic)

- **Scope.** New crate `crates/agent-config-store/` generalizing the registry↔fleet twin: a generic store
  parameterized over `(domain row type, at-rest codec: prost-blob | json-blob, Error variant,
  sanitize/validate hooks)`. Provide the **`file`** tier (single-bundle atomic RMW: load = re-parse +
  `validate`, persist = temp + `rename`, serialized by `Mutex<()>` — mirror `crates/agent-registry/src/file.rs`)
  and the **`sqlite`** tier (rusqlite `Mutex<Connection>`, `card_blob` + indexed cols + `pos`, full-table
  rewrite inside `conn.transaction()`). A **`transaction(|tx| …)`** API for atomic multi-card writes
  (all-or-nothing; `file` degrades to a whole-doc rewrite). Reuse the `safe_segment` id gate +
  `check_id`/`not_found`; clamp numbers on ingest via the domain `sanitize`. **No convergence of existing
  stores; no Postgres yet.**
- **Key files.** `crates/agent-config-store/` (new); in-memory `ConfigStore` double in
  `crates/agent-testkit/`; `nix/checks/config-store-sqlite.nix` (new, mirror `nix/checks/fleet-sqlite.nix`)
  registered in `nix/checks/default.nix`.
- **Deps.** `rusqlite` (workspace pin), `prost`/`serde_json` codecs; feature `config-store-sqlite`.
- **Tests (in-gate, hermetic; file+sqlite via the trait).** `positive_put_get_roundtrip`,
  `positive_multi_card_commit`, `positive_list_scoped_to_tenant`, `negative_missing_card`,
  `negative_partial_failure_rolls_back_all`, `negative_fk_violation_rejected` (sqlite),
  `boundary_max_cards_per_tenant`, `boundary_number_clamped_on_ingest`, `corner_empty_document`,
  `adversarial_hostile_tenant_id_confined`, `adversarial_sql_injection_via_card_field` (bound param).
- **DoD.** Crate builds (default + feature); the trait matrix (incl. atomic-commit + rollback) green
  in-gate over file+sqlite; `nix flake check --max-jobs 8 --cores 4` green; `cargo-deny`/`cargo-machete`
  clear the new deps; gated PR off `main`.

### Phase A2 — Postgres tier + bootstrap config + opt-in DB-integration harness

- **Scope.** Add `sqlx` (postgres, `runtime-tokio-rustls`) behind feature `config-store-postgres`;
  implement the **`postgres`** tier (pool `pool_max`, MVCC txns, `sqlx::migrate!` migration set).
  `[config_store]` bootstrap TOML in `crates/agent-runtime/src/config.rs`
  (`backend`/`dsn_ref`/`path`/`pool_max`/`migrate_on_start`); **DSN is a `*_ref`** (`env:`/`file:`), inline
  password rejected (a `DsnRef` analogue of `ApiKeyRef::parse`). Add the **`grpc`** store client
  (`GrpcConfigStore`) for the `= "grpc"` tier.
- **Key files.** `crates/agent-config-store/` (postgres module + `migrations/`); `agent-runtime/src/config.rs`
  (`[config_store]`); Postgres pin in `nix/versions.nix` (container block ~L288: `postgresImage="postgres:16"`,
  container name, port 5432, db/user); new `nix/postgres/default.nix` (`postgres-up` bespoke like
  `clickhouse-up`; `down`/`client` via `nix/lib/mk-container-app.nix`) wired in `nix/default.nix` (~377) +
  `mkApps` (~523); new `nix/pg-integration.nix` (`writeShellApplication`, `harness.contract`) registered in
  `nix/integration.nix`.
- **Tests.** Hermetic: `DsnRef` parse matrix incl. `adversarial_dsn_ref_rejects_inline_password`. Opt-in
  (PG harness, `nix run .#integration`): real `positive_multi_card_commit`,
  `negative_partial_failure_rolls_back_all` under MVCC, `corner_concurrent_writers_last_write_conflicts`
  (detected conflict, no lost update; barriers not sleeps); **`= "grpc"` parity** (same trait matrix
  in-process vs remote); **wire-fault** against the store service. 0/1/2 contract; skip-with-notice when
  docker/PG absent.
- **DoD.** `nix run .#integration` runs the PG harness (exit 0) or skips-with-notice on a bare machine;
  txn/rollback/conflict proven on real PG; `DsnRef` adversarial rejection green in-gate; hermetic gate
  green; gated PR.

### Phase A3 — Converge `agent-registry` onto the shared store (behavior-preserving)

- **Scope.** Reimplement registry's file+sqlite on `agent-config-store` and add its `postgres` arm, while
  preserving `trait ProviderRegistry` (`crates/agent-core/src/lib.rs:2808`) and the shared `mod ops`
  semantics. `resolve_provider_registry` (`builder.rs:2696`) gains a `postgres` arm; file/sqlite behavior
  identical.
- **Tests.** The **entire existing model-router/registry suite passes unchanged** (the behavior-preserving
  proof) + new shared-store rows; grpc parity unchanged.
- **DoD.** Pre-existing registry tests green with **zero assertion changes**; postgres arm exercised via
  the PG harness; gate green; gated PR.
- **Build note (A3 shipped).** Per grounded decision #2 (keep `rusqlite` for `file`/`sqlite`, add
  `postgres` as the new tier), the legacy `Memory`/`File`/`Sqlite` registry backends are **untouched** —
  so their tests pass unchanged *by construction*. The convergence lands as a new `StoreRegistry`
  (`crates/agent-registry/src/store.rs`, feature `registry-store`) that implements `ProviderRegistry` over
  any `agent_config_store::Backend` (memory/file/sqlite/**postgres**), reusing the shared `ops`/`decide` and
  the same `pb::Upstream`/`pb::RoutePolicy` blobs as the SQLite tier (upstreams + one policy card;
  single-tenant `local` until C2). `resolve_provider_registry` gains the `postgres` arm
  (feature `registry-postgres`), building it over a lazily-connecting `PgBackend` from `[config_store]`.
  In-gate tests run `StoreRegistry` over `MemoryBackend` (agreeing with `MemoryRegistry`); the postgres arm
  runs `#[ignore]`-gated under `nix run .#integration`.

### Phase A3b — Converge `agent-review-fleet` (the clean twin)

- **Scope.** Same shape as A3 for `trait FleetRegistry` (`crates/agent-core/src/lib.rs:2952`; note the
  `set_enabled` verb, JSON codec).
- **DoD.** Existing fleet suite green **unchanged** + postgres arm via the PG harness; gate green; gated PR.
- **Build note (A3b shipped).** Same shape as A3: the legacy `Memory`/`File`/`Sqlite` fleet backends are
  **untouched**, and the convergence lands as a new `StoreFleet`
  (`crates/agent-review-fleet/src/store.rs`, feature `fleet-store`) over any `agent_config_store::Backend`,
  reusing the shared `ops` and the same **JSON** roster rows the file/sqlite tiers store (one collection,
  `fleet_sessions`; single-tenant `local` until C2). `resolve_fleet_registry` gains the `postgres` arm
  (feature `fleet-postgres`) over the shared `store_backend::pg_backend`. In-gate:
  `nix/checks/fleet-store.nix` runs `StoreFleet` over `MemoryBackend` (agreeing with `MemoryFleet`); the
  postgres arm runs `#[ignore]`-gated under `nix run .#integration`.

### Phase A3c — Converge `agent-prompt` (the outlier)

- **Scope.** `FilePromptStore`'s directory-tree tier **stays as-is** (it doesn't fit a single-bundle
  abstraction); route only `sqlite`/`postgres`/`grpc` through the shared store so prompt cards can join a
  cross-card transaction.
- **DoD.** Existing prompt suite green **unchanged**; gate green; gated PR.

---

## Track B — C33 authentication interceptor (keystone; independent of Track A)

### Phase B1 — `AuthInterceptor` tower layer + JWKS/JWT verify + `[auth]` bootstrap

- **Scope.** A tower `Layer` (mirror `crates/agent-grpc/src/server/admission.rs`, HTTP/BoxBody level)
  stacked in `base_router_with_observer` (`health.rs:124`) beside `AdmissionLayer`; widen the `ServeRouter`
  alias (`server/mod.rs:26`) to a two-element `Stack`. Verify a **bearer JWT**: JWKS fetch+cache from the
  configured issuer (rotation honored), **alg allow-list** RS256/ES256 (reject `alg:none` + HS/RS
  confusion), `iss`/`aud`/`exp`/`nbf`(+leeway)/tenant-claim/`sub`. Derive
  `VerifiedIdentity { tenant, subject, roles }` and install it into the `AGENT_IDENTITY` scope so
  `identity_key`/`run_scoped` (`server/mod.rs:137`/`:149`) trust it; **when a verified token is present,
  ignore `x-agent-user-id`**. `[auth]` bootstrap TOML (`mode` oidc|none, issuer, audience, jwks_url,
  tenant_claim, roles_claim, leeway_secs); **`mode=none` preserves today's header path** (explicit,
  logged opt-out; default `oidc`, fail-closed).
- **Key files.** `crates/agent-grpc/src/server/` (new auth layer module + `mod.rs:26` alias + `health.rs:124`
  wiring); `agent-runtime/src/config.rs` (`[auth]`); fake OIDC issuer/JWKS in `crates/agent-testkit/`.
- **Deps.** `jsonwebtoken` (verify-only) behind feature `auth`; clear `deny.toml`.
- **Tests (in-gate; static JWKS, injected clock).** `positive_valid_jwt_derives_tenant`,
  `positive_jwks_rotation_reverifies`, `negative_expired_rejected`, `negative_bad_signature_rejected`,
  `negative_wrong_audience_rejected`, `boundary_clock_skew_within_leeway`,
  `corner_no_token_is_unauthenticated`, `corner_mode_none_uses_header`, `adversarial_alg_none_rejected`,
  `adversarial_client_header_ignored_when_token_present`, `adversarial_hs256_key_confusion_rejected`.
  Opt-in `nix/auth-e2e.nix` (live interceptor + fake issuer over the wire; register in `integration.nix`).
- **DoD.** Interceptor composes over every seam; `mode=none` default-preserves (existing serve-smoke
  green); hermetic verifier matrix green; auth-e2e opt-in exit 0; **no proto change → no buf bump**; gate
  green; gated PR.

---

## Track C — layers on the keystones

### Phase C1 — C34 RBAC (needs B1)

- **Scope.** `role.proto` (**NEW, additive**): `Role`, `Permission(action, resource_type)`, `Binding`;
  `RoleService` (List/Get/Put/Delete). `agent-core`: closed `Action` enum, `ResourceType`,
  `authorize(identity, action, resource) -> Decision`; **deny-by-default**, opaque reason, **cross-tenant
  structurally unresolvable**. Roles/permissions are **cards on the C41 store**. The check wraps every
  control-plane CRUD/config handler (`Put`/`Delete`/`Enable`/`Approve`) — **distinct from the tool
  `Policy`** (`lib.rs:4070`). Seed built-ins: `operator` (host), `org_admin` (tenant).
- **Tests.** `positive_role_grants_rpc`, `positive_operator_crosses_tenants`,
  `negative_missing_permission_denied`, `negative_reader_cannot_write`, `boundary_role_at_hierarchy_edge`,
  `corner_role_with_no_permissions_denies_all`, `corner_unknown_action_denied`,
  `adversarial_cross_tenant_access_denied`, `adversarial_self_grant_escalation_denied`,
  `adversarial_forged_roles_claim_needs_verification` (roles only from B1's verified token). RoleService
  serve-smoke (tcp+uds).
- **DoD.** `authorize` gates all control-plane RPCs; decision table green in-gate; serve-smoke green; buf
  additive (no bump); gate green; gated PR.

### Phase C2 — C35 per-tenant plane (+ C38 prompt) (needs B1 + A3)

- **Scope.** `PerTenant<S>` generalizing `PerUserMemory` (`crates/agent-memory/src/tenant.rs:57`): resolve
  `current_identity().user` per call, lazily build+cache the per-tenant view, `local` → un-namespaced base.
  Wrap `ProviderRegistry`/`FleetRegistry`/`PromptStore`/`GraphStore`/`Scheduler` in `builder.rs`, **enabled
  only under a per-tenant bootstrap flag** (Tier-0 = today, unchanged). Enforce the operator-global vs
  tenant split at the control plane (tenant write to an operator key → denied). **This IS multi-tenancy
  C30** — one implementation, coordinate (see `../multi-tenancy/STATUS.md`). **C38** = this applied to
  `PromptStore`.
- **Tests.** `positive_two_tenants_isolated_stores`, `positive_cached_store_reused`,
  `negative_tenant_cannot_read_other`, `negative_tenant_write_to_operator_key_denied`,
  `boundary_local_tenant_uses_base_path`, `corner_first_write_creates_tenant_view`,
  `corner_no_identity_defaults_local`, `adversarial_hostile_tenant_id_confined`,
  `adversarial_identity_from_token_not_header`, `positive_two_tenants_isolated_prompt_dbs`. Opt-in
  `nix/tenant-isolation.nix` (over the wire: tenant A cannot read/write tenant B); register in
  `integration.nix`.
- **DoD.** Isolation structural + wire-proven; Tier-0 unchanged; cross-track note added to multi-tenancy
  STATUS; gate green; gated PR.

---

## Track D — domain cards (need A1+; per-tenant via C2)

### Phase D1 — C36 forge registry

- **Scope.** `forge_registry.proto` (**NEW, additive**): `ForgeCard`/`ForgeRegistry`/`ForgeRegistryService`.
  `agent-core` `trait ForgeRegistry`; **drop the hardcoded allow-list** (`lib.rs:2917`) → known kinds =
  registered factory kinds; per-kind default `base_url` + `repo_encoding` owned by the impl. Forge factory
  by `kind` (`registry.rs`); `FleetSession` references a forge by card id (**additive field, no bump**);
  `build_session_forge` (`registry.rs:1247`) resolves the card. `token_ref` discipline; `base_url`
  URL-validated + SSRF-screened on the operational forge; unknown `kind` fail-closed. New host impls
  (gitea/bitbucket) are **deferred behind features** — the card + plumbing is this phase.
- **Tests.** Full C36 matrix (`positive_github_card_builds_forge`, `positive_self_hosted_gitlab_base_url`,
  `positive_gitlab_subgroup_encoding`, `negative_unknown_backend_rejected`,
  `negative_missing_repo_encoding_rejected`, `boundary_empty_base_url_uses_kind_default`,
  `boundary_timeout_clamped`, `corner_repo_with_dots_and_dashes_preserved`,
  `adversarial_hostile_repo_slug_rejected`, `adversarial_token_ref_rejects_raw_secret`,
  `adversarial_base_url_ssrf_screened`). ForgeRegistryService serve-smoke.
- **DoD.** Allow-list no longer hardcoded; matrix + serve-smoke green; buf additive (no bump); gate green;
  gated PR.

### Phase D2 — C37 message-transport registry

- **Scope.** `transport_registry.proto` (**NEW, additive**): `TransportCard`/`ChannelBinding`/
  `TransportRegistry`/`TransportRegistryService`. `agent-core`: **bidirectional `MessageTransport`** seam
  (`recv` + `post`; neutral `InboundMessage`/`OutboundMessage`/`Channel`). `agent-slack` becomes an impl
  (`SlackTransport: MessageTransport`) with an outbound `post`. Lift `slack_*`/`FleetSlackCfg` config out of
  `FleetSession` into a transport card referenced by id + purpose (trigger/progress). Factory by `kind`;
  unknown kind fail-closed; inbound text stays **data** (only PR# extracted); `token_ref` never logged;
  per-transport rate-limit + soft-fail post. The review-fleet C18 progress feed becomes a thin caller of
  `post`.
- **Tests.** Full C37 matrix (`positive_slack_recv_and_post_roundtrip`,
  `positive_fleet_references_transport_card`, `negative_unknown_transport_kind_rejected`,
  `negative_post_without_bot_token_errors`, `boundary_rate_limit_enforced`, `corner_post_failure_is_soft`,
  `corner_bot_message_does_not_trigger`, `adversarial_inbound_text_is_not_executed`,
  `adversarial_lookalike_host_link_rejected`, `adversarial_token_ref_never_logged`). Fake
  `MessageTransport` in `agent-testkit`; TransportRegistryService serve-smoke.
- **DoD.** Slack recv+post roundtrip green; config lifted out of `FleetSession`; matrix + serve-smoke
  green; buf additive; gate green; gated PR.

---

## Track E — consolidation

### Phase E1 — C40 control-plane consolidation (needs B1 + C1 + C2)

- **Scope.** Apply the C33 interceptor + C34 `authorize` + C35 tenant-scoping **uniformly** across
  `ConfigService` and every CRUD registry (provider/fleet/prompt/graph/scheduler/forge/transport/role).
  `Get`/`List` tenant-scoped; operator keys reject tenant writes; no service bypasses the gate. The portal
  admin surface (tenants/roles/forges/transports) is **noted as future**, not built here.
- **Tests.** `positive_admin_edits_own_tenant`, `negative_tenant_write_to_operator_key_denied`,
  `adversarial_unauthenticated_rpc_denied` + the per-tenant-isolation integration test. Run the full
  integration suite (serve-smoke / auth-e2e / tenant-isolation / pg-integration / grpc-parity / wire-fault).
- **DoD.** One gate uniformly applied, verified no-bypass; full integration suite green (or
  skip-with-notice on a bare machine); gate green; gated PR.

---

## Cross-cutting testing strategy (binds every phase)

- **Layer 1 — in `nix flake check` (hermetic).** Every unit is a table-driven `rstest`: four classes
  (`positive_`/`negative_`/`boundary_`/`corner_`) **plus `adversarial_` for every untrusted input** (cards,
  `*_ref`/DSN, tenant/role/repo ids, wire numbers, JWT claims, SQL-bound fields), each row `desc`+`expect`,
  `#[cfg(test)] mod` at file end. Covers the file+sqlite store tiers (`tempdir()`), the JWT verifier against
  a **static in-test JWKS** (no network), and the RBAC decision tables. New feature-gated checks follow the
  `nix/checks/fleet-sqlite.nix` `cargoTest --features` template, registered in `nix/checks/default.nix`.
- **Layer 2 — opt-in, `nix run .#integration`.** `pg-integration` (real PG: commit/rollback/concurrent
  conflict; the sqlite tier runs the same suite), `serve-smoke` (each new service, TCP+UDS), `auth-e2e`,
  `tenant-isolation`, `= "grpc"` store parity, `wire-fault`. Shared **0/1/2 exit contract**
  (`nix/lib/contract.sh`); gated on the resource being present, **skip-with-notice** otherwise so the
  aggregate runs on a bare machine.
- **Fixtures (`agent-testkit`).** In-memory `ConfigStore`, fake `MessageTransport`, fake `Forge`, fake OIDC
  issuer (mint/rotate + JWKS), `RoleFixture`. **Determinism:** inject the clock for JWT expiry; conflict
  tests use barriers, not sleeps.
- **Coverage contract.** No phase merges until every cell of its
  [08-testing-and-integration.md](08-testing-and-integration.md) matrix is populated and green under the
  gate.
- **Proto governance.** New card protos (`forge_registry`/`transport_registry`/`role`) and appended
  reference fields are **additive → no `buf.image.binpb` bump**; obey `buf lint` STANDARD (`agent.v1`,
  snake_case, enum prefixes, unique req/resp). C33 changes no proto.

## Out of scope (across all increments)

Concrete IdP product choice (beyond OIDC/JWT bearer); a third identity tier (`org → team → user`); the
portal admin surface; new forge/transport host impls (gitea/bitbucket/matrix/teams/…) — each noted as
future within its phase. Multi-tenancy C29–C31 is **not** re-specified — C2/E1 are its single config-surface
implementation.
