# Security hardening — status

Legend: ✅ merged · 🟡 in progress · ⬜ not started · ❌ dropped

Design: [`README.md`](README.md) · sequence: [`09-increments.md`](09-increments.md) · source:
[gap analysis §10 P0](../../gap-analysis/README.md).

| # | Increment | Closes | State | PR |
|---|---|---|---|---|
| S1 | `auth` default feature, load-time validation, insecure-listen refusal | P0-1, P0-3 | 🟡 | — |
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
- **2026-09-26 — S1.** `auth` joins `agent-cli`'s default features, so the shipped binary can run
  `[auth] mode = "oidc"`. `AuthCfg::validate` runs inside `parse_config`: unknown mode, missing
  `issuer`/`audience`/`jwks_url`, a `jwks_url` that is not https (plain http only to a numeric
  loopback IP; embedded credentials refused), `leeway_secs` > 300, and `oidc` in a build without
  the verifier all fail at load (so the portal's config edits are refused too). New
  `listen_posture` in `agent-grpc/src/server/auth.rs` runs before every served listener binds:
  `mode = "none"` on anything but a numeric loopback IP or a unix socket is a startup error
  unless `[auth] allow_insecure_listen = true`, which warns on every start. `localhost` and
  `::ffff:127.0.0.1` count as remote. `nix/checks/auth.nix` now runs `-p agent-grpc --lib`
  without the feature, so the fail-closed branch is executed in the gate; the workspace `test`
  check runs the verifier matrix. Not in S1: `require_identity` and the per-class rejection
  (S2).
  Gate: `nix flake check` green (the `leak` fork/cancel and `coverage` pty-firehose timing
  flakes, both outside S1's code, passed on rerun).
