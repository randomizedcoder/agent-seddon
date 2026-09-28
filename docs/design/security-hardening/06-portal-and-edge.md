# 06 — Portal login and Envoy hardening

Closes P0-4: Envoy binds `0.0.0.0`, allows every origin, omits `authorization`, has no `jwt_authn`;
the portal hardcodes its identity.

## Today

- Envoy is one nix heredoc ([`nix/portal/default.nix`](../../../nix/portal/default.nix):186-515),
  image `envoyproxy/envoy:v1.31-latest` ([`versions.nix`](../../../nix/versions.nix):152), three
  listeners on `0.0.0.0` (:8090 / :8091 / :8093), filters `grpc_web → cors → router`, CORS
  `allow_origin_string_match: prefix: "*"`, `allow_headers` without `authorization` (:254-262 and
  twins), no `jwt_authn`, no TLS, plain h2 to `127.0.0.1`. Only `${OTLP_AUTHORIZATION}` is
  env-substituted (:541-546); the container runs `--network host`.
- Portal: grpc-dart `^5.1.0`, `GrpcWebClientChannel.xhr`
  ([`channel_web.dart`](../../../portal/lib/src/transport/channel_web.dart):9-20); clients built once
  in `PortalClients` with no interceptors ([`clients.dart`](../../../portal/lib/src/clients.dart):30-64);
  identity is the hardcoded `'x-agent-user-id': 'portal'`
  ([`agent_view_page.dart`](../../../portal/lib/src/pages/agent_view_page.dart):106-109). Config via
  `--dart-define` keys in [`nix/portal/default.nix`](../../../nix/portal/default.nix):71-76; served by
  `static-web-server` (:154-169) with no SPA fallback.

## Portal login

- **`LoginPage`** gates the `NavigationRail` shell: pick an issuer (from `PORTAL_AUTH_ISSUER`, or the
  list `AuthService.Issuers` returns), generate the PKCE verifier, call `Begin`, redirect.
- **Callback** = the portal's own origin with `?code&state` (no new route, so `static-web-server`
  needs no SPA fallback); on load the app calls `Exchange`, then `history.replaceState` to strip
  the query.
- **`AuthState`** (`ChangeNotifier`): `accessToken`, `expiresAt`, `refreshHandle`, `principal`
  and `permissions` from `WhoAmI`. Token in memory plus `sessionStorage`; refresh one minute before
  expiry; an expiry banner when refresh fails; sign-out calls `Logout`.
