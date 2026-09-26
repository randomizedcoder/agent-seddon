# 02 — The agent-issued token, sessions and audit

The review of this plan proposed: *a local certificate authority (smallstep, in nixpkgs) mints signed
JWTs that embed rich authorization data; every service validates the signature.* This doc evaluates
that proposal, adopts it in a split form, and answers the two follow-on questions: do we need a
session-tracking database, and an auth-event stream into ClickHouse.

## Evaluating the proposal

The proposal has two halves that need different tools:

| Half | Right tool | Why |
|---|---|---|
| **Certificates** — service identity, TLS, key lifecycle | smallstep `step-ca` (nixpkgs `step-ca` 0.30.2, `step-cli` 0.30.6) | X.509 + ACME, short-lived auto-renewed certs, SPIFFE-style SANs, offline `step certificate create` for the nix sandbox. Adopted in [07](07-transport-tls-and-pki.md). |
| **JWT minting** — the session token | the agent's own `AuthService` | A CA issues certificates, not user tokens. The token issuer must know sessions, roles and bindings, which only the agent does. Its **signing key is a certificate-backed key issued by step-ca**, so rotation, expiry and provenance ride the same PKI, and the agent publishes the JWKS. |

The result is exactly the flexible system the proposal describes — one signature check at every
service, authorization embedded in the token — without turning the CA into a token service.

## Self-contained vs reference tokens

| | Self-contained (claims carry everything) | Reference (opaque id, look up per call) |
|---|---|---|
| Per-call cost | signature check only | a store round-trip per seam per call |
| Cross-process | works with no shared store at the seam | every seam needs the session store |
| Revocation | stale until `exp` | immediate |
| Size | grows with permissions | constant |

**Decision (D10): self-contained with a short TTL, plus a live session check for sensitive actions.**
The token carries the permissions snapshot; `approve`, `exec`, and `role` / `binding` / `config`
writes additionally verify that `sid` is live (a per-seam cache of ≤ 5 s over the session store).
Reads and ordinary writes trust the token until it expires (15 minutes). Size is bounded: permissions
are compact `action:resource` strings (≤ 40 pairs is under 2 KB; the h2 header-list limit is 16 KB
and the token stays under 4 KB); a role set that would exceed the cap is emitted as `roles` only with
`perms_ref = true`, and seams resolve it by reference.

## Claims

```json
{
  "iss": "https://agent.example",         "aud": "agent-seddon",
  "sub": "user:google/1049…",             "tenant": "example.com",
  "email": "alice@example.com",           "amr": ["oidc:google"],
  "roles": ["reviewer"],
  "perms": ["use:agent","read:prompt","read:review","write:review","approve:review","trigger:fleet","read:fleet","read:graph"],
  "sid": "s_8f3…",  "jti": "t_2a1…",
  "iat": 1790000000, "nbf": 1790000000, "exp": 1790000900,
  "act": null,  "cnf": null
}
```

| Claim | Meaning |
|---|---|
| `sub` | `user:<issuer>/<sub>` for humans, `svc:<name>` for services |
| `tenant` | the verified organization; becomes `SessionKey.user` |
| `roles`, `perms` | resolved at exchange / refresh time ([03](03-rbac.md)) |
| `sid` | the `auth_sessions` row; the reference check key |
| `amr` | how the principal authenticated: `oidc:<issuer>`, `device:<issuer>`, `mtls` |
| `act` | reserved for delegation ([04](04-service-integration.md)); not rewritten per hop in this track |
| `cnf` | certificate thumbprint for service tokens (bound to the mTLS peer) |

Algorithm: **ES256** (P-256, cheap to verify, small); RS256 accepted for compatibility. The allowed
set stays pinned server-side exactly as today ([`auth.rs`](../../../crates/agent-grpc/src/server/auth.rs):286-288);
seams accept the agent's issuer **only** — an IdP token on any RPC other than `Exchange` is
`UNAUTHENTICATED`.

## Keys, JWKS and rotation

```toml
[auth.token]
issuer = "https://agent.example"
audience = "agent-seddon"
ttl_secs = 900
signing_key_ref = "file:/run/agent-seddon/pki/token-signer.key"   # P-256, cert from step-ca
previous_key_ref = ""                                              # kept in the JWKS for one TTL
session_key_ref = "file:/run/agent-seddon/pki/session.key"         # encrypts refresh tokens at rest
```

- `kid` = the certificate thumbprint. The JWKS is served by `AuthService.Jwks` and, once REST lands,
  at `/.well-known/jwks.json` through the transcoder. Envoy `jwt_authn` and every seam read it.
- Rotation: drop the new key file, move the old one to `previous_key_ref`, reload; tokens signed by
  the previous `kid` verify for one TTL, then the old key is removed. The certificate lifecycle
  (issuance, renewal, `not_after`) is step-ca's ([07](07-transport-tls-and-pki.md)); the agent refuses
  to start with an expired signer certificate and `agent doctor` warns two days before expiry.
- One signing key per deployment. Every `--serve-*` process that hosts `AuthService` must share it
  (a file on a shared secret mount); processes that only *verify* need the JWKS URL.

## Session store (D11)

**Yes, a session store is needed.** Without it there is no logout that means anything, no
revocation when a binding changes or an IdP disables a user, no refresh without re-login every
15 minutes, and no answer to "who is signed in to this tenant".

