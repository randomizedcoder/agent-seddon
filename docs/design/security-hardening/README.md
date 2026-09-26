# Security hardening — the P0 closing plan (design of record)

> **Status:** design / pre-implementation, opened 2026-09-26 from the
> [gap analysis](../../gap-analysis/README.md) §10 **P0 — security / tenancy correctness**. Nothing
> in this track is built yet; [`STATUS.md`](STATUS.md) is the tracker and
> [`09-increments.md`](09-increments.md) the build sequence. Every claim below carries a
> `path:line` against `main` `480d7a5` so it can be re-verified.

## Why this exists

The [gap analysis](../../gap-analysis/README.md) found that the multi-tenancy *mechanisms* exist and
are tested, but the shipped default is single-tenant and several of the boundaries are advisory:

| # | P0 finding | Evidence |
|---|---|---|
| 1 | The OIDC verifier is not compiled into `nix build .#agent` (`auth` is an opt-in feature) | [`agent-cli/Cargo.toml`](../../../crates/agent-cli/Cargo.toml):19,27; [`nix/default.nix`](../../../nix/default.nix):109-118 |
| 2 | A verified tenant falls back to the shared `local` tenant when the session header is absent | [`server/mod.rs`](../../../crates/agent-grpc/src/server/mod.rs):172-192; [`identity.rs`](../../../crates/agent-core/src/identity.rs):279-284 |
| 3 | Absent identity is never rejected on stateful RPCs (one exception: `AgentSession.Send`) | [`agent_session.rs`](../../../crates/agent-grpc/src/server/agent_session.rs):145 |
| 4 | Envoy binds `0.0.0.0`, allows every origin, does not allow the `authorization` header, has no `jwt_authn`; the portal hardcodes its identity | [`nix/portal/default.nix`](../../../nix/portal/default.nix):191, 258, 260; [`agent_view_page.dart`](../../../portal/lib/src/pages/agent_view_page.dart):106-109 |
| 5 | No TLS on TCP transports; `https://` is silently downgraded | [`transport.rs`](../../../crates/agent-grpc/src/transport.rs):33-51 |
| 6 | ClickHouse `agent_reader` has no password; users without a row policy read every row | [`schema.sql`](../../../nix/clickhouse/schema.sql):336-340; [`users.xml`](../../../nix/clickhouse/users.xml):25 |
| 7 | `env:` / `file:` credential references resolve against the whole host with no tenant confinement | [`registry.rs`](../../../crates/agent-runtime/src/registry.rs):1357-1376 |

Two more gaps surfaced while designing the fix and are in scope because the P0 items cannot be closed
credibly without them: **authorization covers only mutating control-plane RPCs** (every read, the
interactive agent, live-session observe and served exec are open to any authenticated principal —
[`03-rbac.md`](03-rbac.md)), and **nothing carries a credential between services** (the one outbound
choke point injects trace context and identity headers, never a bearer —
[`client/mod.rs`](../../../crates/agent-grpc/src/client/mod.rs):109-124; [`04-service-integration.md`](04-service-integration.md)).

## The shape in one picture

```
 portal (Flutter web)          CLI (`agent login`)           service (fleet, seam client)
   │ OIDC code+PKCE               │ device code                   │ mTLS cert (step-ca)
   ▼                              ▼                               ▼
 IdP: Google Workspace / Okta / Entra / Keycloak … ──ID token──► AuthService.Exchange ◄── client cert
                                                                    │ resolves tenant, roles, perms
                                                                    │ opens auth_session (Postgres)
                                                                    ▼
                                                     agent-issued JWT (ES256, 15 min, kid from step-ca)
                                                                    │
      Envoy (grpc-web / REST transcoder) ── jwt_authn vs agent JWKS ┤   defense in depth
                                                                    ▼
                                   seam A: AuthLayer verifies → principal + bearer in task scope
                                                │ outbound(): forward bearer + own mTLS identity
                                                ▼
                                   seam B: AuthLayer verifies bearer AND peer cert → records both
                                                │
                                                ▼
                     every auth / authz / binding event → ClickHouse agent_auth_events (tenant RLS)
```

## Decisions

