# 04 — Nix tooling & ports

Everything the portal needs comes from the flake — no host installs — matching the
repo's hermetic-toolchain rule ([`CLAUDE.md`](../../../CLAUDE.md)). This part adds
the Flutter/Dart tooling, the optional web-only proxy, the run apps, and the port
block.

## Tool pins

`nixpkgs` carries `flutter`, `dart`, and `protoc-gen-dart`. In
[`nix/versions.nix`](../../../nix/versions.nix) (the single source of truth):

- `protoc-gen-dart` — used by [`gen-dart`](02-dart-codegen.md) (small, cached binary),
- `flutter` + `dart` — for the app (`nix run .#portal`, increment 06),
- `envoyImage` — a **pinned docker image tag** for the grpc-web proxy (run as a
  container, so the gate never source-builds envoy).

**Deliberately NOT added to `allDevPackages`.** Flutter is a large toolchain;
bloating the lean Rust dev shell with it (and envoy) is the wrong trade. The opt-in
portal apps supply their tools lazily via `runtimeInputs`, so a Rust-only `nix
develop` stays fast. (This corrects the earlier "add to allDevPackages" note.)

## Ports (`nix/constants.nix`)

`nix/constants.nix` is authoritative; `nix run .#gen-constants` renders it into
`crates/agent-grpc/src/constants.rs` and the `constants-sync` check fails on drift.
Add three seam rows after `dimension` (50076) and one proxy port:

| Key | gRPC port | UDS | metrics port |
|---|---|---|---|
| `prompt` | 50077 | `/tmp/agent-seddon/prompt.sock` | 9627 |
| `session_stream` | 50078 | `/tmp/agent-seddon/session-stream.sock` | 9628 |
| `metrics_proxy` | 50079 | `/tmp/agent-seddon/metrics-proxy.sock` | 9629 |

The three seam rows (landed in increments 02–04) flow through `gen-constants` into
`constants.rs` like every other seam. The **grpc-web proxy port (`8090`)** is *not* a
seam — it is UI plumbing that would only clutter the seam table (which the
`constants-sync` check renders verbatim), so it lives as a plain binding in
[`nix/portal/default.nix`](../../../nix/portal/default.nix) instead, read by the
proxy app and the Flutter web `PortalConfig` default.

## Run apps (`nix/portal/default.nix`, new)

Modelled on the existing docker-app / `writeShellApplication` pattern
(`prometheus-up`, `grafana-up`, `buf-image`):

```sh
nix run .#gen-dart        # regenerate the committed Dart stubs (see 02)
nix run .#grpc-web-up     # OPTIONAL — web build only: envoy grpc-web proxy in front of :50100
nix run .#grpc-web-down   # stop it
nix run .#portal          # build + launch the Flutter app — increment 06
```

### The grpc-web proxy is web-only, and optional

Browsers can't speak raw gRPC (HTTP/2 trailers), so the **web** build needs a
grpc-web ↔ gRPC translator in front of the gateway. `grpc-web-up` runs **envoy as a
docker container** (the same docker-app pattern as `prometheus-up`/`clickstack-up`,
so the gate never source-builds envoy) that:

- listens on `:8090` (HTTP/1.1 + grpc-web) with a CORS policy for browser preflight,
- forwards to the `--serve-all` gateway on `:50100` over HTTP/2,
- runs `--network host` (Linux), so it's reachable on host loopback.

The **native desktop** build dials `:50100` directly and needs none of this — so the
proxy is an optional, web-only dependency, not part of the default run path.

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