It lives in the **config store** (`agent-config-store`, migration `0003_auth_sessions.sql` via the
PG-01 runner — [`migrations/`](../../../crates/agent-config-store/migrations/)), so production uses
Postgres and dev uses the `file` / `sqlite` tiers with no extra service, the same way every card store
already does.

| Column | Purpose |
|---|---|
| `sid` (pk), `tenant`, `subject`, `issuer`, `email`, `amr`, `client_kind` (`portal` \| `cli` \| `service`), `client_meta` (user agent or peer SAN) | identity of the session |
| `roles_snapshot` | what was embedded at the last mint (audit) |
| `created_at`, `expires_at` (absolute, default 12 h), `last_seen_at` | lifetime |
| `refresh_token_enc` | the IdP refresh token, encrypted with `session_key_ref`; used only when the IdP session must be revalidated (`revalidate_with_idp_every`, default 8 h) |
| `refresh_handle_hash` | the opaque handle the client holds, hashed; rotated on every `Refresh` |
| `revoked_at`, `revoked_by`, `revoke_reason` | `logout` \| `operator` \| `binding_change` \| `idp` |

RPCs (all on `AuthService`): `Exchange` opens; `Refresh` touches, re-resolves roles, rotates the
handle and mints a new token; `Logout` revokes the caller's session; `ListMySessions` /
`RevokeMySession` for the signed-in user; `ListSessions` / `RevokeSession` for `(read | write,
binding)` holders within their tenant. Expired rows are garbage-collected by the scheduler seam
(`auth_sessions_gc`, daily). Reuse of a rotated refresh handle revokes the whole session (token
theft signal).

## Audit stream (D11)

**Yes, an audit stream is needed**, and ClickHouse with the existing tenant pattern is the right home:
every telemetry table already uses a `user`-leading `ORDER BY` with a `tenant_iso_*` ROW POLICY
([`schema.sql`](../../../nix/clickhouse/schema.sql):42-63, 347-357).

```sql
CREATE TABLE IF NOT EXISTS agent.agent_auth_events
(
    ts              DateTime64(3),
    user            LowCardinality(String),   -- tenant (leading key, RLS predicate)
    session_id      String,                   -- the auth `sid`
    subject         String,
    event           LowCardinality(String),   -- login | exchange | refresh | logout | revoke
                                              -- | verify_ok | verify_fail | authz_allow | authz_deny
                                              -- | binding_put | binding_delete | role_put | role_delete | mtls_peer
    issuer          LowCardinality(String),
    amr             LowCardinality(String),
    action          LowCardinality(String),   -- authz_* rows
    resource_type   LowCardinality(String),
    rpc             LowCardinality(String),
    reason          LowCardinality(String),   -- bounded enum, never token material
    client_kind     LowCardinality(String),
    peer_san        String,
    trace_id        String,
    seq             UInt32
)
ENGINE = MergeTree
ORDER BY (user, ts, seq)
TTL toDateTime(ts) + INTERVAL 400 DAY;

CREATE ROW POLICY IF NOT EXISTS tenant_iso_auth_events ON agent.agent_auth_events
    AS PERMISSIVE FOR SELECT USING user = getSetting('SQL_tenant_id') TO agent_reader;
```

- Written through the existing async-insert telemetry writer
  ([`writer.rs`](../../../crates/agent-telemetry/src/writer.rs):204), never sampled.
- `verify_fail` rows carry a bounded `reason` (`expired`, `bad_signature`, `unknown_kid`, `wrong_aud`,
  …) and no token bytes; `authz_deny` rows carry action and resource, not the request body.
- Cross-tenant queries (an operator's incident review) use the writer / admin credential, which the
  policy does not filter ([08](08-data-plane-and-secrets.md)); tenants see only their own rows.
- The existing counters `agent_auth_verify_total` and `agent_authz_decisions_total` remain the cheap
  health signal; the table is the forensic trail. A portal "Audit" page over it is P1.

## Test matrix

| Class | Case | Expect |
|---|---|---|
| positive | `positive_exchange_mints_token_with_perms` | claims carry tenant, roles, perms, sid |
| positive | `positive_rotation_grace_accepts_previous_kid` | old `kid` verifies within one TTL after rotation |
| positive | `positive_refresh_rotates_handle_and_reresolves_roles` | new handle; a binding added since login appears |
| positive | `positive_audit_row_per_event` | every event kind produces one row with the tenant as `user` |
| negative | `negative_expired_agent_token_rejected` | `exp` + leeway passed → `UNAUTHENTICATED` |
| negative | `negative_revoked_sid_denied_on_sensitive_action` | `Approve` with a revoked `sid` → `PERMISSION_DENIED` |
| negative | `negative_signer_cert_expired_refuses_start` | startup error, never an unsigned token |
| boundary | `boundary_token_size_under_cap` | 40 permissions → under 4 KB; 41 → `perms_ref` form |
| corner | `corner_revoked_sid_still_reads_until_exp` | documented: a revoked session's token reads until `exp` |
| corner | `corner_service_token_carries_cnf` | `amr = mtls`, `cnf` = peer thumbprint |
| adversarial | `adversarial_token_signed_by_old_key_after_grace_rejected` | `kid` dropped from JWKS → rejected |
| adversarial | `adversarial_reused_refresh_handle_revokes_session` | second use of a rotated handle → session revoked, both callers logged out |
| adversarial | `adversarial_audit_rows_isolated_by_tenant` | reader for tenant A sees none of B's rows (RLS harness) |
