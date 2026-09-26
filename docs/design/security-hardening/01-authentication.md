# 01 — Authentication (login)

How a human or a machine proves who they are. What they may then do is
[`03-rbac.md`](03-rbac.md); the token every call carries afterwards is
[`02-token-service.md`](02-token-service.md).

## Where we are today

- `AuthLayer` ([`auth.rs`](../../../crates/agent-grpc/src/server/auth.rs):211-265) verifies one
  configured issuer's RS256 / ES256 JWT against a cached JWKS: `iss`, `aud`, `exp` / `nbf` with
  leeway, `sub`, a tenant claim (default `org`) and a roles claim (default `roles`); it rewrites
  `x-agent-user-id` to the verified tenant and installs `VerifiedPrincipal{tenant, subject, roles}`
  for the RBAC gate. Health and reflection are exempt. Twenty-two hermetic tests
  ([`auth/tests.rs`](../../../crates/agent-grpc/src/server/auth/tests.rs)) use an embedded RSA key, a
  switchable JWKS source and an injectable clock.
- Config is a single `[auth]` block with one issuer
  ([`config.rs`](../../../crates/agent-runtime/src/config.rs):2572-2604); nothing is validated at
  load time, only when a server starts ([`auth.rs`](../../../crates/agent-grpc/src/server/auth.rs):132-155).
- There is no login flow anywhere: the portal hardcodes `x-agent-user-id: 'portal'`
  ([`agent_view_page.dart`](../../../portal/lib/src/pages/agent_view_page.dart):106-109), the CLI has
  no token flag or file, and no client attaches a bearer.

## Requirements (what "enterprise-level" means here)

| Requirement | Met by |
|---|---|
| SSO against the customer's IdP (Google Workspace, Okta, Entra ID, Keycloak, …) | OIDC profiles (below) |
| MFA, password policy, device posture | delegated to the IdP; never re-implemented |
| Short-lived credentials with refresh; logout and revocation that take effect | the agent token + session store ([02](02-token-service.md)) |
| Tenant = organization, derived from a verified claim, never from a client header | profile `tenant_claim` → `safe_segment` → `VerifiedPrincipal.tenant` |
| Group / role mapping when the IdP has none (Google) | role bindings ([03](03-rbac.md)) |
| Machine identities without long-lived shared secrets | mTLS certificates ([07](07-transport-tls-and-pki.md)) |
| Key rotation | JWKS refetch on `kid` miss (already, rate-limited) for IdPs; `kid` grace for the agent key |
| No secrets in the browser | code exchange in the agent; PKCE in the browser |
| Works behind Envoy (grpc-web), REST and native gRPC | one bearer header, one `AuthLayer` ([04](04-service-integration.md)) |
| Auditable | `agent_auth_events` ([02](02-token-service.md)) |

## Options evaluated

| | Mechanism | Enterprise fit | Tenant / roles source | Refresh / revocation | Browser / CLI | Ops cost | Verdict |
|---|---|---|---|---|---|---|---|
| **A** | Google only, via Google Identity Services (ID token minted in the browser) | Google shops only; no Okta / Entra / SAML | `hd` → tenant; **no roles in the token** | 1 h ID token, no refresh in a SPA (re-prompt); revocation = none | GIS script in the portal; no CLI path | lowest | one *profile* of B, not the architecture |
| **B** | Generic OIDC verifier with per-issuer **profiles** | any OIDC IdP; brokers add SAML / LDAP with no agent change | profile claim map + bindings | via the agent token (D) | code + PKCE (portal), device code (CLI) | low | **adopted for login** |
| **C** | Mandatory broker in front (Keycloak / Dex / Zitadel) | strongest federation, SCIM, custom claims | broker claims | broker sessions | broker UI | one more service to run (heavy for the single-host l2 deploy) | recommended deployment for SAML / large orgs; it *is* B with one profile, so not required |
| **D** | Agent-issued JWT after IdP login (token exchange, RFC 8693) | uniform across IdPs; one verification path for gRPC / REST / seam-to-seam | embedded at exchange time | agent-controlled TTL; sessions table | any client | signing key, JWKS endpoint, session store | **adopted for the session / inter-service token** — see [02](02-token-service.md); the cost is what the smallstep PKI and the session store pay for |
| **E** | Edge identity-aware proxy (oauth2-proxy, Pomerium, Cloudflare Access) | fine for browsers | forwarded headers (rejected) or a proxy-signed JWT (= B with the proxy as issuer) | proxy sessions | grpc-web and streaming quirks; no CLI story | extra infra | compatible with B; not built |

