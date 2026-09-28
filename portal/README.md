# Agent Portal (Flutter · gRPC-only)

The portal client for `agent-seddon` — see [`docs/design/portal/`](../docs/design/portal/README.md).
It talks **gRPC only**, over the `--serve-all` gateway (`:50100`): a Launcher for the
observability UIs, a Prompts CRUD editor, and a live Agent View.

## Layout

```
portal/
├── lib/src/gen/        # generated Dart gRPC stubs (committed) — regenerate with `nix run .#gen-dart`
├── lib/                # the app (transport, pages) — increment 06
└── pubspec.yaml        # the Dart/Flutter project — increment 06
```

## Codegen (increment 05, this)

The stubs under `lib/src/gen/` are generated from the `.proto` contracts by
[`buf.gen.yaml`](../buf.gen.yaml) + `protoc-gen-dart` and **committed**. Regenerate
after a wire change:

```sh
nix run .#gen-dart      # buf generate → portal/lib/src/gen/
```

The same generated stubs serve both builds — only the *channel* differs (native
`ClientChannel` vs web `GrpcWebClientChannel`).

## Running (increment 06)

```sh
agent --serve-all                         # the gRPC gateway on :50100
nix run .#portal                          # native desktop (dials :50100 directly)
# …or the web build, behind the grpc-web proxy (browsers can't speak raw gRPC):
nix run .#grpc-web-up                      # envoy: grpc-web :8090 → gateway :50100
nix run .#portal -- -d chrome
```

### Sign-in (security-hardening S13b)

When the agent offers browser sign-in (`[auth] redirect_uris` lists the portal's address), the web
portal shows a sign-in page first and every call then carries the user's agent token. Build-time
knobs (`--dart-define`, wired in `nix/portal/default.nix`):

| Define | Default | Meaning |
|---|---|---|
| `PORTAL_AUTH` | `auto` | `auto` signs in when the agent offers it; `on` insists; `off` never asks |
| `PORTAL_AUTH_ISSUER` | empty | offer only this login issuer when the agent lists it |
| `PORTAL_REDIRECT_URI` | the page's own address | where the IdP returns; must be in `[auth] redirect_uris` |

The native desktop build cannot redirect to an IdP; run it with `PORTAL_AUTH=off` against a
loopback agent.

### The grpc-web bridge (security-hardening S14)

`nix run .#grpc-web-up` renders the Envoy config at bring-up
([`test/portal-envoy/portal_envoy.py`](../test/portal-envoy/portal_envoy.py)) from `PORTAL_*`
environment knobs whose defaults are the safe ones:

| Knob | Default | Meaning |
|---|---|---|
| `PORTAL_GRPC_WEB_HOST` | `127.0.0.1` | bind for all three listeners (`0.0.0.0` for LAN browsers) |
| `PORTAL_WEB_ORIGIN` | `http://127.0.0.1:8092,http://localhost:8092` | the exact origins CORS allows; no wildcards |
| `PORTAL_AUTH` | `auto` | `auto`: check agent tokens at the edge when the agent issues them; `on`: insist; `off`: never |
| `PORTAL_JWT_JWKS` | fetched from the gateway | a JWKS file or https URL instead of `AuthService.Jwks` |
| `PORTAL_JWT_ISSUER` / `PORTAL_JWT_AUDIENCE` | unset | also check `iss` / `aud` at the edge |
| `PORTAL_TLS_CERT` / `PORTAL_TLS_KEY` | unset | serve the listeners over TLS |
| `PORTAL_UPSTREAM_CA` (+ `_CERT` / `_KEY`, `_SNI`) | unset | TLS (mTLS) from Envoy to the agent |

Start the gateway first: with `PORTAL_AUTH=auto` the bridge asks it for its JWKS, waits up to
`PORTAL_JWKS_WAIT` seconds (30), and refuses to start rather than guess. `flutter run -d chrome`
serves from a random port, so pass `-- --web-port 8092` (or add its origin to
`PORTAL_WEB_ORIGIN`).
