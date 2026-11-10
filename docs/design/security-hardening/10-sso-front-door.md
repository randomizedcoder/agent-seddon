# 10 — One sign-in for the portal and the observability tools

> **Status:** design, 2026-11-08. Increments S25–S31 in [`STATUS.md`](STATUS.md). Nothing here is
> built. Claims about today carry a `path:line` against `main` `2ce08c89`.

## Why

A NixOS deployment of the agent stack (`nixosModules.agent-stack`) has no sign-in anywhere, and the
observability tools each have their own:

| Surface | Port | Today |
|---|---|---|
| Portal (static) | 8092 | open; `static-web-server` ([`agent-stack.nix`](../../../nix/nixos/agent-stack.nix):706-718) |
| grpc-web bridge | 8090 / 8091 / 8093 | open: Envoy is rendered with `PORTAL_AUTH = "off"`, no `jwt_authn` ([`nix/portal/default.nix`](../../../nix/portal/default.nix):880-905). Reaching :8090 is code execution as the agent user ([`agent-stack.nix`](../../../nix/nixos/agent-stack.nix):50-53) |
| HyperDX UI | 8080 | its own login by default; open when a host sets `hyperdx.auth = false` (#704, as l2 does); firewall-opened |
| Grafana | 3000 | its own `admin` login; the module sets no password and no auth mode ([`agent-stack.nix`](../../../nix/nixos/agent-stack.nix):721-770), so the password is whatever Grafana's database holds |
| Prometheus | 9090 | open; not managed by the module (the host's own service) |

The agent already has everything a sign-in needs except a login provider and a browser cookie:
OIDC issuers, `AuthService.Begin` / `Exchange` with code + PKCE, agent-minted ES256 tokens,
sessions with refresh and revocation, RBAC and role bindings (S3–S13, S22). The portal sends the
agent token as an `authorization` header from `sessionStorage`
([`auth_interceptor.dart`](../../../portal/lib/src/auth/auth_interceptor.dart):37), which a browser
never attaches to a page load, an image or a new tab. So the tools cannot sit behind it as they are.

## Goal

One sign-in, at the agent's front door, covers the portal, Grafana, HyperDX and Prometheus. Each
tool listens on loopback only and is reached through the front door. The agent decides who gets in
and what they may see, from the same roles and bindings as every RPC.

Out of scope: the k8s edge (K6, [`k8s/09-edge-and-observability.md`](../k8s/09-edge-and-observability.md))
reuses the same pieces later; this design is the NixOS module.

## The shape

```
 browser on l ──https──► Envoy front door  :8443  (TLS)
                          │ 1. ext_authz → agent AuthGate.Check(cookie, path)
                          │      ├─ no/invalid session ⇒ 302 /auth/login?rd=<path>
                          │      ├─ lacks permission   ⇒ 403
                          │      └─ ok ⇒ headers to add upstream (below)
                          │ 2. route
                          ├── /auth/*        → agent AuthGate HTTP (login, callback, logout)  [no check]
                          ├── /dex/*         → Dex :5556                                       [no check]
                          ├── /              → portal static files (loopback)
                          ├── /agent.v1.*    → grpc-web → gateway / sessions / fleet  (+ Bearer)
                          ├── /grafana/      → Grafana 127.0.0.1:3000   (auth.proxy user + role)
                          └── /prometheus/   → Prometheus 127.0.0.1:9090
                         Envoy front door  :8444  (TLS, same check)  → HyperDX UI 127.0.0.1:8080
                         (its own port, so HyperDX's UI is served from `/` as its image expects)

 Dex ──GitHub OAuth──► github.com      Dex is an OIDC issuer the agent already understands
```

## Decisions

- **F1 — Login provider: Dex with the GitHub connector** (chosen 2026-11-08). Dex is packaged in
  NixOS (`services.dex`), turns a GitHub OAuth login into an OIDC ID token, and the agent takes it
  as one more `[[auth.issuers]]` entry with the `generic` profile. Nothing new in the agent for the
  provider itself. Dex serves under the front door at `/dex` so its issuer URL is the same origin.
  Dex mounts every route under its issuer's path (its own example config uses
  `issuer: http://127.0.0.1:5556/dex`), so the front door forwards `/dex/*` unchanged. The GitHub
  OAuth App's client secret and Dex's client secret for the agent are files (`LoadCredential`),
  never in the store. The connector can also be limited to GitHub organizations (`orgs`), which
  cuts who can even open a session before F2 turns them away.
- **F2 — Deny by default.** Any GitHub user can finish a Dex login, so the gate admits only a
  principal with a permission for the route: the operator (`[auth] operator_subjects`, a verified
  email or `sub:<issuer>/<sub>`) and anyone given a binding in the portal's Access page (S22). An
  authenticated principal with no binding gets 403 everywhere, including the portal. Unverified
  emails already match no binding; the Dex issuer entry also sets `require_email_verified = true`
  so a login with one stops at the agent. The admission check runs at `/auth/callback`, before a
  session is opened: a principal with no binding and no `operator_subjects` match gets 403 and no
  session row, so free GitHub accounts cannot fill the tenant's session cap
  (`DEFAULT_MAX_SESSIONS_PER_TENANT`). Nothing is lost: a binding names an email or `sub`, so the
  operator can grant access before the user ever signs in.
- **F3 — The agent owns the browser session (backend-for-frontend).** A new `AuthGate` in the
  agent runs the code flow itself: `/auth/login` calls the existing `Begin` logic and redirects to
  Dex; `/auth/callback` runs the existing code `Exchange`, opens the usual auth session and sets
  one cookie. The cookie holds an opaque random handle (256 bits) mapped to the auth session's
  `sid`, never a JWT, so a revoked session (`Logout`, `RevokeSession`) stops at the next request.
  The login is bound to the browser that started it. `/auth/login` sets a second cookie,
  `__Host-seddon-login` (same attributes, expiring with the `state`), holding a random nonce whose
  hash is stored with the `state`. `/auth/callback` redeems the code only when that cookie is
  present and matches the `state`'s nonce, and clears it either way. Without this, a cross-site
  page could navigate a victim to `/auth/callback` with a code and `state` the attacker got from
  their own login, signing the victim in as the attacker (login CSRF). One login runs per
  browser at a time: a second tab's `/auth/login` replaces the cookie, so the first tab's
  callback is refused and that tab starts again.
  The table stores only the handle's hash, as refresh handles already do (`session.rs`), so a
  copy of the session store yields no live browser session. Each entry belongs to its auth
  session: it counts against the same per-tenant cap (`DEFAULT_MAX_SESSIONS_PER_TENANT`) and goes
  when that session ends, is revoked or is garbage-collected at expiry.
  Cookie: `__Host-seddon`, `HttpOnly`, `Secure`, `SameSite=Lax`, `Path=/`, no `Domain`. Because
  cookies are not port-scoped, the same cookie covers :8443 and :8444.
- **F4 — Envoy checks every request with `ext_authz`**, the HTTP flavour (`http_service`): Envoy
  sends the request's headers to `AuthGate`'s check route on loopback and gets 200 (plus headers
  to add), 302 or 403 back. It needs no protos, and the gate's routes are plain HTTP anyway.
  Envoy passes a denied answer's status and headers (`Location` included) to the browser; S28
  confirms the 302 in a test before anything rests on it, with a 401 and a login page as the
  fallback. `Check` maps the path to a required permission, resolves the caller, and answers with
  headers for the upstream. The caller is the cookie session or, with no cookie, an agent token
  the client sent itself (`authorization: Bearer`, checked against the agent's own signing keys,
  agent tokens only). That second form is how the CLI (`agent login`, S12) and the native portal
  (S23 / S24) get through the front door, unchanged.
  - grpc-web routes: `authorization: Bearer <agent token>`, always: the cookie session's token
    (minted and cached per session until a minute before expiry), or the client's own token,
    verified and echoed back. The existing `AuthLayer` verifies it as today, so every RPC keeps its
    RBAC. The portal keeps no token in JavaScript in this mode.
  - `/grafana/`: `X-Seddon-User` (the principal) and `X-Seddon-Role` (`Admin` for the operator,
    `Editor` with a write permission on observability, else `Viewer`).
  - Prometheus and HyperDX: nothing extra; they are behind the check and on loopback.

  On an RPC (a grpc-web route) the denied answer is a 401 with no `Location`, since a `fetch`
  that followed a 302 would get the login page's HTML instead of a gRPC status. The portal turns
  that 401 into a navigation to `/auth/login?rd=<the page's path>`; `rd` is always a page, never
  an RPC path.

  What reaches each upstream is fixed per route, so a client cannot forge an identity and
  nothing a route needs is lost. Route-level `request_headers_to_remove` takes exact header
  names (no prefixes) and runs in the router, after `ext_authz`, so it would also drop a header
  `Check` adds. A header `Check` supplies on a route is therefore *replaced*, not removed: its
  200 always carries it, listed in `allowed_upstream_headers`, which overwrites the client's
  value. The identity headers are a closed set, and every route says what happens to each:

  | Header | grpc-web | `/` (portal) | `/auth/*` | `/dex/*` | `/grafana/` | `/prometheus/` | HyperDX (:8444) |
  |---|---|---|---|---|---|---|---|
  | `authorization` | replaced | removed | removed | removed | removed | removed | removed |
  | `cookie` | removed | removed | kept | kept | removed | removed | removed |
  | `x-seddon-user` | removed | removed | removed | removed | replaced | removed | removed |
  | `x-seddon-role` | removed | removed | removed | removed | replaced | removed | removed |
  | `x-agent-user-id` | removed | removed | removed | removed | removed | removed | removed |
  | `x-agent-session-id` | kept | removed | removed | removed | removed | removed | removed |

  - `authorization` on grpc-web is the agent token `Check` chose (above): whatever reaches the
    agent is that.
  - `cookie` is kept only where it is needed: `/auth/*`, since logout needs it to find the
    session to end, and `/dex/*`, which is not behind the check and is the identity provider
    itself. The agent and the tools never see the session handle.
  - `x-agent-session-id` is kept on grpc-web: the portal names its session with it, and the RPC's
    own authorization decides access (S7).
  - `x-agent-user-id` is removed everywhere, not only where the agent would overwrite it, so a
    forged one reaches no tool either.

  The table is one Nix value: it renders every route's `request_headers_to_remove` and the
  per-route `replaced` set `Check` answers with. A header added to the gate is a new row, so no
  route can miss it. S28's `portal_envoy` rows assert each route's lists equal the table, and a
  `Check` row asserts it never answers with a header outside its route's `replaced` set.

  `Check` failing or timing out (1 s) fails closed (`failure_mode_allow: false`).
- **F5 — Permissions.** New pairs in the RBAC catalogue, named like the rest (`read:` / `write:`):
  `read:observability` (Grafana Viewer, Prometheus, HyperDX) and `write:observability` (Grafana
  Editor). They sit beside `read:telemetry`, which stays MetricsProxyService's: that one reads the
  agent's own metrics through an RPC, these open the tools' web UIs. The portal itself needs only
  what its RPCs already require; the gate lets a principal reach `/` with any permission at all,
  so an operator with no binding row (`operator_subjects`) gets in, and an unbound user does not.
  `observability` is operator-global (`ResourceType::is_operator_global`), unlike `telemetry`:
  the tools show every tenant's data, so only a host-global granter may bind it, and a tenant
  admin's attempt is refused (`GrantRefusal::HostGlobal`). The pairs land with the gate (S27), so
  its path map is complete from the start. The map is default-deny: a path it does not list gets
  403, so the portal's `/`, its assets and `favicon.ico` are entries of their own.
- **F6 — Grafana uses `auth.proxy`, and HyperDX has no login of its own.** With the front door
  on, the gate is HyperDX's sign-in: the module requires `hyperdx.auth = false` (an assertion),
  so nobody signs in twice. That also makes HyperDX's collector accept OTLP with no ingestion
  key; its ports bind every address under `--network=host`, so it is S30's firewall that keeps
  keyless ingestion local (see F9 and the open item on HyperDX's listeners).
  Grafana: `header_name = X-Seddon-User`, `headers = Role:X-Seddon-Role`,
  `auto_sign_up = true`, `whitelist = 127.0.0.1`, `enable_login_token = false`, login form and
  basic auth off, `root_url = https://<host>:8443/grafana/`, `serve_from_sub_path = true`. The
  `admin` account stays for break-glass; its password comes from a file (F8).
- **F7 — TLS.** A front-door certificate for the configured `publicHost` (name and IP SANs).
  Default: issued by the dev PKI the repo already has (`nix run .#pki-dev`, S4), whose root you
  trust once in the browser on `l`. A real certificate is a file option. The front door is HTTPS
  only, which also satisfies the agent's rule that a non-loopback `redirect_uris` entry is https.
  It sends `Strict-Transport-Security`, which doesn't depend on who issued the certificate.
- **F8 — Grafana's admin password from the repo.** Option `grafana.adminPasswordFile`, passed with
  `$__file{}` through `LoadCredential`, or `grafana.generateAdminPassword` for a random one kept
  in a root-only state directory. Grafana reads `admin_password` only when it creates its
  database, so a oneshot after each Grafana start runs `grafana cli admin reset-admin-password`
  from the same file (`--password-from-stdin`, never argv). The flag exists in the packaged
  Grafana: S25's VM test (#707) resets the password this way and signs in with it. Resetting on every start needs no stored hash, and a
  password changed in the UI goes back to the file's. This lands first (S25) because it is useful
  with or without the front door.
- **F9 — Everything else moves to loopback.** With `frontDoor.enable`, the module binds the portal
  server, the grpc-web listeners, Grafana and (via an option the host sets) Prometheus to
  `127.0.0.1`, stops opening the ports it opens today (8090–8093 and HyperDX's UI), and opens only
  8443 and 8444. 3000 and 9090 are opened by the host, not the module (l2's `lan-access.nix`), so
  the host drops those itself. Assertions refuse a front door while the host's firewall still
  opens a tool's port, whether listed in `allowedTCPPorts` or inside an `allowedTCPPortRanges`
  range, globally or on any interface, and refuse it outright with
  `networking.firewall.enable = false` or with any non-loopback interface in
  `networking.firewall.trustedInterfaces` (which opens every port on it), since the loopback
  binds alone don't cover HyperDX. That refusal is deliberate and will fire on common hosts
  (`tailscale0`, `virbr0`, container bridges): such a host stops trusting the interface and opens
  what it needs on it with `networking.firewall.interfaces.<name>.allowedTCPPorts`, which the
  assertions check like the global lists.
  The assertions read the NixOS firewall options only: rules added through `extraCommands`,
  `extraInputRules` or a hand-written nftables ruleset are invisible to them, so the host config
  review (l2's `lan-access.nix`) stays the backstop. HyperDX runs with `--network=host`, and all its listeners (UI 8080, API
  8000, OpAMP 4320, OTLP 4317 / 4318) bind every address, so their bind addresses must be checked
  against the image (open item); the firewall alone still closes them.
  Two listeners keep their binds. The REST transcoder (8094) is already fixed to `127.0.0.1`,
  whatever the grpc-web bind is, and is not routed through the front door in S25–S31. That is no
  regression: it is loopback-only today too, so a remote REST client keeps using an SSH tunnel
  (`ssh -L 8094:127.0.0.1:8094 <host>`) until a later increment routes it. The
  GitHub webhook receiver (8096, one exact route, `POST /webhooks/github`, HMAC-verified by the
  agent) must stay reachable by GitHub, which carries no cookie and doesn't trust the dev PKI.
  Today it shares the grpc-web listeners' one bind knob (`PORTAL_GRPC_WEB_HOST`), so S30 gives it
  its own: it stays on the host's address while the rest move to loopback. It is not behind the
  check, the module still doesn't open it (a host that takes deliveries does), and the assertions
  don't count it as a tool port.
- **F10 — CSRF.** `SameSite=Lax` stops cross-site sub-requests from carrying the cookie. grpc-web
  calls are non-simple requests (custom content type), so a cross-site page also needs a CORS
  preflight. The portal and its RPCs share :8443, so they need no CORS at all, and the front door
  allows no cross-origin request on either listener: in particular, a page on :8444 (HyperDX, a
  large app that renders log content) cannot make credentialed grpc-web calls to the agent. A
  host that genuinely needs another origin lists it in the existing `extraOrigins`.
  What remains is what SameSite allows: it compares the registrable domain, not the port, so a
  page on :8444 can still send same-site requests to :8443 with the cookie (top-level
  navigations, simple GETs, subresources). None of those can make a grpc-web call or read a
  cross-origin response. Of the gate's own routes, `/auth/logout` is POST-only, and the one GET
  that changes state, `/auth/callback`, redeems a code only for the browser that began that login
  (the nonce cookie, F3).
  The front door's CORS stays non-credentialed: today's `cors_policy` sets no
  `allow_credentials`, and an `extraOrigins` entry does not add it, so a listed origin still
  cannot make a cookie-carrying grpc-web call.
  Grafana keeps its own CSRF checks.
- **F11 — Return address.** `/auth/login?rd=` accepts only a path on the front door, by allowlist:
  run on the percent-decoded value, it starts with one `/` (not `//`), is at most 2048 bytes,
  and every byte is printable ASCII (0x21–0x7E) other than `\`, so no scheme, space, tab or
  other control byte; anything else becomes `/`. The value rides in the single-use `state` the agent already stores, never in a
  cookie. The bound is the gate's: `PendingTable` caps the state key (`MAX_STATE_BYTES`), not the value.
- **F12 — The agent reaches Dex on loopback.** Browsers use Dex's public issuer URL, but the agent
  should not hairpin through its own front door (and trust its certificate) to talk to Dex. A
  `generic` issuer gains `backchannel_url` (`http://127.0.0.1:5556/dex`): the agent fetches
  discovery there and redeems codes there, while the authorization URL it hands the browser and
  the `iss` it expects stay public. `backchannel_url` requires `jwks_url` (the loopback key set),
  so keys never come from the discovery document. The discovered token endpoint must sit under
  the issuer at a `/` boundary: the endpoint string must extend the public issuer's whole URL
  (scheme, host *and port*, path) with a `/`, and is then rewritten onto the back-channel. One
  that does not (another host, another port, plain http, or a prefix like `/dexfoo`) fails
  discovery, so sign-in through that issuer is
  refused rather than redeeming codes at the discovered host. Discovery is read at sign-in, not
  at config load, so that is where the refusal surfaces. So a discovery document cannot point
  the agent at another host for either. Discovery is fetched from `backchannel_url`, but the
  document's `issuer` is still compared with the public issuer, never with the loopback URL.
  The comparison is on the raw strings, so the issuer is configured exactly as Dex writes it: an
  explicit `:443` against an implicit one is refused, not normalized. The S26 `code_endpoints`
  table carries the rows (another host, another port, plain http, the `/dexevil` prefix, the
  issuer itself as the endpoint, a userinfo trick).

## What the agent gains (Rust)

- `crates/agent-grpc/src/server/auth/gate.rs`: the cookie session table (handle → `sid`, in the
  existing session store, so it survives restarts with `session_store = "file"` or `postgres`), the
  path → permission map, token minting and caching, and the three HTTP routes on a loopback
  listener (`tiny_http`, which the `auth` feature already pulls in for the CLI login's loopback
  redirect; hyper is not a direct dependency, and no web framework is added).
- The `ext_authz` check route (F4): the same listener, no protos (the repo vendors no Envoy
  protos; the gRPC flavour would need them).
- As built (S27b-ii): `[auth.gate]` (`listen`, loopback only; `issuer`; `main_origin`) is served
  by the `--serve-all` gateway alone, so a config every unit shares binds the port once. The
  listener (`gate/listener.rs`) answers `GET /auth/login`, `GET /auth/callback` and
  `/auth/logout` (POST; anything else 405), and the check at `/_check/main/<path>` and
  `/_check/hyperdx/<path>`: Envoy's `http_service` appends the original path to its
  `path_prefix`, so each listener's filter names its own prefix and no client header chooses it.
  The browser's routes answer `cache-control: no-store`. Two `authorization` headers are a 400;
  `Cookie` headers are joined with `; ` (RFC 9113 §8.2.3), so a planted second session cookie
  still shows as a repeat. No answer is written with a header value outside printable ASCII
  (a 500 instead), and the IdP's `sub` must be visible ASCII of at most 255 bytes where it is
  read, so `x-seddon-user` cannot split. The callback is `<main_origin>/auth/callback`, and must
  be one of `[auth] redirect_uris`; `main_origin` must be a canonical `https` origin.
- As built (S28a): the front door is rendered by `test/portal-envoy/portal_envoy.py` from
  `nix/portal/envoy-spec.nix`'s `front_door` section with `PORTAL_FRONT_DOOR=on`; F4's header
  table is `nix/portal/front-door-headers.json`, which the gate's Rust tests read too. Envoy's
  `--mode validate` does not resolve an `ext_authz` cluster, so a renderer row checks it.
- As built (S28b): `services.agentSeddon.frontDoor` turns it on in the module. The module sets
  the gateway's `[auth.gate]` (on the spec's gate port), Dex's issuer and callback, and F7's
  certificate: a dev-PKI leaf for `publicHost` (`pki-dev --san … --renew-within 720h`, root-only,
  re-minted daily under the same root when due) or the operator's `certFile` / `keyFile`, both
  as Envoy credentials. The host's `[auth]` stays the host's; the build refuses a front door
  without it. The VM walks Dex's mock sign-in through `:8443`.
- Tests: rstest tables for `Check` (no cookie, unknown handle, revoked session, expired session,
  lacking permission, each route's header set, header-injection attempts in the path, a forged
  `authorization` replaced by the session's, asserting exactly one `authorization` header reaches
  the agent, the same for a forged `X-Seddon-User` on `/grafana/`, an RPC denied as 401 not 302,
  an unmapped path denied, a page path denied as a 302 to `/auth/login?rd=<path>` (the
  counterpart of the RPC's 401), a cookie and an `authorization: Bearer` together (the cookie
  session wins and the client's token is replaced), the cached agent token re-minted a minute
  before expiry and not before, a valid cookie reaching HyperDX on :8444, a preflight from :8444 answered with no `Access-Control-Allow-Origin`
  (the browser does the refusing), a callback with no login cookie, the wrong nonce or another
  `state`'s nonce refused with no code redeemed, an unbound principal refused at the callback
  with no session row, and every denial written
  to the audit stream, as `audit::refused` already is for code exchanges) and for
  `rd` (empty and absent, both becoming `/`, `//evil`, `https://evil`, `/\evil`, encoded variants, `%0d%0a`, raw CR/LF and tab, since
  `rd` lands in the 302's `Location`, a 2049-byte value); every row asserts the
  refusal. The composed row runs the real RPC behind `Check`: a cookie-session caller naming
  another principal's session in `x-agent-session-id` is refused by the session's ownership check.

## Increments

| # | Increment | Lands |
|---|---|---|
| S25 | Grafana admin password from a file + reset oneshot (F8) | module, eval rows |
| S26 | Dex in the module (GitHub connector, secrets as files), the agent issuer wired, `backchannel_url` (F12) | module, Rust, tables, VM test with Dex's `mockCallback` connector |
| S26b | GitHub App from a manifest: `/setup` bootstrap, secrets to files, Dex restart | Rust, module, tables |
| S27a | `AuthGate` core: RBAC pairs (F5), the path map, `rd` (F11), browser cookie sessions (F3) | Rust, tables, catalogue |
| S27b-i | `AuthGate` route logic: login / callback / logout, `Check` (F4; F10's cookie attributes), the token cache | Rust, tables |
| S27b-ii | The gate's loopback listener, config and wiring | Rust, `authz.toml` rows, `auth-e2e` row |
| S28 | Envoy front door: TLS, `ext_authz`, routes, header stripping, CORS (F4, F7, F10) | `portal_envoy.py`, gate rows |
| S29 | Grafana `auth.proxy`, Prometheus sub-path, HyperDX port (F6) | module |
| S30 | Loopback lockdown and firewall (F9) | module assertions; l2 config change |
| S31 | Portal cookie mode (no token in JS) and tool links through the front door | portal, `portal-auth-e2e` |

A NixOS VM test carries the path, and each increment adds the rows it makes true, so no
increment is gated on a property a later one lands:
- S28: Dex with `mockCallback` signing in through the front door, an unbound user getting 403,
  an unmapped path getting 403, a path outside `/dex` never reaching a Dex route, and a denied
  `Check` showing up in the audit stream;
- S29: a signed-in request reaching Grafana as the right role, and a forged `X-Seddon-User`
  ignored;
- S30: every tool port closed from outside the VM.

## Creating the GitHub App from the portal (S26b)

Dex only signs people in. What makes GitHub setup automatic is GitHub's
[App manifest flow](https://docs.github.com/en/apps/sharing-github-apps/registering-a-github-app-from-a-manifest):
the agent posts a manifest (name, callback and webhook URLs, permissions), the operator clicks
*Create* once on github.com, and GitHub redirects back with a one-hour code that the agent trades
(`POST /app-manifests/{code}/conversions`) for the App's id, private key, webhook secret and OAuth
client id and secret. Installing the App on repositories is already built (forge-auth F5d1, the
Forges tab's Connect GitHub); creating it is the manual step today (l2's App was made by hand).

- **One App, two jobs.** The App's OAuth client is also Dex's GitHub connector, so there is no
  separate OAuth App. GitHub Apps use the same `login/oauth/authorize` web flow; the manifest asks
  for the account permission *Email addresses: read* so Dex can read the verified email F2 matches
  on. (Open item: confirm Dex's connector works with a GitHub App's client, user-to-server tokens.)
- **Bootstrap, before anyone can sign in.** With no App, Dex has no GitHub client and nobody can
  sign in, so the first creation cannot sit behind the sign-in. `/setup` on the front door is open
  only while no App exists, and only with a single-use setup code the agent writes to its journal
  at start (as Grafana and Jenkins do); the code expires in an hour and dies once used. After it
  is used, `/setup` answers 404 for good, and the same flow later (rotate, recreate) needs the
  operator role.
- **Where the secrets go.** The conversion response is written straight to files in the agent's
  state directory (mode 0600): the private key and webhook secret for `[github_app]`, the OAuth
  client secret for Dex. None of it is logged, returned to the browser, or put in the store. A
  systemd path unit restarts Dex when its secret file appears or changes, since Dex reads its
  connector config only at start.
- **Then the existing flows.** The operator signs in through Dex (now working), installs the App on
  repositories from the Forges tab, and adds fleet sessions. Other people sign in with their own
  GitHub accounts and see nothing until bound (F2).
- **Checks.** The manifest's URLs are built from `publicHost`, never from the request's `Host`.
  The `state` round-trips like the sign-in's. The conversion runs once per code. Tables cover a
  replayed code, a wrong `state`, a reused setup code, `/setup` after an App exists, and a
  conversion reply over the size cap.

## On l2

l2 needs the dev PKI root trusted in the browser on `l`. Until S26b lands, Dex's GitHub client is
made by hand (callback `https://172.16.50.46:8443/dex/callback`) with its secret in a root-only
file; after it, `/setup` creates the App instead. l2's existing hand-made App (id 5390283) keeps
working for reviews either way. The switch to the front door is
one l2 deploy (S30), with your go-ahead; `~/nixos/desktop/l2/lan-access.nix` then stops opening
3000 and 9090, and the module stops opening 8080.

## Open items

- Dex's GitHub connector with a GitHub App's OAuth client (S26b).
- Where `/setup` runs (S26b). The agent has no plain-HTTP server that could host it: the fleet's
  webhook receiver runs only under `--serve-fleet`, and only once a webhook secret exists, which
  is what the App creates. S27's `AuthGate` serves HTTP for login anyway, so S26b comes after S27
  and adds `/setup` there. The module also has to take the GitHub client id from a file, since a
  manifest-made App's id isn't known at eval.
- `agent login --device` dials the public issuer and ignores `backchannel_url`
  (`crates/agent-grpc/src/client/login.rs`). Behind the front door the public issuer is
  reachable, so this only matters for a CLI on the host itself; settle it with S27's token rules.
- HyperDX's listeners under `--network=host` (can they be loopback-only?), and whether its UI can
  serve under a sub-path, which would fold :8444 into :8443. Also whether its UI calls the API
  (8000) from the browser or proxies it server-side: if from the browser, :8444 routes the API
  too, or S30's lockdown breaks the UI.
- Whether Prometheus should be managed by the module or stay the host's, with the module only
  asserting it listens on loopback.
- Grafana's `auth.proxy` in the packaged version (nixpkgs' `grafana` 13.3.0 at the flake's pin):
  the option names F6 uses (`whitelist` in particular), and that role sync updates the role on
  each request.
- `/prometheus/` needs the host's Prometheus started with `--web.external-url` (and the matching
  route prefix) set to the front door's path; S29 documents it, or sets it if the module comes to
  own Prometheus.