The earlier draft of this plan deferred D. The service-integration questions in
[04](04-service-integration.md) — how credentials chain between seams, how REST plugs in, how to test
propagation — are all answered more simply by one agent-issued token than by forwarding IdP tokens,
so D is adopted alongside B.

## Login token model

The IdP **ID token** (a JWT whose `aud` is our client id) is presented exactly once, to
`AuthService.Exchange`. Access tokens are opaque at Google and are never used. `Exchange` verifies it
with the issuer's profile, resolves the principal, opens a session and returns the agent token.

### Issuer profiles

```toml
[auth]
mode = "oidc"                                   # none | oidc
require_identity = true                         # see 05-identity-and-tenancy.md
operator_subjects = ["email:dave@example.com"]  # bootstrap operators (no binding needed)

[[auth.issuers]]
name = "google"
profile = "google"                              # google | entra | generic
client_id = "….apps.googleusercontent.com"
client_secret_ref = "file:/run/secrets/google-oauth"   # confidential exchange (Google requires it)
allowed_domains = ["example.com"]               # `hd` must be one of these

[[auth.issuers]]
name = "keycloak"
profile = "generic"
issuer = "https://idp.example/realms/agents"    # discovery at /.well-known/openid-configuration
audience = "agent-seddon"
tenant_claim = "org"
roles_claim = "roles"
trust_roles_claim = true                        # union with bindings (default false)
```

| Profile | `iss` | JWKS | tenant ← | subject ← | extra rules |
|---|---|---|---|---|---|
| `google` | `https://accounts.google.com` | `https://www.googleapis.com/oauth2/v3/certs` | `hd` (Workspace domain) | `sub` (+ `email`) | `email_verified == true`; `hd ∈ allowed_domains`; consumer accounts (no `hd`) rejected unless `default_tenant` is set |
| `entra` | `https://login.microsoftonline.com/{tid}/v2.0` | discovery | `tid` | `oid` | `allowed_tenants` |
| `generic` | configured | configured or discovery | `tenant_claim` | `subject_claim` (default `sub`) | `require_email_verified`, `allowed_domains` optional |

The single-issuer form (`issuer` / `audience` / `jwks_url` at the top level) stays accepted as one
`generic` issuer named `default`, so today's configs keep working. The tenant still passes
`safe_segment` ([`identity.rs`](../../../crates/agent-core/src/identity.rs):26-34 allows `.`, so a
Workspace domain such as `example.com` is a valid tenant segment and a valid path segment).

### Bootstrap

A fresh install has no bindings. `[auth] operator_subjects` (operator-global TOML, so only the host
operator can edit it — C29) lists `email:` or `sub:` subjects that receive the `operator` role at
exchange time. Everything else is granted through bindings ([03](03-rbac.md)).

## Human flows

### Portal (browser)

Authorization Code + PKCE, with the exchange in the agent (D4):

1. Portal calls `AuthService.Begin{issuer, code_challenge}` (the PKCE verifier is generated in the
   portal and never leaves it). The agent returns `{authorize_url, state}`; `state` is a server nonce,
   single-use, 10-minute TTL.
