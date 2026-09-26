# Security hardening — status

Legend: ✅ merged · 🟡 in progress · ⬜ not started · ❌ dropped

Design: [`README.md`](README.md) · sequence: [`09-increments.md`](09-increments.md) · source:
[gap analysis §10 P0](../../gap-analysis/README.md).

| # | Increment | Closes | State | PR |
|---|---|---|---|---|
| S1 | `auth` default feature, load-time validation, insecure-listen refusal | P0-1, P0-3 | ⬜ | — |
| S2 | Tenant from principal, identity policy, direct-reader conversion | P0-2, P0-3 | ⬜ | — |
| S3 | Multi-issuer OIDC profiles + fake issuer | D2 | ⬜ | — |
| S4 | tonic TLS, `[grpc.tls]`, `nix run .#pki-dev` | P0-5 | ⬜ | — |
| S5 | Token service core (agent JWT, JWKS, `WhoAmI`) | D1, D10 | ⬜ | — |
| S6 | Session store + `Exchange/Refresh/Logout` | D11 | ⬜ | — |
| S7 | RBAC extension, read gating, authz-coverage gate | D9 | ⬜ | — |
| S8 | Role bindings, bootstrap, escalation rules | D3, D9 | ⬜ | — |
| S9 | Bearer propagation + two-hop chain test | D7 | ⬜ | — |
| S10 | mTLS service identity | D6 | ⬜ | — |
| S11 | `agent_auth_events` audit + doctor probes | D11 | ⬜ | — |
| S12 | CLI `agent login/logout/whoami` | D6 | ⬜ | — |
| S13 | Portal login + capability-aware UI | P0-4 | ⬜ | — |
| S14 | Envoy hardening + `jwt_authn` | P0-4 | ⬜ | — |
| S15 | auth-e2e gate + integration tiers | testing | ⬜ | — |
| S16 | ClickHouse credentials + RLS lockdown | P0-6 | ⬜ | — |
| S17 | Secret-reference confinement | P0-7 | ⬜ | — |

## As-built log

- **2026-09-26** — track opened from the gap analysis §10 P0 list (merged in #482). Design review
  questions recorded and answered in the docs: enterprise login (Google OAuth2 then a JWT →
  [01](01-authentication.md), [02](02-token-service.md)); the RBAC persona model
  ([03](03-rbac.md)); gRPC / REST integration, inter-service credential chaining and how to test
  it ([04](04-service-integration.md)); a local CA (smallstep) minting signed JWTs, adopted as
  step-ca PKI + agent-issued tokens ([02](02-token-service.md), [07](07-transport-tls-and-pki.md));
  a session store in Postgres and an auth-event stream into ClickHouse, both adopted
  ([02](02-token-service.md)). Nothing built yet.
