# Components (C32–C41)

The component catalogue continues the repo-wide C-numbering: review-fleet owns **C1–C22**,
multi-tenancy owns **C23–C31**, so the config track starts at **C32**. Each entry carries
**Purpose / New-or-reuse / Interface / Security** (with a `file:line` seam anchor) **and a test-matrix
stub** cross-referencing [`08-testing-and-integration.md`](08-testing-and-integration.md) — no component
is specified without its tests.

Legend for "New-or-reuse": 🆕 new · ♻️ reuse/extend an existing seam · 🔗 coordinates with another track.

---

## C32 — Config-card pattern (meta-component) ♻️

- **Purpose.** The canonical shape *all* domain config follows, formalizing what
  `ProviderRegistryService` already does. The rule: **new domain config is a card, not a TOML section.**
- **Interface.** A card is one protobuf message = one config document; its **textproto rendering is the
  on-disk file**; a **CRUD + introspection service** (`List/Get/Put(upsert)/Delete` + domain verbs) is
  the live control plane; a **store trait** with `file`/`sqlite`/`postgres`/`grpc` backends (the C41
  data layer) persists it; **secrets ride as `*_ref` references**; **numbers are clamped on ingest**;
  **live-only state is a separate never-persisted message**; the store is **`PerTenant`-wrapped** (C35)
  and **refreshes live**. Reference: `upstream.proto` (`ModelRouterConfig` + `ProviderRegistryService`),
  `crates/agent-core/src/lib.rs:2808` (`trait ProviderRegistry`), `crates/agent-registry/src/file.rs:44`.
- **New-or-reuse.** ♻️ Formalization of an existing pattern — no new code; it is the *convention* the
  other components instantiate.
- **Security.** The pattern *is* the security posture: refs-not-secrets, ingest clamping, fail-closed
  validation, per-tenant isolation by construction.
- **Tests.** Meta: the pattern's guarantees are exercised by every card's matrix (roundtrip, clamp,
  ref-not-secret, per-tenant confinement). See C41 for the shared-store rows.

## C33 — Authentication interceptor (OIDC/JWT) 🆕🔗

- **Purpose.** Establish a **trustworthy** identity. Today there is none — the identity header is
  "attacker-controllable … no auth layer" (`crates/agent-core/src/identity.rs:4`). The keystone every
  tenant guarantee depends on.
- **Interface.** A tonic interceptor verifies a **bearer JWT** (issuer/audience/signature/expiry via
  JWKS) and **derives** `Identity { tenant, subject, roles }` from verified claims, installing it at the
  existing extraction seam (`identity_key` / `run_scoped` / `server::span`,
  `crates/agent-grpc/src/server/mod.rs:137`). When a verified token is present the client-supplied
  `x-agent-user-id` header is **ignored**. Concretizes the multi-session 07 follow-up
  (`../multi-session/07-security.md`).
- **New-or-reuse.** 🆕 New interceptor + JWKS verifier; ♻️ reuses the `AGENT_IDENTITY` task-local +
  `scope()` carrier (`identity.rs:240`) unchanged downstream.
- **Security.** The trust boundary moves from "trust the header" to "verify the token". Fail-closed:
  missing/expired/forged token → `UNAUTHENTICATED`; unverified header alone → no identity (today's
  "default `local`" only under an explicit dev/no-auth bootstrap flag).
- **Tests.** `positive_valid_jwt_derives_tenant`, `negative_expired_rejected`,
  `negative_bad_signature_rejected`, `boundary_clock_skew_within_leeway`,
  `corner_no_token_is_unauthenticated`, `adversarial_client_header_ignored_when_token_present`,
  `adversarial_alg_none_rejected`, `adversarial_wrong_audience_rejected`. (Full detail
  [`02-auth-and-rbac.md`](02-auth-and-rbac.md).)

## C34 — RBAC model 🆕

- **Purpose.** Authorize *who may do what* on the control plane — distinct from the per-*call* tool
  `Policy` seam (`crates/agent-core/src/lib.rs:4070`), which gates the model's tool use, not a user's
  config edits.
