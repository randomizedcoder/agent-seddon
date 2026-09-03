# nix/portal/default.nix
#
# Agent Portal (docs/design/portal) tooling apps — all opt-in, none on the
# `nix flake check` path in a way that source-builds heavy toolchains:
#
#   nix run .#gen-dart        regenerate the committed Dart stubs (buf + protoc-gen-dart)
#   nix run .#portal          native desktop app (raw gRPC to :50100)
#   nix run .#portal-web      build the WEB bundle + serve it headless (no browser)
#   nix run .#grpc-web-up     envoy grpc-web proxy for the WEB build
#                             (:8090 -> gateway :50100, :8091 -> sessions :50080)
#   nix run .#grpc-web-down   stop it
#
# The native desktop build dials the gateway (:50100) directly and needs no proxy;
# `grpc-web-up` exists only because browsers cannot speak raw gRPC (HTTP/2 trailers).
# The proxy runs as a container (like prometheus/clickstack) so the gate never
# source-builds envoy; it honours `CONTAINER_RUNTIME=podman` for docker-less hosts.
#
# Every endpoint the app dials is overridable via `--dart-define` (see the
# `dartDefines` snippet): any `PORTAL_*` env var, or a `-- --dart-define=…` passthrough
# flag, is forwarded to Flutter; unset falls back to the defaults in
# portal/lib/src/config.dart. Defaults keep the tunnel recipe (localhost:8090/8091)
# working out of the box.
{
  pkgs,
  lib,
  versions,
}:
let
  # grpc-web proxy ports. UI plumbing, not seams, so they live here rather than in
  # nix/constants.nix's seam table (which the constants-sync check renders verbatim).
  grpcWebPort = 8090; # browser -> gateway
  grpcWebSessionsPort = 8091; # browser -> sessions
  portalWebPort = 8092; # static server for the built web bundle
  # The gateways the proxy forwards to (mirror nix/constants.nix gateway/sessions).
  gatewayPort = 50100;
  sessionsPort = 50080;
  name = "agent-grpc-web";
  # Fully-qualified so podman (whose unqualified-search list can be empty, e.g. on the
  # headless l2 box) resolves it; docker treats the docker.io/ prefix as a no-op.
  image = "docker.io/${versions.envoyImage}";

  # Forward configurable endpoints to Flutter as --dart-define flags. Any set PORTAL_*
  # env var is threaded through; `-- <extra flags>` still passes through via "$@".
  # Shared by the `portal` (native) and `portal-web` builds.
  dartDefines = ''
    defines=()
    for key in \
      PORTAL_GATEWAY_HOST PORTAL_GATEWAY_PORT \
      PORTAL_SESSIONS_HOST PORTAL_SESSIONS_PORT \
      PORTAL_GRPC_WEB_URL PORTAL_SESSIONS_GRPC_WEB_URL \
      PORTAL_GRAFANA_URL PORTAL_HYPERDX_URL PORTAL_PROMETHEUS_URL; do
      val="''${!key:-}"
      if [ -n "$val" ]; then defines+=("--dart-define=$key=$val"); fi
    done
  '';

  # Regenerate the committed Dart stubs from the protos. buf's first *generation*
  # (Rust codegen stays on tonic-build); output is committed under portal/lib/src/gen.
  gen-dart = pkgs.writeShellApplication {
    name = "gen-dart";
    runtimeInputs = [
      versions.buf
      versions.protoc-gen-dart
    ];
    text = ''
      out="portal/lib/src/gen"
      rm -rf "$out" && mkdir -p "$out"
      buf generate
      echo "wrote Dart stubs to $out"
    '';
  };

  # Build + launch the native Flutter app. Raw gRPC to :50100; endpoints overridable
  # via PORTAL_* env vars (see dartDefines). The platform runner scaffolding (linux/,
  # web/) is generated on demand — git-ignored boilerplate, so only lib/ + pubspec
  # are committed.
  portal = pkgs.writeShellApplication {
    name = "portal";
    runtimeInputs = [ versions.flutter ];
    text = ''
      cd portal
      # Idempotent: adds linux/ + web/ runners if missing, keeps lib/ + pubspec.
      flutter create --platforms=linux,web --project-name agent_portal . >/dev/null
      ${dartDefines}
      exec flutter run "''${defines[@]}" "$@"
    '';
  };

  # Build the Flutter WEB bundle and serve it headless — no browser needed on this
  # host (the viewer's browser is elsewhere, reached over an SSH tunnel). Front the
  # gRPC calls with `grpc-web-up`. Bind loopback by default (tunnel-only); override
  # with PORTAL_WEB_HOST / PORTAL_WEB_PORT.
  portal-web = pkgs.writeShellApplication {
    name = "portal-web";
    runtimeInputs = [
      versions.flutter
      versions.static-web-server
    ];
    text = ''
      cd portal
      flutter create --platforms=web --project-name agent_portal . >/dev/null
      ${dartDefines}
      echo "==> building Flutter web bundle (first run downloads the web SDK)…"
      flutter build web "''${defines[@]}" "$@"
      host="''${PORTAL_WEB_HOST:-127.0.0.1}"
      port="''${PORTAL_WEB_PORT:-${toString portalWebPort}}"
      echo "==> serving portal/build/web at http://$host:$port  (Ctrl-C to stop)"
      echo "    front the gRPC calls with: nix run .#grpc-web-up"
      exec static-web-server --root build/web --host "$host" --port "$port"
    '';
  };

  # Envoy grpc-web proxy: translates browser grpc-web to raw gRPC on the gateways.
  # Two listeners in one config/container: gateway (:8090 -> :50100) and the opt-in
  # sessions gateway (:8091 -> :50080). Web build only.
  envoyConfig = pkgs.writeText "portal-envoy.yaml" ''
    static_resources:
      listeners:
        - name: gateway_grpc_web
          address:
            socket_address: { address: 0.0.0.0, port_value: ${toString grpcWebPort} }
          filter_chains:
            - filters:
                - name: envoy.filters.network.http_connection_manager
                  typed_config:
                    "@type": type.googleapis.com/envoy.extensions.filters.network.http_connection_manager.v3.HttpConnectionManager
                    stat_prefix: gateway_grpc_web
                    codec_type: AUTO
                    route_config:
                      name: gateway_route
                      virtual_hosts:
                        - name: agent_gateway
                          domains: ["*"]
                          typed_per_filter_config:
                            envoy.filters.http.cors:
                              "@type": type.googleapis.com/envoy.extensions.filters.http.cors.v3.CorsPolicy
                              allow_origin_string_match:
                                - prefix: "*"
                              allow_methods: GET, PUT, DELETE, POST, OPTIONS
                              allow_headers: keep-alive,user-agent,cache-control,content-type,content-transfer-encoding,x-grpc-web,x-user-agent,grpc-timeout
                              max_age: "1728000"
                              expose_headers: grpc-status,grpc-message
                          routes:
                            - match: { prefix: "/" }
                              route: { cluster: agent_gateway, timeout: 0s }
                    http_filters:
                      - name: envoy.filters.http.grpc_web
                        typed_config:
                          "@type": type.googleapis.com/envoy.extensions.filters.http.grpc_web.v3.GrpcWeb
                      - name: envoy.filters.http.cors
                        typed_config:
                          "@type": type.googleapis.com/envoy.extensions.filters.http.cors.v3.Cors
                      - name: envoy.filters.http.router
                        typed_config:
                          "@type": type.googleapis.com/envoy.extensions.filters.http.router.v3.Router
        - name: sessions_grpc_web
          address:
            socket_address: { address: 0.0.0.0, port_value: ${toString grpcWebSessionsPort} }
          filter_chains:
            - filters:
                - name: envoy.filters.network.http_connection_manager
                  typed_config:
                    "@type": type.googleapis.com/envoy.extensions.filters.network.http_connection_manager.v3.HttpConnectionManager
                    stat_prefix: sessions_grpc_web
                    codec_type: AUTO
                    route_config:
                      name: sessions_route
                      virtual_hosts:
                        - name: agent_sessions
                          domains: ["*"]
                          typed_per_filter_config:
                            envoy.filters.http.cors:
                              "@type": type.googleapis.com/envoy.extensions.filters.http.cors.v3.CorsPolicy
                              allow_origin_string_match:
                                - prefix: "*"
                              allow_methods: GET, PUT, DELETE, POST, OPTIONS
                              allow_headers: keep-alive,user-agent,cache-control,content-type,content-transfer-encoding,x-grpc-web,x-user-agent,grpc-timeout
                              max_age: "1728000"
                              expose_headers: grpc-status,grpc-message
                          routes:
                            - match: { prefix: "/" }
                              route: { cluster: agent_sessions, timeout: 0s }
                    http_filters:
                      - name: envoy.filters.http.grpc_web
                        typed_config:
                          "@type": type.googleapis.com/envoy.extensions.filters.http.grpc_web.v3.GrpcWeb
                      - name: envoy.filters.http.cors
                        typed_config:
                          "@type": type.googleapis.com/envoy.extensions.filters.http.cors.v3.Cors
                      - name: envoy.filters.http.router
                        typed_config:
                          "@type": type.googleapis.com/envoy.extensions.filters.http.router.v3.Router
      clusters:
        - name: agent_gateway
          connect_timeout: 0.25s
          type: LOGICAL_DNS
          lb_policy: ROUND_ROBIN
          typed_extension_protocol_options:
            envoy.extensions.upstreams.http.v3.HttpProtocolOptions:
              "@type": type.googleapis.com/envoy.extensions.upstreams.http.v3.HttpProtocolOptions
              explicit_http_config:
                http2_protocol_options: {}
          load_assignment:
            cluster_name: agent_gateway
            endpoints:
              - lb_endpoints:
                  - endpoint:
                      address:
                        socket_address: { address: 127.0.0.1, port_value: ${toString gatewayPort} }
        - name: agent_sessions
          connect_timeout: 0.25s
          type: LOGICAL_DNS
          lb_policy: ROUND_ROBIN
          typed_extension_protocol_options:
            envoy.extensions.upstreams.http.v3.HttpProtocolOptions:
              "@type": type.googleapis.com/envoy.extensions.upstreams.http.v3.HttpProtocolOptions
              explicit_http_config:
                http2_protocol_options: {}
          load_assignment:
            cluster_name: agent_sessions
            endpoints:
              - lb_endpoints:
                  - endpoint:
                      address:
                        socket_address: { address: 127.0.0.1, port_value: ${toString sessionsPort} }
  '';

  # Both docker and podman on PATH; CONTAINER_RUNTIME (default docker) picks one.
  grpc-web-up = pkgs.writeShellApplication {
    name = "grpc-web-up";
    runtimeInputs = [
      versions.docker
      versions.podman
    ];
    text = ''
      runtime="''${CONTAINER_RUNTIME:-docker}"
      if ! "$runtime" info >/dev/null 2>&1; then
        echo "grpc-web-up: '$runtime' not reachable — is it installed/running?" >&2
        echo "  (on a podman-only host: CONTAINER_RUNTIME=podman nix run .#grpc-web-up)" >&2
        exit 1
      fi
      if "$runtime" ps -a --format '{{.Names}}' | grep -qx "${name}"; then
        echo "==> restarting ${name}"
        "$runtime" rm -f "${name}" >/dev/null
      fi
      echo "==> starting grpc-web proxy ($runtime, ${image}):"
      echo "      :${toString grpcWebPort}  -> gateway  :${toString gatewayPort}"
      echo "      :${toString grpcWebSessionsPort}  -> sessions :${toString sessionsPort}"
      # `--network host` (Linux) so envoy reaches the gateways on host loopback and
      # the browser (or an SSH tunnel) reaches envoy on the host proxy ports.
      "$runtime" run -d \
        --name "${name}" \
        --network host \
        -v "${envoyConfig}:/etc/envoy/envoy.yaml:ro" \
        "${image}" \
        -c /etc/envoy/envoy.yaml >/dev/null
      echo "grpc-web proxy up. Start the gateways with:"
      echo "  agent --serve-all       (:${toString gatewayPort})"
      echo "  agent --serve-sessions  (:${toString sessionsPort})"
      echo "Stop with: nix run .#grpc-web-down"
    '';
  };

  grpc-web-down = pkgs.writeShellApplication {
    name = "grpc-web-down";
    runtimeInputs = [
      versions.docker
      versions.podman
    ];
    text = ''
      runtime="''${CONTAINER_RUNTIME:-docker}"
      if "$runtime" ps -a --format '{{.Names}}' | grep -qx "${name}"; then
        "$runtime" rm -f "${name}" >/dev/null
        echo "stopped ${name}"
      else
        echo "${name} not running"
      fi
    '';
  };
in
{
  inherit
    gen-dart
    portal
    portal-web
    grpc-web-up
    grpc-web-down
    ;
}