2. The browser is redirected to the IdP and back to `PORTAL_WEB_ORIGIN/?code=…&state=…` (the
   portal's own origin — no new route, so `static-web-server` needs no SPA fallback).
3. Portal calls `AuthService.Exchange{code, state, code_verifier}`. The agent exchanges the code
   (with `client_secret` when configured, PKCE-only otherwise), verifies the returned ID token against
   the issuer profile, resolves roles and permissions, opens an `auth_session`, mints the agent token
   and returns `{access_token, expires_at, refresh_handle, principal}`.
4. Portal keeps the access token in memory (plus `sessionStorage` to survive a reload), attaches
   `authorization: Bearer` on every call, calls `AuthService.Refresh{refresh_handle}` a minute before
   expiry, and `AuthService.Logout{refresh_handle}` on sign-out.

`AuthService` is exempt from the bearer requirement (like health and reflection) and sits behind the
existing admission layer, so it is rate-limited. Portal details: [`06-portal-and-edge.md`](06-portal-and-edge.md).

### CLI (`agent login`)

- `agent login [--issuer name]` uses the **Device Authorization Grant** (RFC 8628) when the issuer's
  discovery document advertises `device_authorization_endpoint` (Google, Keycloak, Okta and Entra
  do); otherwise a loopback-redirect code flow. The CLI then calls `Exchange{id_token}` and stores the
  **agent** token and refresh handle in `$XDG_CONFIG_HOME/agent-seddon/tokens/<issuer>.json` with
  mode `0600` (the keyring backend of [parity 50](../../parity/50-secret-store.md) later).
- `agent logout` revokes the session; `agent whoami` calls `AuthService.WhoAmI` and prints tenant,
  subject, roles, permissions and session id.
- The stored token feeds the process `BearerSource` ([04](04-service-integration.md)), which
  refreshes in the background.

The CLI is hand-parsed with a bare-word subcommand pattern
([`main.rs`](../../../crates/agent-cli/src/main.rs):766-768 for `doctor`); `login` / `logout` /
`whoami` follow it.

## Machine identities

- Services (the fleet, `= "grpc"` seam clients, Envoy, the portal-e2e driver) hold **mTLS
  certificates** issued by the local CA ([07](07-transport-tls-and-pki.md)). A service obtains an
  agent token with `AuthService.Exchange{kind = client_cert}` over its mTLS connection: the peer
  certificate's SAN is looked up in `[auth.mtls] bindings` → tenant + roles → a service principal
  (`sub = "svc:fleet"`). Its downstream calls then forward that token like any other caller's.
- A service token is bound to its certificate (`cnf` claim = certificate thumbprint) and is rejected
  over a connection that does not present that certificate.
- An IdP service account (Google: ID token with `target_audience`; Keycloak: `client_credentials`)
  is an alternative `BearerSource` kind for deployments that already manage service accounts in the
  IdP; it is documented, not built in this track.
- Unix sockets keep file-permission trust. `[auth] uds_trusted = true` keeps header identity on UDS
  listeners explicitly; otherwise `mode` applies to every listener.

```toml
[auth.mtls]
bindings = [
  { san = "spiffe://agent.example/svc/fleet", tenant = "example.com", roles = ["svc_fleet"] },
  { san = "dns:envoy.internal",              tenant = "local",       roles = ["svc_edge"] },
]
```

## Test matrix

Four classes plus `adversarial_`, each row with `desc` and `expect`
([`08-testing-and-integration.md`](../config/08-testing-and-integration.md) shape).

| Class | Case | Expect |
|---|---|---|
| positive | `positive_google_profile_maps_hd_to_tenant` | `hd = example.com` → tenant `example.com`, subject `sub` |
| positive | `positive_entra_profile_maps_tid` | `tid` → tenant |
| positive | `positive_single_issuer_config_still_accepted` | legacy `[auth] issuer/audience/jwks_url` = one `generic` issuer |
| positive | `positive_operator_subject_bootstraps_operator_role` | listed email → `operator` without any binding |
| negative | `negative_email_unverified_rejected` | `email_verified = false` → `UNAUTHENTICATED` |
| negative | `negative_consumer_account_without_hd_rejected` | no `hd`, no `default_tenant` → rejected |
| negative | `negative_domain_not_allowed_rejected` | `hd ∉ allowed_domains` → rejected |
| boundary | `boundary_state_expires_at_ten_minutes` | `Exchange` at 10 min + 1 s → rejected |
| corner | `corner_default_tenant_for_single_org_deploy` | no `hd` + `default_tenant` → that tenant |
| corner | `corner_device_flow_slow_down_honoured` | `slow_down` → interval +5 s |
| adversarial | `adversarial_cross_issuer_kid_confusion_rejected` | a token signed by issuer A's key claiming issuer B → rejected |
| adversarial | `adversarial_state_replay_rejected` | second `Exchange` with the same `state` → rejected |
| adversarial | `adversarial_pkce_verifier_mismatch_rejected` | wrong `code_verifier` → rejected, session not opened |
| adversarial | `adversarial_idp_token_presented_to_a_seam_rejected` | an IdP ID token as bearer on `PromptService.List` → `UNAUTHENTICATED` |

**Fake OIDC issuer.** `agent-testkit` gains an in-process issuer (`tiny_http`, already a workspace
dependency) serving discovery, JWKS, the token endpoint (code and device grants) and the device
endpoint, reusing the RSA fixture from `auth/tests.rs`. It is what lets `nix flake check` run a real
`agent --serve-all` under `mode = "oidc"` ([04 §testing](04-service-integration.md#testing-auth-and-credential-passing)).