- **Interface.** `Role` and `Permission` are **cards** (stored + CRUD'd like any other). A permission is
  `(action, resource-type)`; roles bind permissions; identities bind roles, scoped to a point in the
  hierarchy `host ⊃ org(tenant) ⊃ team? ⊃ user` (single-level in v1; `team` is the noted extension,
  `identity.rs:179`). A check `authorize(identity, action, resource)` gates every control-plane RPC.
- **New-or-reuse.** 🆕 New role/permission cards + the control-plane check; ♻️ leans on C33's derived
  `roles` and the existing `safe_segment` id discipline.
- **Security.** Deny-by-default; cross-tenant access denied structurally (a check can never resolve a
  resource outside the caller's tenant subtree); denial reasons are opaque (no probing oracle, mirroring
  `AllowList`).
- **Tests.** `positive_role_grants_rpc`, `negative_missing_permission_denied`,
  `boundary_role_at_hierarchy_edge`, `corner_role_with_no_permissions_denies_all`,
  `adversarial_cross_tenant_access_denied`, `adversarial_privilege_escalation_via_self_grant_denied`.

## C35 — Per-tenant config plane 🔗♻️

- **Purpose.** Isolate every card store per org, and split operator-global vs tenant-owned keys.
  **Coordinates with / subsumes** multi-tenancy C29–C31 (references them as the enforcement mechanics).
- **Interface.** `PerTenant<Store>` generalizes `PerUserMemory` (`crates/agent-memory/src/tenant.rs:57`):
  resolve `current_identity().tenant` per call, lazily build + cache a per-tenant store view, route
  reads/writes into it. Applied to `ProviderRegistry`, `GraphStore`, `PromptStore`, `Scheduler`,
  `FleetRegistry`, and messaging. Operator-global keys (bootstrap) are rejected for tenant writes at the
  control plane (multi-tenancy C29/C31).
- **New-or-reuse.** ♻️ Generalizes an existing wrapper; 🔗 the wrapper is multi-tenancy C30. No trait
  changes to the wrapped stores.
- **Security.** Isolation is **structural**, bound to verified ambient identity (C33), never a
  model/tenant-supplied value; `local` tenant maps to the un-namespaced base (single-tenant unchanged).
- **Tests.** `positive_two_tenants_isolated_stores`, `negative_tenant_cannot_read_other`,
  `boundary_local_tenant_uses_base_path`, `corner_first_write_creates_tenant_view`,
  `adversarial_hostile_tenant_id_confined` (traversal/separator → `safe_segment` reject).

## C36 — Forge registry 🆕♻️

- **Purpose.** Make the forge pluggable via cards (github/gitlab/gitea/bitbucket/sourceforge…). The
  `Forge` trait is already generic (`crates/agent-core/src/lib.rs:3680`) with a factory registry
  (`r.forge(name, …)`), but the backend allow-list (`"" | "github" | "gitlab"`,
  `crates/agent-core/src/lib.rs:2917`), default base URLs, and per-backend repo-encoding are hardcoded.
- **Interface.** A **forge card** (keyed like `Upstream`): `id`, `kind`, `base_url`, `token_ref`,
  `repo_encoding` (owner__name / group/subgroup / …), capability flags. A forge factory keyed by `kind`;
  adding a host = a new `Forge` impl + a card kind, no core allow-list edit.
- **New-or-reuse.** ♻️ Reuses the `Forge` trait + factory registry; 🆕 the card, the pluggable
  backend/base_url/repo-encoding, new impls (gitea/bitbucket/sourceforge) as future cards.
- **Security.** `token_ref` is a reference; `base_url` and repo slugs `safe_segment`/URL-validated on
  ingest; unknown `kind` rejected fail-closed.
- **Tests.** `positive_github_card_builds_forge`, `positive_gitlab_subgroup_encoding`,
  `negative_unknown_backend_rejected`, `boundary_empty_base_url_uses_kind_default`,
  `corner_repo_with_dots_preserved`, `adversarial_hostile_repo_slug_rejected`. (Full detail
  [`04-forge-registry.md`](04-forge-registry.md).)

## C37 — Message-transport registry 🆕♻️

- **Purpose.** Generalize messaging beyond Slack. Today `SlackTransport` is **inbound-only and
  Slack-named** (`crates/agent-slack/src/lib.rs:43`), with channel/token config baked into
  `FleetSession`/`FleetSlackCfg`.
- **Interface.** A bidirectional **`MessageTransport`** seam (`recv` + `post`) with a transport-neutral
  `InboundMessage`/`OutboundMessage`; a **transport card** (`kind` = slack/matrix/teams/irc/signal,
  endpoint/workspace, `*_token_ref`, channel bindings). Slack is the first impl; the neutral
  `FleetTrigger`/`TriggerSink` downstream is already transport-agnostic. Channel/token config lifts out
  of `FleetSession` into the card.
- **New-or-reuse.** ♻️ Reuses the inbound seam shape + neutral trigger pipeline; 🆕 the bidirectional
  trait, the card, the outbound half, matrix/teams/irc/signal as future impls.
- **Security.** Tokens are `*_ref`; inbound message text stays **data, never instructions** (only a PR
  number is ever extracted, as today); outbound posts reuse the review redaction pass; per-transport
  rate-limit + soft-fail.
- **Tests.** `positive_slack_recv_and_post_roundtrip`, `negative_unknown_transport_kind_rejected`,
  `boundary_rate_limit_enforced`, `corner_post_failure_is_soft`,
  `adversarial_inbound_text_is_not_executed`, `adversarial_token_ref_never_logged`. (Full detail
  [`05-message-transport.md`](05-message-transport.md).)

## C38 — Per-tenant prompt storage (light) ♻️🔗

- **Purpose.** "Different orgs store prompts in different databases." `PromptStore` (file/sqlite/grpc,
  `crates/agent-core/src/lib.rs:2302`) exists but is a single **process-wide** instance
  (`crates/agent-runtime/src/builder.rs:959`).
- **Interface.** A `PerTenantPromptStore` wrapper (C35 applied to `PromptStore`) resolving the tenant per
  call + a per-tenant connection card. The trait needs **no change**; `sqlite`/`grpc`/`postgres` already
  prove multiple DBs are constructible.
- **New-or-reuse.** ♻️ Pure application of C35 to an existing seam.
- **Security.** Same as C35 (structural per-tenant isolation).
- **Tests.** Covered by the C35 matrix instantiated for `PromptStore`
  (`positive_two_tenants_isolated_prompt_dbs`, `adversarial_hostile_tenant_id_confined`).

## C39 — LLM upstream/pool config (reference, light) ♻️

- **Purpose.** Record that this is **already built** and is the exemplar of C32 — `upstream.proto`
  (`Upstream`/`ModelRouterConfig`/`ProviderRegistryService`) + `llm_pool.proto` (`LlmPoolService`,
  members/health) + `agent-registry` (file/sqlite/grpc stores).
- **Interface.** No change proposed beyond consistency deltas (align naming/ingest-clamp conventions,
  move its store onto the shared C41 backend, `PerTenant`-wrap it via C35).
- **New-or-reuse.** ♻️ Reference; the only deltas are convergence onto C41 + C35.
- **Security.** Already conformant (`api_key_ref`, ingest clamps, health split).
- **Tests.** Existing model-router tests stand; add the shared-store + per-tenant rows when it converges.

## C40 — Config control-plane consolidation 🔗

- **Purpose.** Put every control-plane service under one auth (C33) + RBAC (C34) gate, tenant-scoped
  (C35), and describe the portal admin surface. Coordinates multi-tenancy C31.
- **Interface.** The interceptor + an RBAC check wrap `ConfigService` and every CRUD registry
  (`ProviderRegistry`/`ReviewFleet`/`Prompt`/`Graph`/`Scheduler` + the new forge/transport/role
  services). `Get/List` are tenant-scoped; operator-global keys reject tenant writes.
- **New-or-reuse.** 🔗 Composition of C33/C34/C35 over existing services; the portal gains a tenants/
  roles/forges/transports admin surface (future, not this pass).
- **Security.** One gate, uniformly applied; no service bypasses it.
- **Tests.** `positive_admin_edits_own_tenant`, `negative_tenant_write_to_operator_key_denied`,
  `adversarial_unauthenticated_rpc_denied`, plus the per-tenant-isolation integration test (C41/testing).

## C41 — Transactional config data layer 🆕

- **Purpose.** The **OLTP config store** — the reason config is SQL, not ClickHouse (which stays
  telemetry-only). One store trait, three tiers, atomic multi-card transactions.
- **Interface.** A store trait with `file` (textproto; bootstrap/dev) → `sqlite` (single-node) →
  **`postgres`** (multi-writer production) backends, selected by a `store = "..."` string (the
  established selector). **Atomic multi-card transactions** (create tenant + roles + cards in one
  commit). SQL schema + migrations; per-tier connection config in bootstrap TOML (`store`, DSN,
  credential `*_ref`). The concrete DB is **abstracted behind the gRPC seam** (`= "grpc"`). Topology:
  **per-domain typed services share this one store** — not a generic mega-service. Generalizes today's
  `agent-registry`/`agent-prompt`/`agent-review-fleet` file+sqlite+grpc stores onto a shared,
  Postgres-capable backend (proposed crate `agent-config-store`, async SQL via `sqlx`).
- **New-or-reuse.** 🆕 The Postgres tier + the transactional/shared-store layer + migrations; ♻️ the
  file/sqlite/grpc tier shapes and the selector pattern already exist per-domain.
- **Security.** Credentials are `*_ref` (bootstrap-resolved); every card ingest is clamped/validated;
  per-tenant rows are scoped by verified identity (C33/C35); transactions are all-or-nothing.
- **Tests.** `positive_put_get_roundtrip`, `negative_missing_card`, `boundary_max_cards_per_tenant`,
  `corner_empty_document`, `adversarial_hostile_tenant_id_confined`,
  `positive_multi_card_commit`, `negative_partial_failure_rolls_back_all`; integration:
  Postgres harness (real transactions, rollback, concurrent-writer conflict) + `= "grpc"` parity. (Full
  detail [`06-config-store-and-data-layer.md`](06-config-store-and-data-layer.md).)

---

## Dependency graph

```
C33 auth ─┬─> C34 RBAC ─────────────┐
          └─> C35 per-tenant ───┐    │
C41 store ─────────────────────┴────┴─> C40 control-plane consolidation
   │                                     ^
   └─> C36 forge / C37 transport / C38 prompt / C39 llm  (all cards on the shared store)
C32 pattern = the convention all of the above instantiate
```

**Two keystones:** C41 (the shared transactional store) and C33 (verified identity). RBAC (C34) and
per-tenant (C35) sit on both; the domain card registries (C36–C39) sit on C41; C40 composes them.
