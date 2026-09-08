# Unified configuration architecture — design of record

> **Status:** design / pre-implementation. **This is docs only** — nothing here is built yet.
> Each component is tracked in [`STATUS.md`](STATUS.md); the cross-cutting build map is
> [`IMPLEMENTATION.md`](IMPLEMENTATION.md).

## Why this exists

agent-seddon's configuration grew organically into **two coexisting philosophies**, and we can now
predict the primitives the next phase needs (an org running the review fleet at 10–50 tenants). This
document steps back, names the pattern the codebase has *already been converging on*, unifies the two
philosophies under it, and sketches the whole future config surface — filling in full detail for the
four primitives we need now (**auth + RBAC, per-tenant config, multi-forge, generic messaging**) and a
transactional **config data layer**, and leaving the rest as lighter sketches.

It deliberately **coordinates with, and does not duplicate**, the [`multi-tenancy/`](../multi-tenancy/README.md)
(C23–C31), [`multi-session/`](../multi-session/README.md) (identity + the auth follow-up),
[`model-router/`](../model-router/README.md) (the reference config-card impl), and
[`review-fleet/`](../review-fleet/README.md) tracks.

## The two philosophies today

**1. Static TOML.** `config/agent.toml` → 46 `[section]` structs
(`crates/agent-runtime/src/config.rs:13`) → a `schemars` JSON-Schema
(`crates/agent-runtime/src/config_schema.rs:109`) → the `ConfigService` seam
(`GetSchema/GetValues/Validate/Put/Status`, `crates/agent-proto/proto/agent/v1/config.proto:19`) with
**comment-preserving `toml_edit` write-back** (`crates/agent-runtime/src/config_store.rs`) → the portal
Settings tab. It is **operator-global** and **immutable until restart**, and it selects seam impls via
`backend = "..."` strings resolved through the factory registry (`register_builtins`,
`crates/agent-runtime/src/registry.rs:448` — 13 seam kinds, plus more wired directly in `builder.rs`).

**2. Dynamic protobuf/textproto registries.** Live CRUD, no restart. The mature exemplar is
`ProviderRegistryService` + `ModelRouterConfig` (`crates/agent-proto/proto/agent/v1/upstream.proto`):
one proto **message = the whole config document**, its **textproto rendering = the on-disk file**
(`config/model-router/example.textproto`), a **CRUD service = the live control plane**, a **store trait**
with `file`/`sqlite`/`grpc` backends (`crates/agent-registry/src/file.rs:44` re-parses each read so
hand-edits are picked up live; `RegistryRouter` refreshes on an interval), **secrets as `*_ref`
references** (`api_key_ref: "env:GLM_API_KEY"`), **numbers clamped on ingest**, and **live-only state
split into a never-persisted message** (`UpstreamHealth`). The same shape recurs in `ReviewFleetService`,
`GraphService`, `PromptService`, and `SchedulerService`.

The insight: **philosophy 2 is the destination.** This design formalizes it as *the config-card
pattern* (C32) and states the rule that new domain config is a **card**, not a TOML section.

## The two-tier model (the boundary rule)

Config splits into two tiers, and every knob belongs to exactly one:

- **Operator-global bootstrap → stays TOML** (`agent.toml`). What the *process* needs before it can
  serve anything: gRPC ports/sockets/wiring, **which store backend** each seam uses, first-run
  connection details (the SQL DSN, the IdP issuer URL), telemetry endpoints. One host, one operator.
  **No TOML rip-out.**
- **Domain + per-tenant config → protobuf registries** (the config-card pattern, on the transactional
  store of C41). Everything a tenant or admin edits at runtime: LLM upstreams/routing, forges,
  messaging transports, prompts, the fleet roster, tenants/roles/permissions. Lives in the store,
  driven over gRPC, per-tenant-scoped, no restart.

Decision rule: *if it must exist before the server starts, or is inherently one-per-host, it is
bootstrap TOML; otherwise it is a card.* (Full rule + a per-section migration map in
[`01-config-card-pattern.md`](01-config-card-pattern.md).)

