# 09 — Increments

Each increment is one PR off `main`, gated by `nix flake check`, with four-class + `adversarial_`
tables. Tracker: [`STATUS.md`](STATUS.md).

| # | Increment | Closes | Depends |
|---|---|---|---|
| S1 | `auth` in default features; `nix/checks/auth.nix` repointed to `--no-default-features`; `AuthCfg` validated at load; startup refusal for `mode = "none"` on a non-loopback listener (`allow_insecure_listen`) | P0-1, part of P0-3 | — |
| S2 | Tenant from principal; convert the five direct `.user` readers; `identity_policy.rs` with per-class rejection; `require_identity`; mt-audit sub-check 5; harness helpers (`dial_for`, ghz, fleet-e2e, `scoped_request`) | P0-2, P0-3 | S1 |
| S3 | Multi-issuer login verifier with profiles (`google`, `entra`, `generic`), domain / email rules, single-issuer back-compat; testkit fake OIDC issuer | D2 | S1 |
| S4 | PKI: tonic `tls` feature, `Endpoint` scheme, `[grpc.tls]`, `nix run .#pki-dev` (step-cli offline), transport-matrix `tls` / `mtls` rows | P0-5 | — |
| S5 | **Token service core:** agent JWT mint / verify (claims, ES256, `kid`, rotation grace), `AuthService.Jwks/WhoAmI`, `[auth.token]`, seams accept only agent tokens; `AGENT_BEARER` + `scope_request` | D1, D10 | S3 |
| S6 | Session store: `0003_auth_sessions` migration (file / sqlite / postgres), open / refresh / revoke / GC, `Exchange/Refresh/Logout/ListSessions/RevokeSession`, refresh-handle rotation, `sid` reference checks | D11 | S5 |
| S7 | RBAC extension: new actions / resources, built-in role set, `Approve` on `review`, gate every read and the ungated surfaces, `observe` ownership, mt-audit sub-check 6 (`authz.toml`) | D9 | S2 |
| S8 | `RoleBinding` card + RPCs, exchange-time resolution, `operator_subjects`, escalation and lockout rules, revoke-on-change | D3, D9 | S6, S7 |
| S9 | Propagation: `outbound()` forwards `AGENT_BEARER` else the `BearerSource` service token; `x-agent-hops`; spawn-site re-scope + grep gate; two-hop in-process chain test | D7 | S5 |
| S10 | mTLS service identity: `PeerVerifier`, `[auth.mtls] bindings`, `Exchange{client_cert}` service tokens with `cnf`, non-loopback plaintext refusal | D6 (machine) | S4, S5, S9 |
| S11 | Audit stream: `agent_auth_events` table + policy + writer hooks for every auth / authz / binding event; `doctor` probes (signer cert, JWKS, IdP discovery, session store) | D11 | S6, S8 |
| S12 | CLI: `BearerSource` token file, `agent login/logout/whoami` (device flow → `Exchange`), `[grpc.client] bearer` | D6 (human) | S6 |
| S13 | Portal login: `AuthState`, `AuthInterceptor`, `LoginPage`, callback, capability-aware controls, Layer-A fakes | P0-4 (portal) | S6, S8 |
| S14 | Envoy hardening: loopback bind, exact-origin CORS, `authorization`, `jwt_authn` against the agent JWKS, `PORTAL_AUTH=off`, `--mode validate` check, portal-e2e under auth, optional TLS / mTLS contexts | P0-4 | S5, S13 |
| S15 | `nix flake check` **auth-e2e** (fake issuer + real `--serve-all` + step-cli certs + two-tenant isolation + chain through a `= "grpc"` seam) and the `nix run .#integration` step-ca / Postgres / ClickHouse tiers | testing | S9, S10, S11 |
| S16 | ClickHouse lockdown: passwords from files, `users_without_row_policies_can_read_rows = false`, writer / reader split, per-query tenant setting, HyperDX wiring, RLS harness | P0-6 | — (S11 adds its table) |
| S17 | Secret-reference confinement: `SecretScope`, `[secrets]`, inline refusal under `per_tenant` | P0-7 | — |

## Lanes

```
S1 → S2 → S7 ─────────────┐
S1 → S3 → S5 → S6 → S8 ───┼→ S11 ─┐
          S5 → S9 ─────────┼───────┼→ S15
S4 ──────────→ S10 ────────┘       │
S6 → S12                            │
S6, S8 → S13 → S14 ─────────────────┘
S16, S17 independent
```

Four lanes can run in parallel after S1: correctness (S2, S7), login / token / sessions (S3 → S8),
PKI (S4 → S10), data plane (S16, S17). S15 is last: it is the end-to-end gate the whole track is
judged by.

## REST

REST ([gap analysis §4](../../gap-analysis/README.md#4-api-surface-protobuf--openapi--rest)) is a
separate track. When it lands it adds `google.api.http` annotations for `AuthService` (so
`/auth/*` and `/.well-known/jwks.json` exist) and one transcoder row to the S15 harness. Nothing in
`AuthLayer` changes ([04](04-service-integration.md)).

## Docs touched per increment

- `docs/grpc.md`: metadata, TLS and bearer sections (S4, S5, S9).
- `docs/components/mt-audit.md`: sub-checks 5 and 6 (S2, S7).
- `config/agent.toml` examples: `[auth]`, `[auth.token]`, `[auth.mtls]`, `[grpc.tls]`, `[secrets]`,
  `[telemetry]` (each owning increment).
- `docs/design/config/02-auth-and-rbac.md`: link; its C34 role section is superseded by
  [`03-rbac.md`](03-rbac.md) (S7).
- `docs/design/portal/STATUS.md`: the mTLS follow-up resolved (S14).
- `nix/clickhouse/schema.sql` comments (S11, S16).
- `docs/gap-analysis/README.md` §10: tick each P0 as it closes.

## Definition of done (every increment)

- `nix flake check --max-jobs 8 --cores 4` green, including the new gate the increment adds.
- Four-class tables plus `adversarial_` for every untrusted input the increment touches.
- The owning doc in this track updated to match what was built (as-built notes in
  [`STATUS.md`](STATUS.md)); no doc claims something the code does not do.
- No secret material in the tree: keys and certificates are generated at build or test time.
