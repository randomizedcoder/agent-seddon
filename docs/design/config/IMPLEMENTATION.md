# Implementation touch map (for a future build)

Cross-cutting blast radius for the C32–C41 build. **Nothing here is built yet** — this is the order-of-
operations + risk map a future set of gated PRs follows. Anchors are current `file:line`s.

## Crates / files each component touches

| Component | `agent-proto` | `agent-core` | `agent-grpc` | `agent-runtime` | other crates |
|---|---|---|---|---|---|
| C41 store | (schemas live in each card's proto) | store trait shape | store `= "grpc"` server/client per domain | `builder.rs` store selection; `[config_store]` in `config.rs` | **new `agent-config-store`** (or generalize `agent-registry`): `file`/`sqlite`/`postgres` via `sqlx` + SQL migrations |
| C33 auth | none (identity rides metadata, `agent-proto/src/identity.rs:6`) | `Identity` enrich (`identity.rs`) | **auth interceptor** at `server/mod.rs:137` + JWKS verifier; `[auth]` in `config.rs` | wire the interceptor into every `serve_*` | JWKS/JWT dep (e.g. `jsonwebtoken`) |
| C34 RBAC | `role.proto` (NEW, additive) | `authorize()` + role/permission types | RBAC check wrapping control-plane handlers | role card store on C41 | — |
| C35 per-tenant | none | (uses `current_identity`) | — | `PerTenant<Store>` wrap in `builder.rs` (= multi-tenancy C30) | generalize `agent-memory/src/tenant.rs:57` |
| C36 forge | `forge_registry.proto` (NEW, additive) | `ForgeRegistry` trait; drop hardcoded allow-list (`lib.rs:2917`) | `ForgeRegistryService` server/client | forge factory by `kind`; card store | `agent-forge`: gitea/bitbucket/… impls |
| C37 transport | `transport_registry.proto` (NEW, additive) | `MessageTransport` seam (recv+post) | `TransportRegistryService` server/client | transport factory by `kind`; card store | `agent-slack` → impl of the seam; matrix/teams/… future crates |
| C38 prompt | none | (uses `PromptStore`, `lib.rs:2302`) | — | `PerTenantPromptStore` wrap; store on C41 | `agent-prompt` onto shared store |
| C39 llm | (existing `upstream.proto`) | — | — | converge `agent-registry` onto C41 + `PerTenant` | `agent-registry` |
| C40 consolidation | none | — | apply C33+C34+C35 to `ConfigService` + all CRUD services | — | portal admin surface (future) |

## Widest blast radius

**C41 is the widest change**: every existing file/sqlite store (`agent-registry`, `agent-prompt`,
`agent-review-fleet`) converges onto one shared backend. Sequence it **first** and behavior-preserving
(same trait, same tests green) before any tenancy layers touch it.

## New dependencies + ops

- **Postgres** — version-pinned in `nix/versions.nix` (alongside ClickHouse); a `nix flake check`
  **DB-integration** harness spins it like the ClickHouse one (`nix/checks/`), applies `sqlx::migrate!`,
  exercises transactions/rollback/conflict, tears down on exit.
- **`sqlx`** — one async SQL code path for sqlite + postgres; compile-time-checked queries where
  practical.
- **JWT/JWKS** — a verify-only dep (e.g. `jsonwebtoken`) for C33; a fake issuer lives in `agent-testkit`.
- All new deps go through `cargo-deny`/`cargo-machete` (the existing static-analysis gate) and are
  feature-gated (`postgres`, `forge-gitea`, `transport-matrix`, …) so a minimal build stays minimal.

## Proto governance

- **New card protos are additive** (`forge_registry.proto`, `transport_registry.proto`, `role.proto`) →
  `buf breaking` passes with **no `buf.image.binpb` bump**. Prefer new protos over editing
  `config.proto`/`upstream.proto` in place.
- Retrofitting an existing config proto (e.g. adding a field to `FleetSession` to reference a
  forge/transport card) is additive too **if** fields are appended with new numbers; a
  removal/renumber would need `nix run .#buf-image` (review-gated). Design the card references as
  additive fields.
- New protos must obey `buf lint` STANDARD (package `agent.v1`, snake_case, enum prefixes, unique
  request/response messages) — the review-fleet/upstream protos are the templates.

## Order of operations (summary)

1. C41 shared store (postgres tier + migrations + DB-integration check); converge existing stores.
2. C33 auth interceptor (`mode=none` default-preserving).
3. C34 RBAC + C35 per-tenant (both on C33; C35 = multi-tenancy C30).
4. C36 forge + C37 transport + C38 prompt cards (on C41; per-tenant via C35).
5. C40 control-plane consolidation + portal admin surface.

Each step: a gated PR off `main`, `nix flake check --max-jobs 8 --cores 4` green, table-driven tests
(four classes + adversarial) + the relevant integration harness, per [`08-testing-and-integration.md`](08-testing-and-integration.md).

## Risks

- **Store convergence regressions** — mitigate by making step 1 behavior-preserving (same trait tests
  pass on the new backend) before any tenancy layering.
- **Auth lockout** — `mode=none` bootstrap preserves today; `oidc` is opt-in per deployment until the
  IdP is wired.
- **Cardinality/secret leakage** — cards hold only `*_ref`s; RBAC labels bounded; identity from verified
  token only.
- **Cross-track drift with multi-tenancy** — C35/C40 must be *the* implementation of C30/C31, not a
  parallel one; coordinate before building (see [`STATUS.md`](STATUS.md) dependencies).