## The dependency chain: auth → tenancy → RBAC

Everything tenant-scoped rests on a trustworthy identity, which **does not exist today**: identity is a
trusted `x-agent-user-id` / `x-agent-session-id` metadata header, explicitly "attacker-controllable …
no auth layer" (`crates/agent-core/src/identity.rs:4`). So the chain is:

1. **Authentication (C33)** — an OIDC/JWT bearer interceptor at the gRPC boundary *verifies* a token and
   **derives** the tenant + subject, replacing the trusted header at the existing extraction point
   (`crates/agent-grpc/src/server/mod.rs:137`). The keystone.
2. **Per-tenant config (C35)** — with a trustworthy tenant, `PerTenant<Store>` (generalized from
   `PerUserMemory`, `crates/agent-memory/src/tenant.rs:57`) isolates every card store per org.
3. **RBAC (C34)** — roles + permissions gate the **control-plane RPCs** (who may edit which cards),
   distinct from the per-call tool `Policy` seam.

## Storage: OLTP config vs OLAP telemetry

Config is **transactional (OLTP)** and needs atomic multi-card writes (create a tenant + its roles + its
forge/transport cards in **one commit**). **ClickHouse is unsuitable and stays telemetry-only (OLAP).**
The config store trait therefore has three tiers behind one seam — `file` (textproto; bootstrap/dev) →
`sqlite` (single-node) → **`postgres`** (the serious multi-writer production DB for 10–50 tenants) —
and the concrete DB is **abstracted behind the Rust gRPC seam** (`= "grpc"`) so no caller binds to it.
Topology: **per-domain typed services share this one store** (not a generic mega-service); full detail
in [`06-config-store-and-data-layer.md`](06-config-store-and-data-layer.md).

## Security posture

- **Secrets are never stored** — only kind-prefixed *references* (`env:NAME` / `file:/path`), resolved
  on the host that builds the concrete client, exactly as `Upstream.api_key_ref` /
  `FleetSession.token_ref` do today. A card is safe to back up, log (masked), and serve over the wire.
- **Identity is derived, never asserted** — post-C33 the verified token is authoritative; a
  client-supplied identity header is ignored when a token is present.
- **Fail closed** — every untrusted input (imported cards, tenant/role/repo ids, DSNs, wire numbers,
  JWT claims) is validated/clamped/`safe_segment`-checked on ingest; a bad value is rejected, not
  sanitized.

## Testing is a design output

Per [`../../../CLAUDE.md`](../../../CLAUDE.md), testing is specified here, not bolted on later. Every
component (C32–C41) ships a **table-driven test matrix** — `desc` + `expect` per row, all four classes
(`positive_`/`negative_`/`boundary_`/`corner_`) **plus a mandatory `adversarial_` class** for untrusted
input — and the design names the **integration harnesses** (Postgres DB-integration, serve-smoke,
auth end-to-end, per-tenant isolation, `= "grpc"` parity, wire-fault). Full matrix in
[`08-testing-and-integration.md`](08-testing-and-integration.md).

## Non-goals

- **No TOML removal** — bootstrap stays TOML.
- **No concrete IdP selection** beyond committing to OIDC/JWT bearer as the mechanism.
- **No third identity tier** (`org → team → user`) in v1 — noted as the extension
  (`crates/agent-core/src/identity.rs:179`), not specified.
- **No duplication of multi-tenancy C29–C31** — referenced as the per-tenant enforcement mechanics.
- **No code** this pass — every component is designed here, built later per [`STATUS.md`](STATUS.md).

## Reading order

[`00-components.md`](00-components.md) (the C32–C41 catalogue) → the primitive docs
[`01`](01-config-card-pattern.md)–[`07`](07-storage-migration-and-existing.md) →
[`08-testing-and-integration.md`](08-testing-and-integration.md) → [`STATUS.md`](STATUS.md) /
[`IMPLEMENTATION.md`](IMPLEMENTATION.md).
