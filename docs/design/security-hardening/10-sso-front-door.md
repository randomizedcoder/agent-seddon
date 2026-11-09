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
  so a login with one stops at the agent.
- **F3 — The agent owns the browser session (backend-for-frontend).** A new `AuthGate` in the
  agent runs the code flow itself: `/auth/login` calls the existing `Begin` logic and redirects to
  Dex; `/auth/callback` runs the existing code `Exchange`, opens the usual auth session and sets
  one cookie. The cookie holds an opaque random handle (256 bits) mapped to the auth session's
  `sid`, never a JWT, so a revoked session (`Logout`, `RevokeSession`) stops at the next request.
  The table stores only the handle's hash, as refresh handles already do (`session.rs`), so a
  copy of the session store yields no live browser session. Each entry belongs to its auth
  session: it counts against the same per-tenant cap (`DEFAULT_MAX_SESSIONS_PER_TENANT`) and goes
  when that session ends, is revoked or is garbage-collected at expiry.
  Cookie: `__Host-seddon`, `HttpOnly`, `Secure`, `SameSite=Lax`, `Path=/`, no `Domain`. Because
  cookies are not port-scoped, the same cookie covers :8443 and :8444.
- **F4 — Envoy checks every request with `ext_authz`**, the HTTP flavour (`http_service`): Envoy
  sends the request's headers to `AuthGate`'s check route on loopback and gets 200 (plus headers
  to add), 302 or 403 back. It needs no protos, and the gate's routes are plain hyper anyway.
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

  What Envoy removes from the client request depends on the route, so a client cannot forge an
  identity and nothing a route needs is lost. Route-level `request_headers_to_remove` is static
  and runs in the router, after `ext_authz`, so it would also drop any header `Check` adds. A
  header `Check` supplies is therefore never removed on that route; `Check`'s 200 always carries
  it, listed in `allowed_upstream_headers`, which overwrites the client's value:
  - every route: `x-agent-user-id` (the agent's auth layer already overwrites it from the
    verified token, S2), and `x-seddon-*` on every route except `/grafana/`;
  - `/grafana/`: `X-Seddon-User` and `X-Seddon-Role` are not removed but replaced: `Check`'s 200
    always carries both, so a forged value never reaches Grafana. Any other `x-seddon-*` header
    is removed there too;
  - grpc-web routes: `authorization` is replaced the same way (`Check`'s 200 always carries it,
    above). Whatever reaches the agent is what `Check` chose. `x-agent-session-id` is kept: the portal names its
    session with it, and the RPC's own authorization decides access (S7);
  - tool routes: `authorization` and `cookie`, so the tools never see the session handle;
  - `/auth/*`: nothing, since logout needs the cookie to find the session to end.

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
- **F6 — Grafana uses `auth.proxy`.** `header_name = X-Seddon-User`, `headers = Role:X-Seddon-Role`,
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
  from the same file (`--password-from-stdin`, never argv; S25's VM test signs in with it). Resetting on every start needs no stored hash, and a
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
  binds alone don't cover HyperDX.
  The assertions read the NixOS firewall options only: rules added through `extraCommands`,
  `extraInputRules` or a hand-written nftables ruleset are invisible to them, so the host config
  review (l2's `lan-access.nix`) stays the backstop. HyperDX runs with `--network=host`, and all its listeners (UI 8080, API
  8000, OpAMP 4320, OTLP 4317 / 4318) bind every address, so their bind addresses must be checked
  against the image (open item); the firewall alone still closes them.
  Two listeners keep their binds. The REST transcoder (8094) is already fixed to `127.0.0.1`,
  whatever the grpc-web bind is, and is not routed through the front door in S25–S31. The
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
  cross-origin response, and the gate's own routes change state only on POST.
  `/auth/logout` is POST-only.
  Grafana keeps its own CSRF checks.
- **F11 — Return address.** `/auth/login?rd=` accepts only a path on the front door (starts with
  one `/`, no `//`, no scheme, no backslash, at most 2048 bytes); anything else becomes `/`. The
  value rides in the single-use `state` the agent already stores, never in a cookie. The bound
  is the gate's: `PendingTable` caps the state key (`MAX_STATE_BYTES`), not the value.
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

## What the agent gains (Rust)

- `crates/agent-grpc/src/server/auth/gate.rs`: the cookie session table (handle → `sid`, in the
  existing session store, so it survives restarts with `session_store = "file"` or `postgres`), the
  path → permission map, token minting and caching, and the three HTTP routes on a loopback
  listener (plain hyper, which the workspace already uses; no web framework added).
- The `ext_authz` check route (F4): the same hyper listener, no protos (the repo vendors no Envoy
  protos; the gRPC flavour would need them).
- Tests: rstest tables for `Check` (no cookie, unknown handle, revoked session, expired session,
  lacking permission, each route's header set, header-injection attempts in the path, a forged
  `authorization` replaced by the session's, asserting exactly one `authorization` header reaches
  the agent, the same for a forged `X-Seddon-User` on `/grafana/`, an RPC denied as 401 not 302,
  an unmapped path denied, a cross-origin preflight from :8444 refused, and every denial written
  to the audit stream, as `audit::refused` already is for code exchanges) and for
  `rd` (`//evil`, `https://evil`, `/\evil`, encoded variants, `%0d%0a` and raw CR/LF, since `rd`
  lands in the 302's `Location`, a 2049-byte value); every row asserts the
  refusal. The composed row runs the real RPC behind `Check`: a cookie-session caller naming
  another principal's session in `x-agent-session-id` is refused by the session's ownership check.

## Increments

| # | Increment | Lands |
|---|---|---|
| S25 | Grafana admin password from a file + reset oneshot (F8) | module, eval rows |
| S26 | Dex in the module (GitHub connector, secrets as files), the agent issuer wired, `backchannel_url` (F12) | module, Rust, tables, VM test with Dex's `mockCallback` connector |
| S26b | GitHub App from a manifest: `/setup` bootstrap, secrets to files, Dex restart | Rust, module, tables |
| S27 | `AuthGate`: login / callback / logout, cookie sessions, `Check` (F3, F4, F11; F10's cookie attributes); RBAC pairs (F5) | Rust, tables, catalogue, `authz.toml` rows, `auth-e2e` row |
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
- HyperDX's listeners under `--network=host` (can they be loopback-only?), and whether its UI can
  serve under a sub-path, which would fold :8444 into :8443.
- Whether Prometheus should be managed by the module or stay the host's, with the module only
  asserting it listens on loopback.
- Grafana's `auth.proxy` role sync: confirm it updates the role on each request in the packaged
  version (nixpkgs' `grafana` 13.3.0 at the flake's pin).
- `/prometheus/` needs the host's Prometheus started with `--web.external-url` (and the matching
  route prefix) set to the front door's path; S29 documents it, or sets it if the module comes to
  own Prometheus.