- **D1 — Two token layers.** The IdP proves *who you are* (login); the agent issues *what you may
  do* (the session and inter-service token). External OIDC ID tokens are accepted **only** by
  `AuthService.Exchange`; every other RPC accepts only agent-issued JWTs. This is OAuth token
  exchange (RFC 8693) and it is what makes one verification path serve gRPC, grpc-web, REST and
  seam-to-seam calls alike. [`02-token-service.md`](02-token-service.md).
- **D2 — Login = generic OIDC with per-issuer profiles; Google Workspace is the first profile.**
  Every enterprise IdP speaks OIDC; brokers (Keycloak, Dex, Zitadel) cover SAML and LDAP shops with
  no agent change. [`01-authentication.md`](01-authentication.md).
- **D3 — Roles come from role *bindings* in the config store, plus optional IdP claims, resolved at
  exchange time and embedded in the agent token.** Google ID tokens carry no roles.
  [`03-rbac.md`](03-rbac.md).
- **D4 — Browser login = Authorization Code + PKCE with the code exchange done by the agent.** Google
  web clients require a client secret at the token endpoint, so the exchange cannot live in the
  browser; doing it in the agent keeps the portal gRPC-only (no cookies, no new HTTP surface).
- **D5 — Tenant is derived only from the verified principal; absent identity is rejected per service
  class; `auth` compiles into the default binary; a non-loopback listener refuses `mode = "none"`
  unless explicitly allowed.** [`05-identity-and-tenancy.md`](05-identity-and-tenancy.md).
- **D6 — Machine identity = mTLS certificates from a local CA (smallstep `step-ca`); human identity
  = agent tokens from OIDC login (portal) or device-code login (CLI). No static shared secrets.**
  [`07-transport-tls-and-pki.md`](07-transport-tls-and-pki.md).
- **D7 — Propagation = forward the caller's agent token unchanged on every downstream call, with the
  calling service attested by mTLS.** Delegation is recorded, not re-minted. One cluster audience;
  per-service audiences are a documented follow-up. [`04-service-integration.md`](04-service-integration.md).
- **D8 — Every surface funnels through the one tower `AuthLayer`** (it is an `http::Request` layer,
  not a tonic interceptor): native gRPC, Envoy grpc-web, REST via `grpc_json_transcoder`, and any
  future Rust gateway. Edge `jwt_authn` is defense in depth, never the only check.
- **D9 — Authorization covers every served function, reads included, with least-privilege built-in
  roles for the real personas and a self-escalation-proof permission-management rule.**
- **D10 — Self-contained token, reference-checked for sensitive actions.** Claims carry tenant,
  subject, roles, a permissions snapshot and the session id; 15-minute lifetime; `approve`, `exec`
  and `role` / `binding` / `config` writes also check the session is live.
- **D11 — A session store and an audit stream are needed, not optional.** `auth_sessions` in the
  config store (Postgres in production; `file` / `sqlite` tiers for dev) is what makes logout,
  revocation and refresh real; `agent.agent_auth_events` in ClickHouse, tenant-partitioned under the
  existing ROW POLICY pattern, is the forensic trail.
- **D12 — Data plane: passwords for every ClickHouse user, `users_without_row_policies_can_read_rows
  = false` with an explicit writer policy, per-query tenant setting, and tenant-confined secret
  references.** [`08-data-plane-and-secrets.md`](08-data-plane-and-secrets.md).

## Recommendation summary

