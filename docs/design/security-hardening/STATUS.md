# Security hardening — status

Legend: ✅ merged · 🟡 in progress · ⬜ not started · ❌ dropped

Design: [`README.md`](README.md) · sequence: [`09-increments.md`](09-increments.md) · source:
[gap analysis §10 P0](../../gap-analysis/README.md).

| # | Increment | Closes | State | PR |
|---|---|---|---|---|
| S1 | `auth` default feature, load-time validation, insecure-listen refusal | P0-1, P0-3 | ✅ | #487 |
| S2 | Tenant from principal, identity policy, direct-reader conversion | P0-2, P0-3 | ✅ | #489 |
| S3 | Multi-issuer OIDC profiles + fake issuer | D2 | ✅ | #492 |
| S4 | tonic TLS, `[grpc.tls]`, `nix run .#pki-dev` | P0-5 | ✅ | #494 |
| S5 | Token service core (agent JWT, JWKS, `WhoAmI`) | D1, D10 | 🟡 | — |
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
- **2026-09-26 — S1 (#487).** `auth` joins `agent-cli`'s default features, so the shipped binary can run
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
- **2026-09-26 — S2 (#489).** `agent_core::current_tenant()` now prefers the verified principal's tenant
  (new `scoped_tenant()` is the `Option` form for callers that must fail closed), so a token with
  no session header runs as its own tenant, never `local`. The direct readers now use it: memory
  `PerUserMemory`, the ClickHouse reader's `SET SQL_tenant_id`, the metrics `ambient_tenant`
  label, the digest `Query` filter (via `server::request_tenant`) and the distiller's alternatives
  rows. The ConfigService tenant-write guard already defers to the principal and is unchanged.
  New `crates/agent-grpc/src/server/identity_policy.rs`: `class_of` (closed match over all 39
  services, the mt-audit classes), `service_of(path)`, and `admit`. The auth layer calls `admit`
  after verifying a token, and without a verifier when `require_identity` is on: `scoped` and
  `single-store` services without a valid session get `UNAUTHENTICATED("identity required")`, an
  unclassified service gets `PERMISSION_DENIED`. `[auth] require_identity` (`Option<bool>`)
  defaults per listener to on for routable addresses and off for loopback and unix sockets.
  mt-audit sub-check 5 `identity-policy` parses `class_of` and fails on any difference from the
  manifest; a Rust test asserts every service in `method_paths()` has a class. Not changed: the
  auth layer still *rewrites* `x-agent-user-id` rather than ignoring it (equivalent, since the
  verified tenant wins); the harness helpers (`dial_for` headers, ghz `-m`, fleet-e2e
  `DIAL_FLAGS`, a Rust `scoped_request()`) are not needed while every harness listens on
  loopback, so they move to S15, where the strict path first runs.
  Gate: `nix flake check` green (the `leak` check's `tools_do_not_leak` window-2 timing flake,
  outside S2's code, passed on rerun).
- **2026-09-26 — S3 (#492).** `[[auth.issuers]]` adds any number of OIDC issuers beside the single-issuer
  form, which now resolves to one `generic` issuer named `default` that keeps trusting its roles
  claim (existing configs verify exactly as before). New
  `crates/agent-grpc/src/server/auth/issuer.rs`: `ResolvedIssuer::resolve` applies the `google` /
  `entra` / `generic` profile defaults (accepted `iss`, JWKS or discovery, claim map) and refuses
  unusable combinations (Google with no `allowed_domains` and no `default_tenant`, Entra with no
  `allowed_tenants`, profile-fixed claims overridden, roles trusted from Google, unsafe names,
  domains or default tenant, plain-http or credentialed key URLs); `identity()` maps verified
  claims to tenant / subject / roles / email per profile. The JWT verifier moved to
  `auth/jwt.rs`: one `JwtVerifier` per issuer (own key cache), `DiscoveryJwks` (the discovery
  document must name the configured issuer and its `jwks_uri` must be https or loopback), and
  `MultiIssuerVerifier`, which routes by the unverified `iss` (unknown, missing or array `iss`
  rejected before any fetch) and refuses duplicate names or overlapping `iss` at startup.
  `VerifiedIdentity` gained `issuer` and `email` (for S5's exchange). Config: `AuthIssuerCfg`
  (`client_id` is an alias of `audience`) with `deny_unknown_fields`, because a misspelt
  restriction would otherwise widen who may sign in; checked at load. Fake issuer:
  `agent_testkit::oidc` (feature `oidc`) serves discovery + JWKS on loopback via `tiny_http`,
  with two fixed keys (the existing RSA fixture moved here from `auth/tests.rs`, plus a P-256
  key) so two issuers can have different keys. Deferred, with the increments that use them:
  `operator_subjects` bootstrap (S8); the fake issuer's token and device endpoints, `state`
  expiry / replay and PKCE cases (S6, S12); "an IdP token presented to a seam is rejected"
  (S5, when seams accept only agent tokens). Subjects are not yet namespaced by issuer; the
  agent token's `sub = user:<issuer>/<sub>` (S5) does that.
  Gate: `nix flake check --max-jobs 8 --cores 4` green (`leak`'s `fork_cancel_cycle_does_not_leak`
  in `agent-providers`, untouched here, flaked once and passed on rerun).
- **2026-09-26 — S4 (#494).** tonic's `tls` + `tls-webpki-roots` features are on in `agent-grpc` (rustls
  with `ring`; the lock still has no aws-lc). `Endpoint::Tcp` is now `{ hostport, tls }`:
  `https://` dials TLS, while `http://` and bare `host:port` stay plaintext. Before S4,
  `https://` was stripped and dialed plaintext. New `crates/agent-grpc/src/tls.rs`:
  - `ServerTls::load(cert, key, client_ca)`; a client CA makes the listener mutual.
  - `ClientTls::load(ca, cert, key, domain)`: a configured CA is the only trust anchor, else
    the webpki roots.
  - PEM files are capped at 1 MiB, must contain a `-----BEGIN` block, and are validated by
    building the rustls config at load. A group- or world-readable key logs a warning.
  - The client side is installed process-wide with `set_client_tls` (replace semantics), so
    the ~50 `connect_lazy()` callers are unchanged; `connect_lazy_with` takes it explicitly.

  `base_router_with_tls` seeds the router with TLS. The CLI's four serve entry points now share
  `serve_base`, which applies TLS to TCP listeners only (a unix socket stays plaintext), refuses
  an `https://` listen address with no cert, and logs `transport = plaintext | tls | mtls`.
  Config (`[grpc.tls]`, `[grpc.tls.client]`, both `deny_unknown_fields`):
  - `GrpcTlsCfg::validate` runs at load: cert and key must be set together, `client_ca`
    needs a server cert, and the domain must be a hostname or an IP.
  - Client TLS is installed early in `build_agent_with`.
  - Values are plain paths, not `file:` references (doc 07 updated).

  `nix run .#pki-dev` (`test/pki-dev/pki_dev.py`, stdlib python driving `step certificate
  create` offline) mints the root CA, a `token-signer`, and per-service leaves (SANs
  localhost / 127.0.0.1 / ::1 / name / `spiffe://agent.<deployment>/svc/<name>`, EKU
  server + client). It is idempotent, `--force` never deletes the directory, `--verify` checks
  every leaf, and it prints the `[grpc.tls]` block. The new `pki-dev-tests` check runs its
  four-class tables plus a real step-cli mint, verify and check-the-checks (a foreign-CA leaf
  and a corrupt cert must fail) offline in the sandbox.

  Tests:
  - `crates/agent-grpc/tests/tls.rs` is a 16-case handshake matrix on `127.0.0.1:0`, using
    the new `agent_testkit::pki` (rcgen, in-memory CA). It covers TLS, mTLS and domain
    override; expired, not-yet-valid, other-CA and wrong-name server certs; a foreign or
    missing client cert; plaintext against TLS and TLS against plaintext; web roots against
    a private CA; bare `host:port` staying plaintext; and UDS unaffected.
  - Parse, PEM, domain and config-load tables.
  - `serve-smoke` gains `tls` and `mtls` rows (step-cli certs, grpcurl). Both must also refuse
    a plaintext client, and mTLS must refuse a client without a certificate. All four
    transports pass. This also fixed a pre-existing break: since #305, backticks in
    serve-smoke's agent.toml heredoc comments failed shellcheck, so the app did not build.

  Deferred:
  - Peer SAN → service principal, `[auth.mtls]` bindings, and refusing plaintext on
    non-loopback listeners: S10.
  - The `step-ca` daemon (`nix run .#step-ca`) and `renew_cmd` / `step ca renew`: integration
    tier, S15.
  - The signer-certificate expiry `doctor` probe: S11.
  - Hot reload of certificates: needs a restart today.
  Gate: `nix flake check --max-jobs 8 --cores 4` green (`leak`'s `fork_cancel_cycle_does_not_leak`
  in `agent-providers`, untouched here, flaked once and passed on rerun).
- **2026-09-26 — S5 (in progress).** The agent issues its own tokens. New
  `crates/agent-grpc/src/server/auth/token.rs`: `TokenService` loads a P-256 signing key (PKCS#8,
  or SEC1 as `step-cli` and `nix run .#pki-dev` write it; 64 KiB cap, warns when group/other can
  read it), mints ES256 tokens with header `typ = at+jwt` and `kid` = RFC 7638 thumbprint, and
  verifies them. The key may be the current `signing_key` or the `previous_key` kept after a
  rotation. Claims: `iss`, `aud`, `sub = user:<issuer>/<sub>`, `tenant`, `email`, `amr`, `roles`,
  `perms` (`"action:resource"` from new `agent_core::effective_permissions`; left out with
  `perms_ref = true` past 40), `iat`/`nbf`/`exp`, `jti`. `exp` is the earlier of `ttl_secs`
  (default 900, 60..=3600) and the login token's own `exp`.

  New `AuthService` (`auth.proto`, `auth/service.rs`):
  - `Exchange{id_token}` verifies a login token with the S3 issuers and returns an agent token.
    It was pulled forward from S6 in stateless form: no `sid`, no refresh.
  - `Jwks` returns the public key set.
  - `WhoAmI` returns the caller's verified claims.

  With `[auth.token]` set:
  - The `AuthLayer` verifies only agent tokens at every seam and serves `AuthService` on the
    listener (`serve_auth_service`, called from `serve_base`).
  - `Exchange` and `Jwks` are exempt from the bearer check.
  - Without `[auth.token]`, `oidc` keeps the S1–S3 direct IdP verification and warns.
  - The verified bearer is scoped beside the principal (`agent_core::AGENT_BEARER`), and
    `agent_core::scope_request` re-installs identity, principal and bearer across a spawn (S9
    uses both to forward).

  Config: `[auth.token]` (`AuthTokenCfg`, unknown keys rejected) is validated at load:
  - it needs `mode = "oidc"`, `issuer`, `audience` and `signing_key`;
  - `ttl_secs` is bounded;
  - `previous_key` must be a different file;
  - the agent `issuer` must not be a login issuer's `iss`.

  `AuthService` is class `stateless` in `identity_policy.rs` and the mt-audit manifest. The
  audit now scans `server/**/*.rs`, not only the top level, so a handler in a submodule cannot
  escape it.

  Tests:
  - Token tables: key formats and refusals (RSA, P-384, encrypted, junk); service bounds; mint
    and verify; expiry capping; leeway; rotation grace; the 40/41-permission boundary; JWKS
    usable by another verifier.
  - Adversarial tokens: an IdP token, a missing or wrong `typ`, RS256, an unknown `kid`, a
    foreign `iss` or `aud`, a traversal tenant, tampering, and `alg:none`.
  - Layer `from_params` and exemption / bearer-scope tables.
  - `tests/auth_token.rs`, over a real server with a `FakeIssuer`:
    Exchange → WhoAmI → seam call; an IdP token at a seam and an agent token at `Exchange` both
    rejected; the 16 KiB `id_token` cap.

  Deferred:
  - Sessions, refresh, logout and `sid` checks: S6.
  - Verify-only processes reading a remote JWKS: S9.
  - Signer-certificate expiry probe: S11.
  - `agent login`: S12.
  - Key hot-reload: needs a restart.

  Gate: `nix flake check --max-jobs 8 --cores 4 --keep-going` green (all checks passed).
