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
| S5 | Token service core (agent JWT, JWKS, `WhoAmI`) | D1, D10 | ✅ | #498 |
| S6 | Session store + `Exchange/Refresh/Logout` | D11 | ✅ | #505 |
| S7 | RBAC extension, read gating, authz-coverage gate | D9 | ✅ | #504 |
| S8 | Role bindings, bootstrap, escalation rules | D3, D9 | ✅ | #511 |
| S9 | Bearer propagation + two-hop chain test | D7 | ✅ | #514 |
| S10 | mTLS service identity | D6 | ✅ | #516 |
| S11a | `agent_auth_events` audit stream | D11 | ✅ | #518 |
| S11b | `doctor` auth probes (signer, JWKS, IdP discovery, session store) | D11 | ✅ | #521 |
| S12 | CLI `agent login/logout/whoami` | D6 | ✅ | #524 |
| S13a | Browser sign-in server side (`Issuers` / `Begin` / code + PKCE `Exchange`) | P0-4 | ✅ | #528 |
| S13b | Portal login + capability-aware UI | P0-4 | ✅ | #533 |
| S14 | Envoy hardening + `jwt_authn` | P0-4 | 🟡 | — |
| S15 | auth-e2e gate + integration tiers | testing | ⬜ | — |
| S16 | ClickHouse credentials + RLS lockdown | P0-6 | ✅ | #506 |
| S17 | Secret-reference confinement | P0-7 | ✅ | #507 |

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
- **2026-09-26 — S5 (#498).** The agent issues its own tokens. New
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
- **2026-09-26 — S7 (#504).** Every RPC is authorized, reads included.

  Model (`agent_core::rbac`):
  - `Action` gains `use` and `observe`. `ResourceType` gains `agent`, `exec`, `review`, `binding`
    and `telemetry`, all tenant-owned. `Config` stays the only operator-global resource.
  - The built-in roles are those of [03](03-rbac.md): `viewer` (alias `reader`),
    `review_viewer`, `agent_user`, `reviewer`, `fleet_admin`, `access_admin`, `svc_fleet`,
    `svc_seam`, `org_admin`, `operator`. All of their ids are reserved.
  - `Approve` moves from `fleet` to `review`, and `UpdateReview` is `(write, review)`. A stored
    card that grants `approve:fleet` also grants `approve:review`, so existing approver cards
    keep working.

  Enforcement:
  - New `crates/agent-grpc/src/server/authz_policy.rs`: `gate_of(service, method)` is a closed
    match giving each of the 160 RPCs a gate. The `AuthLayer` enforces it after the identity
    policy (`authz::gate`), and an RPC with no row is denied.
  - The decision counter (`AuthzObserver`) now ticks there, once per call. Handler
    `authz::require` stays on the mutating RPCs as defense-in-depth and records the span fields.
  - The agent's own seams (context, provider, tokenizer, tools, repo, memory, search, forge,
    tasks, `ProviderRegistry.Route`, digest) are `(use, agent)`; Sandbox and Pty are
    `(use, exec)`; the metrics proxy is `(read, telemetry)`.

  Session ownership (`agent_session.rs`):
  - `SessionSource::owner()` is new, and the runtime sink records the first verified caller to
    `Send` (first write wins).
  - A non-owner's `Subscribe`/`Snapshot` needs `(observe, agent)` in the owner's tenant; on an
    unowned session, in their own. A non-owner's `Send` is denied.

  mt-audit sub-check 6, **authz-coverage**: `gate_of` must equal the committed
  `test/mt-audit/authz.toml` row by row, and each handler `require` must name its row's
  permission (`--dump-authz` renders the table).

  Deviations from the design:
  - The gate is one table in the layer rather than a `require` in every handler, and the audit
    parses that table.
  - Ownership is recorded at `Send`, not `SessionRegistry.Open`, because the live event source
    is created there.
  - `DigestService` is `(use, agent)` rather than `(read, telemetry)`, because the agent loop
    writes and reads it.

  Behaviour changes under `oidc`:
  - `reader`/`viewer` no longer reach the agent's seams (no `(use, agent)`).
  - `ActionsOnAll` cards also cover the new resources.
  - Admin tokens carry `perms_ref` because they exceed 40 permissions (`org_admin` 104,
    `operator` 112).

  Tests:
  - Role grant and nesting tables, and the legacy `approve:fleet` mapping.
  - The `gate_of` path table, and a test that every RPC in `method_paths()` has a gate.
  - A persona × RPC table through `gate`, and the observer counting once.
  - `require_in` tenant cases, the session-ownership table, and first-owner-wins on the sink.
  - `tests/auth_token.rs`: a `viewer` token is refused a seam but reaches `WhoAmI`.
  - mt-audit check-the-checks for every finding kind.

  Deferred:
  - Role bindings, bootstrap operators and the escalation rules: S8.
  - Registry reads by the runtime under a forwarded user token (`agent_user` has no
    `(read, registry)`): S9.
  - Dropping unknown pairs from stored cards at load: old cards still decode, because only
    variants were added.

  Gate: `nix flake check --max-jobs 8 --cores 4 --keep-going` green (2026-09-26).

- **2026-09-26 — S6 (#505).** Sign-in sessions. Logout and revocation now mean something.

  Store: new [`auth/session.rs`](../../../crates/agent-grpc/src/server/auth/session.rs).
  - `SessionStore` over any `agent-config-store` `Backend`. Each session is a JSON card in the
    `auth_sessions` collection, keyed `(tenant, sid)`, capped at 4096 per tenant.
  - Absolute lifetime `session_ttl_secs` (default 12 h, 900 s..=30 d).
  - Refresh handle `rh1.<tenant>.<sid>.<secret>`; only the secret's SHA-256 is stored.
  - Rotation is a `CompareAndSwap`, so a race has exactly one winner.
  - Reuse of a retired handle revokes the session. A forged secret is only refused.
  - `is_live` has a 5 s per-process cache and fails closed.

  Tokens:
  - Agent tokens carry a required `sid`, which `safe_segment` checks when the token is verified.
  - `mint` takes a `Grant`. Its expiry is capped at the session's end, as well as at the login
    token's.

  `AuthService`:
  - `Exchange` opens a session and returns `refresh_handle` and `session_expires_at`.
  - New RPCs: `Refresh` (public, handle ≤ 1 KiB), `Logout`, `ListMySessions` and
    `RevokeMySession` (for the signed-in user), and `ListSessions` / `RevokeSession`
    (`read`/`write:binding`; another tenant needs a host-global grant).
  - `WhoAmI` returns `sid`.

  Layer:
  - A sensitive RPC also needs a live session. `authz_policy::is_sensitive` covers any
    `approve`, `(use, exec)`, and writes or deletes of `role`, `binding` and `config`.
  - A denial is `UNAUTHENTICATED`.

  Config: `[auth.token] session_store` (`memory` | `file` | `postgres`), `session_path`,
  `session_ttl_secs` and `max_sessions_per_tenant`, validated at load. A new `auth-postgres`
  runtime feature is part of the `postgres` umbrella.

  Deviations from 02: listed under "As built (S6)" in
  [02-token-service.md](02-token-service.md#session-store-d11).
  - A card collection instead of a `0003` migration.
  - Opportunistic GC instead of a scheduled job.
  - A roles snapshot until S8.
  - No IdP refresh token or revalidation yet.

  Tests:
  - Session store table, about 25 cases: rotation, reuse, forged secret, race, expiry,
    malformed-handle adversarial table, swapped tenant, tampered row, cache window, tenant
    scoping, GC, retired-list cap.
  - Token `sid` adversarial cases and the `is_sensitive` table.
  - Config and resolver tables.
  - `tests/auth_token.rs` on the wire: exchange opens a session; refresh rotates without a
    bearer; a reused handle kills the session; logout stops `Approve`
    (`UNIMPLEMENTED` → `UNAUTHENTICATED`) while `WhoAmI` still works; revoke-my-other-session;
    another subject's session is invisible; `ListSessions` needs `read:binding`; bad handles are
    rejected.
  - mt-audit `authz.toml` has the six new rows.

  Gate: `nix flake check --max-jobs 8 --cores 4 --keep-going` green (2026-09-26). The `leak` check's
  `agent-memory` `summarize_step_does_not_leak` failed once on live-block timing (18 → 30) and
  passed when rebuilt alone. S6 does not touch that crate.

- **2026-09-27 — S16 (#506, ClickHouse lockdown).** Every ClickHouse login now has a password, and each
  tenant-scoped read binds its tenant. See [08 "As built (S16)"](08-data-plane-and-secrets.md#as-built-s16).
  - Logins: admin (`default`), `agent_writer`, `agent_reader`, `agent_viewer`. The SQL users are
    created `HOST NONE`, and `clickhouse-up` sets their passwords as SHA-256 hashes from 0600
    files generated by `test/clickhouse/ch_creds.py` (`nix run .#clickhouse-creds`).
  - `users_without_row_policies_can_read_rows = false` now lives in a `config.d` override. Its C27
    home in `users.d` was silently ignored, which the new harness caught.
  - The reader binds `SQL_tenant_id` before every read instead of once per cached connection. A
    connection first used by tenant A had been serving tenant B's reads under A's scope; a
    scoped read with no tenant is now refused.
  - `[telemetry] password_file` / `reader_password_file`, `user` defaulting to `agent_writer`,
    load-time credential validation, `reader_password` masked by the config service.
  - HyperDX, Grafana, portal-e2e, fleet-measure and graph-arena log in with the right login.
  - `clickhouse-up` refuses a pre-S16 container rather than discarding its telemetry.

  Tests:
  - `ch-creds-tests` gate check: 34 cases, including check-the-checks for the harness's matcher.
  - `read_scope` table; telemetry `validate` and password-file tables; a doctor probe case.
  - `nix run .#ch-integration`: a 24-row live matrix, plus the Rust shared-connection test. That
    test failed against the pre-S16 binding and passes now. Run on l2 with podman.

  Upgrade: recreate the agent ClickHouse (`nix run .#clickhouse-down && nix run .#clickhouse-up`,
  which discards its telemetry), then point `[telemetry] password_file` at
  `nix run .#clickhouse-creds -- path writer`. HyperDX and Grafana containers need recreating to
  pick up their logins. HyperDX keeps its ClickHouse connection in Mongo from the first sign-up,
  so an existing install also needs that connection switched to `agent_viewer` in the UI (or a
  fresh `hyperdx-down -- --volumes`).
  Gate: `nix flake check --max-jobs 8 --cores 4 --keep-going` green (2026-09-27), first run.

- **2026-09-27 — S17 (#507).** Secret references from tenants are confined (P0-7).

  New [`agent-runtime/src/secrets.rs`](../../../crates/agent-runtime/src/secrets.rs):
  - `SecretScope` (`Operator` | `Tenant`) and a process `SecretsPolicy` installed by the builder.
  - Under `[tenancy] per_tenant = true`, a tenant's `file:` reference resolves only inside
    `[secrets] root/<tenant>/`, through `agent_core::confine`, so symlinks cannot lead out.
  - A tenant's `env:` reference is refused unless `[secrets] allow_env_for_tenants`.
  - Errors name the rule, never the path or variable.

  Call sites:
  - Fleet rows and forge cards (`resolve_tenant_token_ref(row.user, …)`).
  - Transport bot tokens in progress posts, and app tokens in the Slack watch; the watch groups
    by `(owner, ref)`.
  - Provider upstream cards (`synth_key_refs` under `current_tenant()`).
  - Operator config keeps the old behaviour. `resolve_token_ref` is now the operator form, and
    reads through the same module.

  Config: `[secrets] root` (default `$XDG_CONFIG_HOME/agent-seddon/secrets`) and
  `allow_env_for_tenants`. Unknown keys are refused.

  Deviations from 08:
  - Inline-key refusal needed no change, because every card path already accepts only the
    `ApiKeyRef` grammar.
  - Confinement is keyed on `per_tenant` rather than on `tenant != local`.

  Tests:
  - A secrets table: relative and absolute inside, missing file, host path, `/etc/passwd`, `..`,
    another tenant's directory, a symlink to another tenant, a symlink escape, a dangling link,
    the directory itself, and a raw secret. Each rejection asserts the path is not echoed.
  - `env:` refused by default and allowed when on; bad tenant segments rejected.
  - Operator and per-tenant-off keep legacy resolution; an unset `env:` is absent.
  - `synth_key_refs_with` (seven provider-card cases) and `[secrets]` config parsing.
  Gate: `nix flake check --max-jobs 8 --cores 4 --keep-going` green (2026-09-27); `leak`'s
  `fork_cancel_cycle_does_not_leak` (agent-providers, untouched here) flaked once and passed on
  rebuild.

- **2026-09-27 — S8 (#511).** Role bindings. Roles now come from the config store, not
  only from the IdP.

  Bindings: new [`auth/binding.rs`](../../../crates/agent-grpc/src/server/auth/binding.rs).
  - `RoleBinding {id, tenant, kind: sub|email|domain|mtls_san, subject, roles, granted_by,
    granted_at, expires_at}` is a card in collection `role_bindings`, on the session store's
    backend. It is keyed by tenant and capped at 1024 per tenant and 32 roles per binding.
  - Subjects are validated per kind: an `<issuer>/<sub>` pair, an email, or a domain.
    Emails and domains are trimmed and lowercased; control characters and whitespace are refused.
  - Resolution happens at `Exchange` and every `Refresh`. The roles are the union of:
    - the claim roles (a `trust_roles_claim` issuer),
    - the tenant's active bindings that match,
    - `operator` for `[auth] operator_subjects`.
  - An `email` or `domain` binding matches only a verified email. `VerifiedIdentity` and
    `AuthSession` gain `email_verified`. `mtls_san` bindings match nothing until S10.
  - If the binding store cannot be read, no token is minted: `Exchange` revokes the session it
    just opened.

  `AuthService` RPCs:
  - `ListBindings` / `GetBinding` (`read:binding`), `PutBinding` (`write:binding`) and
    `DeleteBinding` (`delete:binding`). The proto change is additive.
  - Rules, all refused with an opaque `PERMISSION_DENIED`: no grant beyond the caller's own
    permissions, a host-global role or another tenant only from a host-global caller, and no
    binding that names the caller (by sub, email or domain).
  - The last-admin guard is `FAILED_PRECONDITION`, and an unknown role is `INVALID_ARGUMENT`.
  - A delete, or a put that narrows a binding, revokes the old binding's live sessions
    (`revoke_reason = "binding"`) unless `keep_sessions` is set.

  Role cards: `RoleService.Put`/`Delete` are host-global (`agent_core::check_role_write`), because
  the catalog is shared by every tenant.

  Config: `[auth] operator_subjects` (`email:` / `sub:`, at most 64) is checked at load and needs
  `[auth.token]`.

  Deviations are listed under "As built (S8)" in [03](03-rbac.md#permission-to-manage-permissions-rules):
  - The binding RPCs are on `AuthService`, not `RoleService`.
  - Role-card writes are host-global.
  - `session.roles` now holds the login's claim roles.
  - `keep_sessions` replaces `revoke_active`, and only a narrowing put revokes.
  - The lockout guard is serialized per process.

  Tests:
  - `binding/tests.rs`: subject kinds, 27 validation cases (traversal, control characters, two `@`,
    wildcard domains, size caps), matching (unverified email, domain suffix, prefix and subdomain,
    tenant firewall, expiry boundary), `operator_subjects` parse and match, resolution,
    20 `check_binding_write` cases, the last-admin table, and a store round trip including a
    key-mismatch blob.
  - `agent_core::rbac`: `check_role_write` and `exceeding_permissions` tables.
  - `role.rs`: the card-write gate.
  - Config and `from_params` `operator_subjects` cases.
  - `tests/auth_token.rs` over a real listener:
    - bootstrap operator (verified and unverified email);
    - `Exchange` picks up a binding;
    - `positive_binding_change_revokes_sessions`;
    - `keep_sessions` followed by a refresh that drops the role;
    - refresh picks up a widening and is revoked by a narrowing;
    - 14 `PutBinding` refusal and grant cases;
    - `corner_last_binding_admin_not_deletable`;
    - binding reads, and `ListBindings` needing `read:binding`.
- **2026-09-27 — S9 (#514).** The caller's credentials now follow a request from one
  seam to the next.
  - `outbound()` sends `authorization: Bearer` with the caller's agent token (the
    `AGENT_BEARER` task-local S5 added). With no caller token it sends the process's
    service token from the new `BearerSource`, a once-per-process install; S10 and S12
    provide real sources. A caller's token is never replaced by the service's.
  - `x-agent-hops`: `AuthLayer` computes this server's hop as the inbound value + 1, with
    auth on or off. Over 4 is `FAILED_PRECONDITION`; a malformed or non-ASCII value is
    `INVALID_ARGUMENT`. `outbound()` stamps its own count, replacing any forged one.
  - `RequestScope` gains `hops`; `scope_request` installs it.
  - Six request spawn sites now run under `scope_request`. The session actor's `Run`
    message carries its submitter's scope. Deliberate exceptions carry
    `// unscoped-spawn: <reason>`.
  - Gate: `crates/agent-grpc/tests/no_unscoped_spawn.rs` scans the production source of
    `agent-grpc` server/client, `agent-runtime` and `agent-review-fleet`. It has 10 fixture
    cases testing the checker itself.
  - Tests:
    - `tests/auth_chain.rs`: two real servers, A forwarding to B. The user's token and
      principal reach B at hop 2; no token stops at A; a 6-case hop table at B; a forged
      `x-agent-hops: 0` still counts real hops; the loop ceiling; the service-token
      fallback with auth off at A, plus the refused second install.
    - Tables for `parse_hops`/`inject_hops`, `inbound_hops`, `select_bearer`,
      `scope_request` with hops, and `outbound()`'s bearer and hop headers.
  - Deferred: `peer_san` and service-token-needs-mTLS (S10), and attributing a queued
    `ReviewNow` to its requester (S10/S11).
- **2026-09-27 — S10 (#516).** Services prove who they are with their client certificate.
  - `AuthService.Exchange{use_client_cert}` (an additive proto field) maps the mTLS peer's URI SAN
    through `[[auth.mtls.bindings]]` to a service token:
    - `sub = svc:<service>`, `amr = ["mtls"]`;
    - a `cnf` `x5t#S256` thumbprint of the leaf certificate;
    - the binding's roles plus `mtls_san` role bindings, which now match.
    The service session lasts one token TTL, records `peer_san`, and refuses `Refresh`.
  - `AuthLayer` accepts a `cnf` token only from the certificate it names, or relayed by another
    bound service. A token without `cnf` needs no certificate. `peer_san` is a `grpc.server` span
    field.
  - Deviation: relaying by any bound service is allowed, because S9 forwards tokens unchanged
    across hops. The exact-certificate rule in the design would break every two-hop chain.
    Recorded in [04](04-service-integration.md).
  - `MtlsBearerSource` (installed by `[auth.mtls] token_endpoint`) is the first real S9
    `BearerSource`. It re-exchanges at 2/3 of the lifetime and backs off through `agent-retry`.
  - `oidc` on a non-loopback TCP listener without `[grpc.tls]` refuses to start
    (`allow_insecure_listen` overrides it with a warning).
  - Peer SANs come from a fail-closed DER walk (`auth/peer.rs`): minimal lengths only, a
    duplicate SAN extension refused, at most 64 entries, URIs printable and at most 2048 bytes.
  - `[auth.mtls]` is validated at load (the SAN, service, tenant and role rules, the 256-binding
    cap, an `https` endpoint with a client cert).
  - Tests:
    - `tests/mtls_identity.rs` (real mTLS listeners): a bound token issued; a
      connection table (own cert, relay, laptop cert, plaintext); `Exchange` refusals; a
      person's token over an unbound certificate; an `mtls_san` binding adding a role; the
      bearer source.
    - Unit tables: the DER walk (every truncation of a real leaf), bindings, `cnf`, token `cnf`
      claims (malformed = rejected), service sessions, the listen posture, and the config tables.
  - Deferred: a queued `ReviewNow` attributed to its requester (S11, with the audit rows);
    `step ca renew` and the `step-ca` daemon (S15).
- **2026-09-27 — S11a (#518).** S11 is split in two: the audit stream (here) and the `doctor`
  probes (S11b).
  - `agent_core::audit`: the `AuthEvent` type and a process-global sink.
    `record_auth_event` strips control characters and caps every text field at 256 bytes.
  - The serve path emits:
    - `AuthLayer` refusals;
    - gate decisions, and the handler checks in `authz::decide_on_span`;
    - permission-management refusals;
    - `Exchange` / `Refresh` (success and every refusal reason);
    - session revocation (`logout` for your own session, else `revoke` naming who did it);
    - role and binding writes.
  - `agent` forwards the events to `TelemetryHandle::record_auth_event`, which adds the
    trace id and writes to `agent.agent_auth_events`: `ORDER BY (user, ts, seq)`, kept
    400 days.
    - `tenant_iso_auth_events` (excluding `''`) applies to `agent_reader`;
      `operator_all_auth_events` to the writer, viewer and admin.
    - The RLS harness has four new rows (own tenant, viewer sees all, cross-tenant
      refused, unproven refusals hidden).
  - The gate's session-liveness check now runs before the permission, so an allow row
    always means the call went ahead.
  - Tests:
    - `server/audit.rs` tables: `rpc_label` against forged paths, decision filtering,
      target and peer SAN.
    - Revocation events, refresh labels and reason labels.
    - Layer rows through the served stack.
    - Rows and handle in `agent-telemetry`, including the trace id under a real
      OpenTelemetry layer.
    - `tests/auth_token.rs` over a real listener: the login → refresh → logout →
      `session_not_live` sequence; refused exchanges; binding put/delete; a gate denial.
    - Live (`nix run .#ch-integration`, podman on l2): the RLS rows, and a new ignored
      round-trip test. It writes as `agent_writer` through the real telemetry writer,
      then reads as `agent_reader`, which sees only its tenant's row. This exercise also
      showed that a row with the default 1970 `ts` is dropped by the TTL, so the seed
      stamps `now64(3)`.
  - Deferred: attributing a queued `ReviewNow` to its requester still waits for the
    fleet queue to carry the principal.
- **2026-09-27 — S11b (#521).** `agent doctor` gains four auth probes (`doctor/auth.rs`, behind
  `agent-runtime/auth`):
  - `auth.signer`: the signing key and `previous_key` load. The detail is the key id. A key
    file readable by group or other warns.
  - `auth.issuer.<name>`: one per login issuer. The key set is fetched through the verifier's
    own code (`probe_issuer_keys`), via discovery when there is no `jwks_url`.
    - `HttpJwks::get` and `DiscoveryJwks::discover` now return a reason. It is a short class
      (HTTP status, could not connect, timed out, not JSON, different issuer, no `jwks_uri`),
      never the URL.
    - The verifier still sees `()` and logs the reason.
  - `auth.sessions`: the session store lists tenants within the timeout. `memory` or unset
    warns.
  - `tls.certs`: the listener and client certificates are inside their window, and warn once
    less than a third of the lifetime is left.
    - `peer.rs` gains a DER `validity` reader: UTCTime / GeneralizedTime, `Z` only, and an
      inverted window is refused.
    - It also gains `pem_certificates` and `cert_file_validity`.
  - Signer expiry is not probed: `[auth.token]` holds a key, not a certificate.
  - `issuer_params` moved from `agent-cli` into `agent_runtime::auth_params`, so the serve path
    and the doctor build the same `IssuerParams`.
  - Tests:
    - Graders: the certificate window at the one-third boundary, expired, not yet valid.
    - Each probe against real files, a real session-store tier, testkit PKI leaves, and the
      loopback fake OIDC issuer. This covers discovery naming another issuer and URL
      credentials never echoed.
    - `der_time` / `validity` tables, including every truncation of a real leaf.
- **2026-09-27 — S12 (#524).** `agent login` / `logout` / `whoami`, and `[grpc.client] bearer`.
  - Device flow (RFC 8628) in `agent_grpc::client::login`:
    - Discovery must name the issuer, and every endpoint and shown URL passes `check_fetch_url`.
    - IdP answers are capped at 64 KiB. `interval` is clamped to 1–60 s (a `slow_down` adds 5 s)
      and the code window to 30 min.
    - The user code and URLs are refused if they carry control characters, so a hostile IdP
      cannot rewrite the terminal. Google's `verification_url` is accepted.
  - `AgentAuth` wraps `Exchange` / `Refresh` / `Logout` / `WhoAmI` at one endpoint.
    - A stored token that cannot be a header value is refused, not sent.
  - `TokenFile` keeps `<issuer>.json`:
    - The file is `0600` in a `0700` directory, written by an atomic rename.
    - Load refuses group/other-readable files, symlinks, non-files, files over 64 KiB and
      garbage.
  - `refresh_stored` holds `<issuer>.json.lock` across a refresh.
    - If another process already wrote a newer usable token, it is adopted without spending the
      handle.
    - `UNAUTHENTICATED` / `PERMISSION_DENIED` / `INVALID_ARGUMENT` mean the session ended; the
      rest is transient.
  - `LoginBearerSource` is the process `BearerSource`.
    - It refreshes at two thirds of each lifetime and backs off through `agent-retry`.
    - It drops the token once the session ends.
  - Config:
    - `[grpc.client] bearer`: `login` | `login:<issuer>` | `env:` | `file:`. A raw token is
      refused, as is `bearer` together with `[auth.mtls] token_endpoint`.
    - `[grpc.client] auth_endpoint` must be https, a loopback IP or `unix:`.
    - `[[auth.issuers]] client_secret` must be a reference, checked in every mode.
    - `AuthCfg::login_issuer` picks the named issuer or the only one.
  - `agent-cli`: bare `login` / `logout` / `whoami`, with `--issuer` and `--endpoint`.
    - They run before the egress proxy and telemetry.
    - `logout` deletes the local file even when the agent cannot be reached, and says the
      session stays live there until it expires.
  - Tests:
    - Tables for poll classification, pacing, device answers (escape sequences,
      `javascript:`, remote `http`, embedded credentials, oversize codes) and token-file
      refusals.
    - Device flow against the testkit `FakeIssuer`, which now scripts `/device` + `/token`:
      `slow_down`, `pending`, grant, deny, expire, and the client secret on every request.
    - `tests/cli_login.rs` over a real tonic `AuthService`: login → `WhoAmI`; refresh rotates
      and persists the handle; two sources sharing a file refresh together without revoking
      the session; logout ends the stored login; a forged handle ends it.
    - Config load tables; CLI parse table.
  - Deferred: the loopback-redirect code flow (see 01's as-built note), and a keyring backend
    (parity 50).
- **2026-09-27 — S13a (#528).** Browser sign-in, server side. S13 is split: the portal (S13b) needs
  RPCs that did not exist.
  - Design change: `Begin` / `Exchange{code}` are new `AuthService` RPCs and fields (additive,
    no `buf` baseline move). `06-portal-and-edge.md` assumed them.
  - `CodeFlow` in `server/auth/code_flow.rs`:
    - `Begin` checks the issuer, the exact redirect URI and the S256 challenge shape before
      touching the network, then discovers the issuer's endpoints (cached) and stores a random
      single-use `state` (10 min, ≤ 1024 in flight) with a `nonce`.
    - `Exchange{code, state, code_verifier}` spends the `state` first, checks the verifier
      against the challenge (so the IdP is never asked with a wrong one), and redeems the code
      with the redirect URI, the verifier and the `client_secret`. IdP answers are capped at
      64 KiB, with a 10 s timeout.
    - The ID token then goes through the normal login verifier. It must name the issuer `Begin`
      chose and carry the `nonce`.
    - One credential per `Exchange`: ID token, code or client certificate.
  - `Issuers` / `Begin` are public, alongside `Exchange` / `Jwks` / `Refresh`: `is_exempt`,
    `gate_of`, `test/mt-audit/authz.toml`.
  - Config:
    - `[auth] redirect_uris`: `https`, or `http` to a loopback IP; no fragment, no surrounding
      whitespace, ≤ 16; needs `[auth.token]`. Checked at load and again by the layer.
    - `[[auth.issuers]] client_secret` is resolved by the serve path only when browser sign-in
      is on (`serving_issuer_params`). A failed resolution refuses to start.
    - `IssuerParams.client_secret` is a `ClientSecret` whose `Debug` redacts the value.
  - Testkit `FakeIssuer::start_code`:
    - `/authorize` answers `302` to `redirect_uri?code&state`.
    - `/token` redeems each code once, checking the client, the secret, the redirect URI and
      PKCE S256.
    - A script can forge `nonce` / `iss` / `aud`.
  - Tests:
    - `code_flow` tables: challenge / verifier shapes (42/43/128/129), the S256 value
      cross-checked against Python hashlib, redirect-URI rules (`javascript:`, `data:`,
      userinfo, fragment, LAN `http`), parameter encoding, `state` expiry / single use / cap,
      nonce and issuer checks, and `Begin` / redeem refusals made before any network call.
    - `tests/browser_login.rs` over a real tonic server: `Issuers` → `Begin` → IdP redirect →
      `Exchange` → `WhoAmI`, and the secret and verifier reach the IdP. Refused: a spent
      `state`, a wrong verifier (the IdP gets no request), a code swapped between two sign-ins,
      a forged `nonce`, a wrong client secret, a redirect URI off the list, an unknown issuer,
      an ID token and a code together, and sign-in with no `redirect_uris`.
    - Found by the tests: `with_code_flow(None)` dropped the whole `AuthService`; fixed before
      commit.
- **2026-09-27 — S13b (#533).** Portal browser sign-in over S13a's RPCs.
  - `AuthState` (`ChangeNotifier`) runs the flow: `Issuers` → PKCE verifier + `Begin` →
    redirect → callback `?code&state` → `Exchange` → session in `sessionStorage` → refresh one
    minute before expiry → a refused refresh signs out with "Your session has ended".
  - `AuthInterceptor` adds the bearer to every unary and streaming call on every client.
    `AuthPlatform` hides the browser (`package:web`: location, history, sessionStorage); native
    and tests use `MemoryAuthPlatform`.
  - `AuthGate` shows `LoginPage` until signed in, then the shell inside a `CapabilityScope`,
    with an account strip on the navigation rail.
  - Design change: `PORTAL_AUTH` defaults to `auto` (sign in when the agent offers it), so
    deployments without `redirect_uris` keep working unchanged. `PORTAL_REDIRECT_URI` added.
  - Envoy: `authorization` added to CORS `allow_headers` (needed now; the rest is S14).
  - Only the `auth.*` Dart stubs were regenerated, to keep clear of the REST track's proto
    annotations.
  - Tests:
    - `login_spec` / `login_test` over the real gate and interceptor against a fake
      `AuthService`: round trip with the bearer on the shell's calls (none on sign-in RPCs),
      refresh swaps the bearer, sign-out revokes with the bearer, stored-session resume,
      `auto` / `off` run anonymously, refresh at `expires_at − 60` (and at once under a
      minute). Adversarial: a mismatched or planted `state` is never exchanged; markup in
      `?error=` is not echoed.
    - `FakeGateway` now records request metadata, so bearer assertions are on the wire.
    - Fleet: a `read:review`-only user sees no Approve, a read-only editor, no Review now and a
      disabled switch.
    - L0: the S256 vector matches the agent's; capability near-miss strings; the refresh delay;
      the `PORTAL_AUTH` parser.
  - Test de-flakes (the same hangs show on `main` under load): the fake `Subscribe` left an
    unlistened controller when cancelled mid-handler, and teardown awaited its `close()` for
    the full 10-minute timeout; the router `settle()` and graph `retry()` now wait for the
    reload's reply, not just the recorded call; `portal-widget` runs `flutter test
    --concurrency=4`.
  - Deferred: native desktop sign-in (read the CLI's stored login), a Roles / bindings page.
- **2026-09-27 — S14.** Envoy hardening. The grpc-web bridge config moved from a nix heredoc to a
  tested renderer ([`test/portal-envoy/portal_envoy.py`](../../../test/portal-envoy/portal_envoy.py))
  over a listener spec ([`nix/portal/envoy-spec.nix`](../../../nix/portal/envoy-spec.nix));
  `grpc-web-up` is its shim. Details: [06, "As built (S14)"](06-portal-and-edge.md#envoy-hardening).
  - Defaults: bind `127.0.0.1`, exact-origin CORS (`http://127.0.0.1:8092`,
    `http://localhost:8092`), `authorization` allowed.
  - `jwt_authn` against the agent's own JWKS (fetched with `AuthService.Jwks`, or a file / URL),
    bypassing AuthService, health and reflection; `PORTAL_AUTH` `auto` (default) / `on` / `off`.
    An unreachable agent is an error, never a silent "no edge check".
  - Optional listener TLS and upstream TLS / mTLS from files.
  - Image bumped to `envoyproxy/envoy:v1.39-latest`; `versions.envoy-bin` (same minor) runs
    `--mode validate` in the new `portal-envoy` check, which also runs check-the-checks.
  - Verified live on l2 with podman on side ports (CORS, missing / garbage / valid / wrong-`iss`
    tokens, bypass paths, TLS).
  - Moved to S15: `portal-e2e` under auth (needs S15's fake-issuer agent).
