# 04 — Nix tooling & ports

Everything the portal needs comes from the flake — no host installs — matching the
repo's hermetic-toolchain rule ([`CLAUDE.md`](../../../CLAUDE.md)). This part adds
the Flutter/Dart tooling, the optional web-only proxy, the run apps, and the port
block.

## Tool pins

`nixpkgs` is `nixos-unstable` (`flake.nix`), which carries `flutter`, `dart`, and
`protoc-gen-dart`. In [`nix/versions.nix`](../../../nix/versions.nix) (the single
source of truth for versions):

- add `flutter`, `dart` (a **desktop-Linux**-enabled Flutter),
- add `protoc-gen-dart` (used by [`gen-dart`](02-dart-codegen.md)).

Add them to `allDevPackages` in [`nix/packages.nix`](../../../nix/packages.nix) so
`nix develop` has the SDK + plugin on `PATH`.

## Ports (`nix/constants.nix`)

`nix/constants.nix` is authoritative; `nix run .#gen-constants` renders it into
`crates/agent-grpc/src/constants.rs` and the `constants-sync` check fails on drift.
Add three seam rows after `dimension` (50076) and one proxy port:

| Key | gRPC port | UDS | metrics port |
|---|---|---|---|
| `prompt` | 50077 | `/tmp/agent-seddon/prompt.sock` | 9627 |
| `session_stream` | 50078 | `/tmp/agent-seddon/session-stream.sock` | 9628 |
| `metrics_proxy` | 50079 | `/tmp/agent-seddon/metrics-proxy.sock` | 9629 |
| `grpc_web_proxy` | **8090** (HTTP/1.1 grpc-web) | — | — |

The seam rows flow through `gen-constants` into `constants.rs` (and the Prometheus
scrape config) like every other seam; the `grpc_web_proxy` port is UI-plumbing, read
by the proxy app and the Flutter web `PortalConfig` default.

## Run apps (`nix/portal/default.nix`, new)

Modelled on the existing docker-app / `writeShellApplication` pattern
(`prometheus-up`, `grafana-up`, `buf-image`):

```sh
nix run .#gen-dart        # regenerate Dart stubs (see 02)
nix run .#portal          # build + launch the Flutter app
nix run .#grpc-web-up     # OPTIONAL — web build only: grpc-web proxy in front of :50100
nix run .#grpc-web-down
```

### The grpc-web proxy is web-only, and optional

Browsers can't speak raw gRPC (HTTP/2 trailers), so the **web** build needs a
grpc-web ↔ gRPC translator in front of the gateway. `grpc-web-up` runs a minimal
`grpcwebproxy` (or Envoy) that:

- listens on `:8090` (HTTP/1.1 + grpc-web),
- forwards to the `--serve-all` gateway on `:50100`,
- binds **loopback only** — no new trust boundary; it's a same-host protocol shim.

The **native desktop** build dials `:50100` directly and needs none of this — so the
proxy is documented as an optional, web-only dependency, not part of the default run
path.

## Bringing it all up

```sh
# one gateway hosts every seam, including the three new ones:
nix run .#agent -- --serve-all --config config/agent.toml     # gRPC on :50100

# native desktop portal (no proxy):
nix run .#portal

# …or the web build:
nix run .#grpc-web-up        # :8090 → :50100
nix run .#portal             # (web target) served at a localhost URL

# observability stack the Launcher links to (unchanged):
nix run .#prometheus-up      # :9090  (MetricsProxyService's upstream)
nix run .#grafana-up         # :3000
nix run .#clickstack-up      # :8080
```

`MetricsProxyService` reads Prometheus at `:9090`, so `prometheus-up` is its upstream
(the proxy fails soft if it's down — an empty result + `error`, per
[`01`](01-backend-seams.md)). The pool cell and prompt/session panels need only the
gateway.

## What lands where

| File | Change |
|---|---|
| `nix/versions.nix` | pin `flutter`, `dart`, `protoc-gen-dart` |
| `nix/packages.nix` | add them to `allDevPackages` |
| `nix/constants.nix` (+ `nix/gen-constants.nix`) | seam rows 50077–50079, `grpc_web_proxy` 8090 → regenerate `constants.rs` |
| `buf.gen.yaml` (new, root) | Dart codegen config (see [`02`](02-dart-codegen.md)) |
| `nix/portal/default.nix` (new) | `gen-dart`, `portal`, `grpc-web-up/down` apps |
| `nix/default.nix` | register the new apps |
