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
  the verified tenant (now advisory only, [05](05-identity-and-tenancy.md)).
- **Capability-aware UI:** Approve, Fleet edit forms, Router edits and the Roles page render only
  when `permissions` contains the matching pair ([03](03-rbac.md)); the server still enforces.
- `--dart-define`: `PORTAL_AUTH_ISSUER` (issuer *name*), `PORTAL_AUTH=off` for loopback dev; both
  wired into the nix build.

## Envoy hardening

Same heredoc, env knobs with safe defaults:

| Knob | Default | Effect |
|---|---|---|
| `PORTAL_GRPC_WEB_HOST` | `127.0.0.1` | listener bind (all three) |
| `PORTAL_WEB_ORIGIN` | `http://127.0.0.1:8092` | CORS `exact` match; `authorization` added to `allow_headers` |
| `PORTAL_JWT_ISSUER`, `PORTAL_JWT_JWKS`, `PORTAL_JWT_AUDIENCE` | the agent's `[auth.token]` values | one `jwt_authn` provider (`remote_jwks`, `cache_duration`, `forward: true`) |
| `PORTAL_AUTH` | `on` | `off` renders the `jwt_authn` filter out (loopback dev) |
| `PORTAL_TLS_CERT` / `PORTAL_TLS_KEY` | unset | listener `DownstreamTlsContext` (step-ca certificates) |
| `PORTAL_UPSTREAM_MTLS` | unset | `UpstreamTlsContext` with Envoy's service certificate, so the agent sees `peer_san = svc:envoy` |

`jwt_authn` bypass rules: `/agent.v1.AuthService/*`, `/grpc.health.*`, `/grpc.reflection.*`. The
edge check is defense in depth; `AuthLayer` in the agent is the enforcement point
([04](04-service-integration.md)). When REST lands, the same listener adds `grpc_json_transcoder`
ahead of `grpc_web`; auth is unchanged.

## Tests

- **Layer A (hermetic widget tests):** `FakeGateway`
  ([`fake_gateway.dart`](../../../portal/test/testkit/fake_gateway.dart):33-39) gains a fake
  `AuthService` and a server interceptor that asserts `authorization` on every non-auth call
  (`RecordedCall` grows `metadata`); "not signed in", "expired", "refresh failed" states;
  capability-hidden controls per role.
- **Layer B (`nix run .#portal-e2e`):** runs under the testkit fake issuer with `PORTAL_AUTH=on`;
  asserts the login round trip and one authorized read.
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