| Gap | Decision | Doc | Increment |
|---|---|---|---|
| `auth` feature off in the shipped binary | default feature; fail-closed branch kept under test | 05 | S1 |
| `local` fallback without session header | tenant from `VerifiedPrincipal`; per-class rejection | 05 | S2 |
| absent identity on stateful RPCs | identity policy table + `require_identity` + startup refusal | 05 | S1, S2 |
| enterprise login | OIDC profiles (Google first), exchange in the agent, device flow for the CLI | 01 | S3, S12, S13 |
| one token for every surface and hop | agent-issued JWT signed with a step-ca-issued key; JWKS published | 02 | S5 |
| logout / revocation / "who is signed in" | `auth_sessions` table in the config store | 02 | S6 |
| audit | `agent_auth_events` in ClickHouse with tenant RLS; `doctor` probes | 02 | S11 |
| who may do what | permission matrix, new resources / actions, built-in personas, bindings, escalation rules, read gating, authz-coverage gate | 03 | S7, S8 |
| gRPC / grpc-web / REST / future gateway | one `AuthLayer`; Envoy `jwt_authn` against the agent JWKS | 04, 06 | S14 |
| inter-service chaining | forward the bearer + mTLS peer attestation + hop cap; two-hop test | 04 | S9, S10 |
| Envoy bind / CORS / `authorization` | loopback default, exact origin, header allowed, `jwt_authn` | 06 | S14 |
| TLS / mTLS | tonic `tls`; `[grpc.tls]`; step-ca for dev and integration, bring-your-own CA for production | 07 | S4, S10 |
| ClickHouse credentials and RLS | passwords from files, writer / reader split, per-query tenant setting | 08 | S16 |
| secret references | `SecretScope` confinement under a per-tenant root | 08 | S17 |
| proving it | unit → in-process two-hop → `nix flake check` auth-e2e → `nix run .#integration` (step-ca, Postgres, ClickHouse) | 04 | S15 |

## Threat model (who this track defends against)

- A **browser on the LAN** asserting any tenant through Envoy (gap 2.8): closed by D4, D7, D8 and
  the Envoy hardening.
- A **token-bearing client with no session header** reading and writing the shared `local` tenant
  (gap 2.2): closed by D5.
- **Anyone who can reach ClickHouse** reading every tenant's rows as `agent_reader` (gap 2.4): closed
  by D12.
- A **tenant card** pointing a `file:` reference at another tenant's key or a host file (gap 2.6):
  closed by D12's secret confinement.
- **Plaintext gRPC on the LAN** (gap 2.2): closed by D6.
- An **authenticated low-privilege user** reading the roster, posting a review, watching another
  user's session, or granting themselves a role: closed by D9.
- A **compromised downstream seam** replaying a forwarded token elsewhere: bounded by the 15-minute
  TTL, the single audience, the hop cap and the session reference checks (D7, D10).
- A **stolen refresh handle**: revocable through the session store (D11); reuse of a rotated handle
  revokes the whole session.

**Non-goals for this track:** SCIM provisioning, an org → team → user tier, tenant CRUD and quotas
(P1 in the gap analysis), SAML natively (use a broker), portal admin pages beyond role and binding
editing, attribute-based policies, per-service token audiences, HSM / KMS-backed signing.

## Relationship to other tracks

- [`config/02-auth-and-rbac.md`](../config/02-auth-and-rbac.md) (C33 / C34) built the verifier, the
  RBAC core and role cards this track extends; its C34 role model is superseded by
  [`03-rbac.md`](03-rbac.md).
- [`multi-session/07-security.md`](../multi-session/07-security.md) named the auth follow-up and the
  "fail closed on absent identity" rule that [`05-identity-and-tenancy.md`](05-identity-and-tenancy.md)
  finally lands.
- [`multi-tenancy/02-data-scoping-and-rls.md`](../multi-tenancy/02-data-scoping-and-rls.md) designed
  the writer / reader split that [`08-data-plane-and-secrets.md`](08-data-plane-and-secrets.md) completes.
- The [gap analysis §4](../../gap-analysis/README.md#4-api-surface-protobuf--openapi--rest) REST
  plan (Envoy `grpc_json_transcoder`) is a separate track; [`04-service-integration.md`](04-service-integration.md)
  shows that auth needs no change when it lands.
- [`parity/50-secret-store.md`](../../parity/50-secret-store.md) is the secret-store seam this
  track's confinement is shaped to become.
- The [doctor track](../doctor/README.md) `Probe` seam gains the PKI / JWKS / IdP / session-store probes.

## Build order

`S1 → S2` (correctness, no new surface) · `S3 → S5 → S6 → S8` (login → token → sessions → bindings)
· `S7` (RBAC model, after S2) · `S4 → S10` (PKI, independently) · `S9` (propagation, after S5) ·
`S11` (audit, after S6 + S8) · `S12 / S13 → S14` (CLI, portal, Envoy) · `S15` (the end-to-end
gate, last) · `S16`, `S17` (data plane, independently). Details and dependencies in
[`09-increments.md`](09-increments.md).