- **`AuthInterceptor implements ClientInterceptor`** adds `authorization: Bearer …` on every unary
  and streaming call, for both channel implementations; the hardcoded `'portal'` user is replaced by
  the verified tenant (now advisory only, [05](05-identity-and-tenancy.md)). Since S15c it also
  adds `x-agent-session-id` = the auth session id (`sid`): a call with a token carries a principal,
  so the agent refuses scoped services that name no session. A header the call sets itself (the
  Agent page's session) is kept.
- **Capability-aware UI:** Approve, Fleet edit forms, Router edits and the Roles page render only
  when `permissions` contains the matching pair ([03](03-rbac.md)); the server still enforces.
- `--dart-define`: `PORTAL_AUTH_ISSUER` (issuer *name*), `PORTAL_AUTH=off` for loopback dev; both
  wired into the nix build.

**As built (S13a).** The server half is in: `AuthService.Issuers`, `AuthService.Begin` and
`Exchange{code, state, code_verifier}`, with `[auth] redirect_uris` as the exact return URIs
([`docs/grpc.md`](../../grpc.md), "A person in a browser"). The agent generates the `nonce` and
the single-use `state`; the portal generates only the PKCE verifier. The portal half is S13b.

**As built (S13b).** The portal half, in
[`portal/lib/src/auth/`](../../../portal/lib/src/auth/):

- `PORTAL_AUTH` is `auto` by default, not `on`. `auto` asks `Issuers`; when the agent offers no
  browser sign-in (or is older than S13a and answers `UNIMPLEMENTED`) the portal runs as before,
  with no bearer. `on` insists on sign-in; `off` never asks. `PORTAL_REDIRECT_URI` overrides
  the return address (default: the page's own origin and path), which must be in the agent's
  `[auth] redirect_uris`.
- The pending sign-in (`state`, verifier, issuer) and the session (token, expiry, refresh handle)
  live in `sessionStorage`, one tab each. A callback whose `state` is not the one this tab
  stored is never exchanged; the stored one is spent either way, and the query is stripped
  before anything else happens. A reload with a live session confirms it with `WhoAmI`; a
  lapsed one is refreshed.
- `AccountStrip` at the foot of the navigation rail: who is signed in (tooltip) and sign out.
- Capability-aware controls: Fleet Approve (`approve:review`), draft edits (`write:review`),
  Review now (`trigger:fleet`), the enable switch (`write:fleet`); Router add / save / enable
  (`write:registry`) and delete (`delete:registry`). `perms_ref` tokens show everything.
  There is no Roles page yet.
- The Agent tab's `OpenRequest.user` and `x-agent-user-id` carry the verified tenant (advisory).
- Envoy's CORS `allow_headers` gains `authorization`, so the bearer survives the preflight; the
  rest of the Envoy hardening is S14.
- Native desktop cannot redirect, so it shows an error rather than a sign-in button that goes
  nowhere. Reading the CLI's stored login (S12) there is a follow-up.

## Envoy hardening

Same heredoc, env knobs with safe defaults:

| Knob | Default | Effect |
|---|---|---|
| `PORTAL_GRPC_WEB_HOST` | `127.0.0.1` | listener bind (all three) |
| `PORTAL_WEB_ORIGIN` | `http://127.0.0.1:8092` | CORS `exact` match; `authorization` added to `allow_headers` |
| `PORTAL_JWT_ISSUER`, `PORTAL_JWT_JWKS`, `PORTAL_JWT_AUDIENCE` | the agent's `[auth.token]` values | one `jwt_authn` provider (`remote_jwks`, `cache_duration`, `forward: true`) |
| `PORTAL_AUTH` | `auto` (as built; designed as `on`) | `off` renders the `jwt_authn` filter out (loopback dev) |
| `PORTAL_TLS_CERT` / `PORTAL_TLS_KEY` | unset | listener `DownstreamTlsContext` (step-ca certificates) |
| `PORTAL_UPSTREAM_MTLS` | unset | `UpstreamTlsContext` with Envoy's service certificate, so the agent sees `peer_san = svc:envoy` |

`jwt_authn` bypass rules: `/agent.v1.AuthService/*`, `/grpc.health.*`, `/grpc.reflection.*`. The
edge check is defense in depth; `AuthLayer` in the agent is the enforcement point
([04](04-service-integration.md)). When REST lands, the same listener adds `grpc_json_transcoder`
ahead of `grpc_web`; auth is unchanged.

**As built (S14).** The heredoc is gone. [`nix/portal/envoy-spec.nix`](../../../nix/portal/envoy-spec.nix)
holds the listener table (and is now the single source of the bridge ports);
[`test/portal-envoy/portal_envoy.py`](../../../test/portal-envoy/portal_envoy.py) renders the
bootstrap as JSON (Envoy reads JSON as YAML, and `json.dumps` means no knob can escape its
string), and `grpc-web-up` is a shim that runs it and starts the container.

- Bind: `PORTAL_GRPC_WEB_HOST`, an IP literal, default `127.0.0.1`, applied to all three
  listeners. A non-loopback bind prints a note naming the allowed origins.
- CORS: `PORTAL_WEB_ORIGIN` is a comma list of exact origins, normalised to
  `scheme://host[:port]`. `*`, `null`, paths, queries, credentials and control characters are
  refused. `authorization` stays in `allow_headers`.
- `jwt_authn`: one provider (`agent`), `forward: true`, `bypass_cors_preflight`, placed after
  `cors` (so the preflight is answered and the 401 carries CORS headers) and before `router`.
  The three bypass prefixes skip it; everything else requires it. The agent answers
  `AuthService.Jwks` only over gRPC, so by default the renderer calls it with `grpcurl` (using
  the committed `auth.proto`, not reflection) and inlines the key set as `local_jwks`.
  `PORTAL_JWT_JWKS` may instead name a file or an https (loopback http) URL, which becomes
  `remote_jwks` with its own cluster. A key set with private members (`d`, `k`, …) is refused.
  `PORTAL_JWT_ISSUER` / `PORTAL_JWT_AUDIENCE` are optional; unset, only the agent checks them.
- `PORTAL_AUTH` defaults to `auto`, matching the portal: `auto` renders `jwt_authn` when the
  agent serves a non-empty key set and omits it (with a note) when the agent answers
  `UNIMPLEMENTED`; `on` refuses to start without keys; `off` never asks. In every mode an
  unreachable agent is an error after `PORTAL_JWKS_WAIT` seconds (default 30): the edge cannot
  tell "no tokens" from "not up yet", and guessing would fail open. Key rotation needs a
  re-run of `grpc-web-up` (the agent keeps the previous key in its set for one token lifetime).
- TLS: `PORTAL_TLS_CERT` / `_KEY` add a `DownstreamTlsContext` (ALPN h2, http/1.1) to every
  listener. `PORTAL_UPSTREAM_CA` adds an `UpstreamTlsContext` to the three agent clusters
  (not the OTLP one), checking the DNS SAN `PORTAL_UPSTREAM_SNI` (default `localhost`, which
  `nix run .#pki-dev` leaves carry); `_CERT` / `_KEY` add Envoy's client certificate. The
  files are mounted read-only under `/etc/envoy/tls/`.
- The rendered file holds the OTLP ingestion key, so it is written `0600` and the container runs
  as the invoking user (`--user`, plus `--userns keep-id` under podman).
- Image `envoyproxy/envoy:v1.39-latest` (was v1.31) so the gate validates the version that runs:
  `versions.envoy-bin` is the cached upstream 1.39 binary.
- `portal-redeploy` probes `grpc.health.v1.Health/Check` through the bridge (bypassed by
  `jwt_authn`) and fails on a non-zero `grpc-status`; `portal-e2e` pins `flutter drive` to
  `127.0.0.1:8097` and allows exactly that origin.
- Gate: the `portal-envoy` check runs the renderer's tables, then `envoy --mode validate` on
  five modes (auth off, local JWKS, remote JWKS, LAN bind with two origins, TLS + upstream
  mTLS + auth) over the real spec, and check-the-checks: a corrupt JWKS, an unknown provider
  and a missing key file must fail validate.
- Verified live on l2 (podman, side ports): a foreign origin gets no
  `access-control-allow-origin`; no token and a garbage token get `grpc-status: 16`; a token
  signed by the served key passes to the upstream; a wrong `iss` is refused; AuthService and
  health pass without a token; the TLS listener answers over https with the dev CA.
- Not in S14: `portal-e2e` under auth needs an agent that issues tokens from a fake issuer,
  which is S15's harness, so it moves there. The REST transcoder (rest-openapi PR-05) now
  lands as a filter in the renderer.

## Tests

- **Layer A (hermetic widget tests):** `FakeGateway`
  ([`fake_gateway.dart`](../../../portal/test/testkit/fake_gateway.dart):33-39) gains a fake
  `AuthService` and a server interceptor that asserts `authorization` on every non-auth call
  (`RecordedCall` grows `metadata`); "not signed in", "expired", "refresh failed" states;
  capability-hidden controls per role.
- **Layer B under auth (`nix run .#portal-auth-e2e`, S15c):** headless Chromium over WebDriver
  against the real web build, a fake IdP with the code flow, the hardened bridge and an mTLS
  agent; asserts the login round trip, a scoped page loading, resume, callback replay / forgery
  and IdP refusal, and sign-out revoking the session. `flutter drive` cannot follow the IdP
  redirect (it would end the test), so `portal-e2e` stays the anonymous Layer B.
- **Gate:** `envoy --mode validate` on the rendered YAML for both `PORTAL_AUTH` values.

| Class | Case | Expect |
|---|---|---|
| positive | `positive_login_round_trip_sets_bearer` | every recorded call carries `authorization` |
| positive | `positive_envoy_config_validates_both_modes` | `--mode validate` OK |
| negative | `negative_unsigned_in_shell_hidden` | rail not rendered before login |
| negative | `negative_origin_not_allowed_blocked` | CORS preflight from another origin → no `access-control-allow-origin` |
| boundary | `boundary_refresh_one_minute_before_expiry` | refresh fires at `exp − 60 s` |
| corner | `corner_auth_off_renders_no_jwt_filter` | rendered YAML has no `jwt_authn` |
| adversarial | `adversarial_callback_state_mismatch_rejected` | foreign `state` → login page with error, no exchange |
| adversarial | `adversarial_review_viewer_sees_no_approve_button` | control absent; a forged call still `PERMISSION_DENIED` |
