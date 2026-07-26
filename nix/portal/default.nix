# nix/portal/default.nix
#
# Agent Portal (docs/design/portal) tooling apps — all opt-in, none on the
# `nix flake check` path in a way that source-builds heavy toolchains:
#
#   nix run .#gen-dart       regenerate the committed Dart stubs (buf + protoc-gen-dart)
#   nix run .#grpc-web-up     envoy grpc-web proxy for the WEB build (:8090 -> gateway :50100)
#   nix run .#grpc-web-down   stop it
#
# The `nix run .#portal` runner (native Flutter app) lands with the app itself in
# increment 06. The native desktop build dials the gateway (:50100) directly and
# needs no proxy; `grpc-web-up` exists only because browsers cannot speak raw gRPC
# (HTTP/2 trailers). The proxy runs as a docker container (like prometheus/clickstack),
# so the gate never source-builds envoy.
{
  pkgs,
  lib,
  versions,
}:
let
  # grpc-web proxy port. UI plumbing, not a seam, so it lives here rather than in
  # nix/constants.nix's seam table (which the constants-sync check renders verbatim).
  grpcWebPort = 8090;
  # The `--serve-all` gateway the proxy forwards to (mirrors nix/constants.nix gateway).
  gatewayPort = 50100;
  name = "agent-grpc-web";
  image = versions.envoyImage;

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

  # Build + launch the Flutter app. Native desktop by default (raw gRPC to :50100);
  # `nix run .#portal -- -d chrome` for the web build (front it with grpc-web-up).
  # The platform runner scaffolding (linux/, web/) is generated on demand — it is
  # git-ignored boilerplate, so only lib/ + pubspec are committed.
  portal = pkgs.writeShellApplication {
    name = "portal";
    runtimeInputs = [ versions.flutter ];
    text = ''
      cd portal
      # Idempotent: adds linux/ + web/ runners if missing, keeps lib/ + pubspec.
      flutter create --platforms=linux,web --project-name agent_portal . >/dev/null
      exec flutter run "$@"
    '';
  };

  # Envoy grpc-web proxy: translates browser grpc-web on :8090 to raw gRPC on the
  # gateway :50100. Web build only.
  envoyConfig = pkgs.writeText "portal-envoy.yaml" ''
    static_resources:
      listeners:
        - name: grpc_web_listener
          address:
            socket_address: { address: 0.0.0.0, port_value: ${toString grpcWebPort} }
          filter_chains:
            - filters:
                - name: envoy.filters.network.http_connection_manager
                  typed_config:
                    "@type": type.googleapis.com/envoy.extensions.filters.network.http_connection_manager.v3.HttpConnectionManager
                    stat_prefix: ingress_http
                    codec_type: AUTO
                    route_config:
                      name: local_route
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
                              route:
                                cluster: agent_gateway
                                timeout: 0s
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
  '';

  grpc-web-up = pkgs.writeShellApplication {
    name = "grpc-web-up";
    runtimeInputs = [ versions.docker ];
    text = ''
      if ! docker info >/dev/null 2>&1; then
        echo "grpc-web-up: docker daemon not reachable — is it running?" >&2
        exit 1
      fi
      if docker ps -a --format '{{.Names}}' | grep -qx "${name}"; then
        echo "==> restarting ${name}"
        docker rm -f "${name}" >/dev/null
      fi
      echo "==> starting grpc-web proxy (${image}): :${toString grpcWebPort} -> gateway :${toString gatewayPort}"
      # `--network host` (Linux) so envoy reaches the gateway on host loopback and
      # the browser reaches envoy on host :${toString grpcWebPort}.
      docker run -d \
        --name "${name}" \
        --network host \
        -v "${envoyConfig}:/etc/envoy/envoy.yaml:ro" \
        "${image}" \
        -c /etc/envoy/envoy.yaml >/dev/null
      echo "grpc-web proxy up. Start the gateway with: agent --serve-all"
      echo "Stop with: nix run .#grpc-web-down"
    '';
  };

  grpc-web-down = pkgs.writeShellApplication {
    name = "grpc-web-down";
    runtimeInputs = [ versions.docker ];
    text = ''
      if docker ps -a --format '{{.Names}}' | grep -qx "${name}"; then
        docker rm -f "${name}" >/dev/null
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
    grpc-web-up
    grpc-web-down
    ;
}
