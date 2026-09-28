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
