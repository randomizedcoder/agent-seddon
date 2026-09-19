# nix/portal/default.nix
#
# Agent Portal (docs/design/portal) tooling apps — all opt-in, none on the
# `nix flake check` path in a way that source-builds heavy toolchains:
#
#   nix run .#gen-dart        regenerate the committed Dart stubs (buf + protoc-gen-dart)
#   nix run .#portal          native desktop app (raw gRPC to :50100)
#   nix run .#portal-web      build the WEB bundle + serve it headless (no browser)
#   nix run .#grpc-web-up     envoy grpc-web proxy for the WEB build
#                             (:8090 -> gateway :50100, :8091 -> sessions :50080,
#                              :8093 -> fleet :50086)
#   nix run .#grpc-web-down   stop it
#   nix run .#portal-redeploy one-verb server-tier bring-up: (re)start the --serve-all
#                             gateway (:50100), tear down ANY stale bridge squatting
#                             :8090, recreate the canonical grpc-web bridge, then
#                             health-check the FULL browser path (grpc-web round-trip
#                             through the bridge). Never touches a separate --serve-fleet.
#
# The native desktop build dials the gateway (:50100) directly and needs no proxy;
# `grpc-web-up` exists only because browsers cannot speak raw gRPC (HTTP/2 trailers).
# The proxy runs as a container (like prometheus/hyperdx) so the gate never
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
  agent,
  harness,
  portal-test-report,
}:
let
  # grpc-web proxy ports. UI plumbing, not seams, so they live here rather than in
  # nix/constants.nix's seam table (which the constants-sync check renders verbatim).
  grpcWebPort = 8090; # browser -> gateway
  grpcWebSessionsPort = 8091; # browser -> sessions
  portalWebPort = 8092; # static server for the built web bundle
  grpcWebFleetPort = 8093; # browser -> fleet (--serve-fleet)
  # The gateways the proxy forwards to (mirror nix/constants.nix gateway/sessions).
  gatewayPort = 50100;
  sessionsPort = 50080;
  fleetPort = 50086; # the full review-fleet process (--serve-fleet)
  # Prometheus metrics of the --serve-all gateway (mirrors nix/constants.nix
  # GATEWAY.metrics_port). The Layer-B e2e reads per-RPC deltas from here.
  metricsPort = 9700;
  # OTLP/gRPC collector (the decomposed HyperDX otel-collector), reached on host
  # loopback since envoy runs --network host. Access logs + tracer spans from the
  # bridge ship here so the browser -> envoy -> gateway -> seam hop is one trace.
  otelCollectorPort = versions.otlpGrpcPort;
  # The SINGLE agent ClickHouse (HTTP :8123). It now holds BOTH the Layer-B perf rows
  # (`agent.portal_gui_perf`, inc 09) AND the OTLP spans (`default.otel_traces`, written
  # by the HyperDX collector) — the obs stack was decomposed off the all-in-one so there
  # is one ClickHouse, and `trace_id` is a same-server (cross-DB) JOIN, not a key link.
  chHttpPort = versions.clickhouseHttpPort;
  chContainer = versions.clickhouseContainerName;
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
      PORTAL_FLEET_HOST PORTAL_FLEET_PORT \
      PORTAL_GRPC_WEB_URL PORTAL_SESSIONS_GRPC_WEB_URL PORTAL_FLEET_GRPC_WEB_URL \
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
      pkgs.coreutils
      pkgs.gnused
    ];
    text = ''
      cd portal
      flutter create --platforms=web --project-name agent_portal . >/dev/null
      ${dartDefines}
      echo "==> building Flutter web bundle (first run downloads the web SDK)…"
      # --pwa-strategy=none: do NOT generate/register a service worker. This portal
      # is an internal tool that gets rewired constantly (endpoints, gateway); a
      # cached SW served a stale pre-`personality-selector` bundle for days and
      # surfaced as a bogus "Not connected to the gateway" 404 — pure client cache,
      # not a server fault. The SW buys nothing here and costs exactly that. A caller
      # can still override by re-passing --pwa-strategy after "$@".
      flutter build web --pwa-strategy=none "''${defines[@]}" "$@"
      # Belt-and-suspenders: drop any service-worker file a previous strategy left in
      # the output tree so the static server never hands a client a registerable SW.
      rm -f build/web/flutter_service_worker.js
      # Brand the browser tab: `flutter create` regenerates web/ with the stock Flutter
      # favicon/PWA icons + a placeholder <title>, so we overwrite them in the built
      # output tree (post-build) with the agent-seddon mark. Source assets are tracked
      # under portal/branding/ (generated from agent-seddon.png; see that dir).
      if [ -d branding ]; then
        install -m0644 branding/favicon.png build/web/favicon.png
        mkdir -p build/web/icons
        install -m0644 branding/Icon-192.png          build/web/icons/Icon-192.png
        install -m0644 branding/Icon-512.png          build/web/icons/Icon-512.png
        install -m0644 branding/Icon-maskable-192.png build/web/icons/Icon-maskable-192.png
        install -m0644 branding/Icon-maskable-512.png build/web/icons/Icon-maskable-512.png
        sed -i 's#<title>agent_portal</title>#<title>Agent Seddon</title>#' build/web/index.html
      fi
      host="''${PORTAL_WEB_HOST:-127.0.0.1}"
      port="''${PORTAL_WEB_PORT:-${toString portalWebPort}}"
      echo "==> serving portal/build/web at http://$host:$port  (Ctrl-C to stop)"
      echo "    front the gRPC calls with: nix run .#grpc-web-up"
      # --cache-control-headers=false: static-web-server defaults to stamping
      # `cache-control: max-age=31536000` (one year) on EVERY file. Flutter's
      # `--pwa-strategy=none` output is served under fixed, un-hashed names
      # (`index.html`, `flutter_bootstrap.js`, `main.dart.js`), so that default made a
      # returning browser hold the previous bundle for a year — a `portal-redeploy` was
      # invisible until a manual hard-reload. That is the same staleness the disabled
      # service worker caused (see the --pwa-strategy note above), reintroduced through
      # the HTTP cache. Disabling the header leaves `last-modified` in place, so the
      # browser revalidates (If-Modified-Since → 304 or fresh bytes) and always picks up
      # a redeploy. The revalidation cost is negligible for a loopback/LAN internal tool.
      exec static-web-server --root build/web --host "$host" --port "$port" \
        --cache-control-headers=false
    '';
  };

  # Envoy grpc-web proxy: translates browser grpc-web to raw gRPC on the gateways.
  # Three listeners in one config/container: gateway (:8090 -> :50100), the opt-in
  # sessions gateway (:8091 -> :50080), and the opt-in fleet process
  # (:8093 -> :50086, the Fleet tab). Web build only.
  #
  # Each listener ships OTLP access logs + tracer spans to the ClickStack collector
  # (otel_collector cluster -> 127.0.0.1:${toString otelCollectorPort}) so the
  # browser -> envoy -> gateway -> seam hop is one trace (docs/design/portal-gui-testing/
  # 07-envoy-otel.md). ClickStack's all-in-one-auth image requires an ingestion key on
  # OTLP, so the `''${OTLP_AUTHORIZATION}` sentinel below is substituted at bring-up by
  # grpc-web-up from $PORTAL_OTLP_AUTHORIZATION|$CLICKSTACK_INGESTION_API_KEY — the secret
  # is NEVER baked into the nix store (mirrors the agent's [telemetry] otlp_headers). Unset
  # ⇒ empty header ⇒ the collector drops the bridge's telemetry (best-effort, non-fatal).
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
                    access_log:
                      - name: envoy.access_loggers.open_telemetry
                        typed_config:
                          "@type": type.googleapis.com/envoy.extensions.access_loggers.open_telemetry.v3.OpenTelemetryAccessLogConfig
                          common_config:
                            log_name: envoy-portal-bridge
                            transport_api_version: V3
                            grpc_service:
                              envoy_grpc:
                                cluster_name: otel_collector
                              initial_metadata:
                                - key: authorization
                                  value: "''${OTLP_AUTHORIZATION}"
                          resource_attributes:
                            values:
                              - key: service.name
                                value: { string_value: envoy-portal-bridge }
                          body: { string_value: "%REQ(:PATH)%" }
                          attributes:
                            values:
                              - key: duration_ms
                                value: { string_value: "%DURATION%" }
                              - key: response_duration_ms
                                value: { string_value: "%RESPONSE_DURATION%" }
                              - key: request_duration_ms
                                value: { string_value: "%REQUEST_DURATION%" }
                              - key: grpc_status
                                value: { string_value: "%GRPC_STATUS%" }
                              - key: response_code
                                value: { string_value: "%RESPONSE_CODE%" }
                              - key: response_flags
                                value: { string_value: "%RESPONSE_FLAGS%" }
                              - key: upstream_host
                                value: { string_value: "%UPSTREAM_HOST%" }
                              - key: request_id
                                value: { string_value: "%REQ(X-REQUEST-ID)%" }
                    tracing:
                      random_sampling: { value: 100 }
                      provider:
                        name: envoy.tracers.opentelemetry
                        typed_config:
                          "@type": type.googleapis.com/envoy.config.trace.v3.OpenTelemetryConfig
                          service_name: envoy-portal-bridge
                          grpc_service:
                            envoy_grpc:
                              cluster_name: otel_collector
                            initial_metadata:
                              - key: authorization
                                value: "''${OTLP_AUTHORIZATION}"
                            timeout: 0.250s
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
                              allow_headers: keep-alive,user-agent,cache-control,content-type,content-transfer-encoding,x-grpc-web,x-user-agent,grpc-timeout,x-agent-user-id,x-agent-session-id,traceparent,tracestate,x-request-id
                              max_age: "1728000"
                              expose_headers: grpc-status,grpc-message,x-envoy-upstream-service-time,traceparent,tracestate
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
                    access_log:
                      - name: envoy.access_loggers.open_telemetry
                        typed_config:
                          "@type": type.googleapis.com/envoy.extensions.access_loggers.open_telemetry.v3.OpenTelemetryAccessLogConfig
                          common_config:
                            log_name: envoy-portal-bridge
                            transport_api_version: V3
                            grpc_service:
                              envoy_grpc:
                                cluster_name: otel_collector
                              initial_metadata:
                                - key: authorization
                                  value: "''${OTLP_AUTHORIZATION}"
                          resource_attributes:
                            values:
                              - key: service.name
                                value: { string_value: envoy-portal-bridge }
                          body: { string_value: "%REQ(:PATH)%" }
                          attributes:
                            values:
                              - key: duration_ms
                                value: { string_value: "%DURATION%" }
                              - key: response_duration_ms
                                value: { string_value: "%RESPONSE_DURATION%" }
                              - key: request_duration_ms
                                value: { string_value: "%REQUEST_DURATION%" }
                              - key: grpc_status
                                value: { string_value: "%GRPC_STATUS%" }
                              - key: response_code
                                value: { string_value: "%RESPONSE_CODE%" }
                              - key: response_flags
                                value: { string_value: "%RESPONSE_FLAGS%" }
                              - key: upstream_host
                                value: { string_value: "%UPSTREAM_HOST%" }
                              - key: request_id
                                value: { string_value: "%REQ(X-REQUEST-ID)%" }
                    tracing:
                      random_sampling: { value: 100 }
                      provider:
                        name: envoy.tracers.opentelemetry
                        typed_config:
                          "@type": type.googleapis.com/envoy.config.trace.v3.OpenTelemetryConfig
                          service_name: envoy-portal-bridge
                          grpc_service:
                            envoy_grpc:
                              cluster_name: otel_collector
                            initial_metadata:
                              - key: authorization
                                value: "''${OTLP_AUTHORIZATION}"
                            timeout: 0.250s
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
                              allow_headers: keep-alive,user-agent,cache-control,content-type,content-transfer-encoding,x-grpc-web,x-user-agent,grpc-timeout,x-agent-user-id,x-agent-session-id,traceparent,tracestate,x-request-id
                              max_age: "1728000"
                              expose_headers: grpc-status,grpc-message,x-envoy-upstream-service-time,traceparent,tracestate
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
        - name: fleet_grpc_web
          address:
            socket_address: { address: 0.0.0.0, port_value: ${toString grpcWebFleetPort} }
          filter_chains:
            - filters:
                - name: envoy.filters.network.http_connection_manager
                  typed_config:
                    "@type": type.googleapis.com/envoy.extensions.filters.network.http_connection_manager.v3.HttpConnectionManager
                    stat_prefix: fleet_grpc_web
                    codec_type: AUTO
                    access_log:
                      - name: envoy.access_loggers.open_telemetry
                        typed_config:
                          "@type": type.googleapis.com/envoy.extensions.access_loggers.open_telemetry.v3.OpenTelemetryAccessLogConfig
                          common_config:
                            log_name: envoy-portal-bridge
                            transport_api_version: V3
                            grpc_service:
                              envoy_grpc:
                                cluster_name: otel_collector
                              initial_metadata:
                                - key: authorization
                                  value: "''${OTLP_AUTHORIZATION}"
                          resource_attributes:
                            values:
                              - key: service.name
                                value: { string_value: envoy-portal-bridge }
                          body: { string_value: "%REQ(:PATH)%" }
                          attributes:
                            values:
                              - key: duration_ms
                                value: { string_value: "%DURATION%" }
                              - key: response_duration_ms
                                value: { string_value: "%RESPONSE_DURATION%" }
                              - key: request_duration_ms
                                value: { string_value: "%REQUEST_DURATION%" }
                              - key: grpc_status
                                value: { string_value: "%GRPC_STATUS%" }
                              - key: response_code
                                value: { string_value: "%RESPONSE_CODE%" }
                              - key: response_flags
                                value: { string_value: "%RESPONSE_FLAGS%" }
                              - key: upstream_host
                                value: { string_value: "%UPSTREAM_HOST%" }
                              - key: request_id
                                value: { string_value: "%REQ(X-REQUEST-ID)%" }
                    tracing:
                      random_sampling: { value: 100 }
                      provider:
                        name: envoy.tracers.opentelemetry
                        typed_config:
                          "@type": type.googleapis.com/envoy.config.trace.v3.OpenTelemetryConfig
                          service_name: envoy-portal-bridge
                          grpc_service:
                            envoy_grpc:
                              cluster_name: otel_collector
                            initial_metadata:
                              - key: authorization
                                value: "''${OTLP_AUTHORIZATION}"
                            timeout: 0.250s
                    route_config:
                      name: fleet_route
                      virtual_hosts:
                        - name: agent_fleet
                          domains: ["*"]
                          typed_per_filter_config:
                            envoy.filters.http.cors:
                              "@type": type.googleapis.com/envoy.extensions.filters.http.cors.v3.CorsPolicy
                              allow_origin_string_match:
                                - prefix: "*"
                              allow_methods: GET, PUT, DELETE, POST, OPTIONS
                              allow_headers: keep-alive,user-agent,cache-control,content-type,content-transfer-encoding,x-grpc-web,x-user-agent,grpc-timeout,x-agent-user-id,x-agent-session-id,traceparent,tracestate,x-request-id
                              max_age: "1728000"
                              expose_headers: grpc-status,grpc-message,x-envoy-upstream-service-time,traceparent,tracestate
                          routes:
                            - match: { prefix: "/" }
                              route: { cluster: agent_fleet, timeout: 0s }
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
        - name: agent_fleet
          connect_timeout: 0.25s
          type: LOGICAL_DNS
          lb_policy: ROUND_ROBIN
          typed_extension_protocol_options:
            envoy.extensions.upstreams.http.v3.HttpProtocolOptions:
              "@type": type.googleapis.com/envoy.extensions.upstreams.http.v3.HttpProtocolOptions
              explicit_http_config:
                http2_protocol_options: {}
          load_assignment:
            cluster_name: agent_fleet
            endpoints:
              - lb_endpoints:
                  - endpoint:
                      address:
                        socket_address: { address: 127.0.0.1, port_value: ${toString fleetPort} }
        - name: otel_collector
          connect_timeout: 0.25s
          type: LOGICAL_DNS
          lb_policy: ROUND_ROBIN
          typed_extension_protocol_options:
            envoy.extensions.upstreams.http.v3.HttpProtocolOptions:
              "@type": type.googleapis.com/envoy.extensions.upstreams.http.v3.HttpProtocolOptions
              explicit_http_config:
                http2_protocol_options: {}
          load_assignment:
            cluster_name: otel_collector
            endpoints:
              - lb_endpoints:
                  - endpoint:
                      address:
                        socket_address: { address: 127.0.0.1, port_value: ${toString otelCollectorPort} }
  '';

  # Both docker and podman on PATH; CONTAINER_RUNTIME (default docker) picks one.
  grpc-web-up = pkgs.writeShellApplication {
    name = "grpc-web-up";
    runtimeInputs = [
      versions.docker
      versions.podman
      pkgs.gettext # envsubst — inject the OTLP auth token into the config at bring-up
      pkgs.coreutils
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
            # Render the effective config: substitute ONLY $OTLP_AUTHORIZATION (the ClickStack
            # ingestion key) into the committed template, so the secret never lands in the nix
            # store. Written to a stable per-user path (not a mktemp we'd trap-clean) so it
            # outlives this script for the detached container's lifetime. Empty ⇒ empty header.
            OTLP_AUTHORIZATION="''${PORTAL_OTLP_AUTHORIZATION:-''${CLICKSTACK_INGESTION_API_KEY:-}}"
            export OTLP_AUTHORIZATION
            render_dir="''${XDG_RUNTIME_DIR:-/tmp}"
            effective_config="$render_dir/${name}-envoy.yaml"
      # shellcheck disable=SC2016 # envsubst takes the literal var NAME (not its value) to scope substitution
            envsubst '$OTLP_AUTHORIZATION' <"${envoyConfig}" >"$effective_config"
            if [ -z "$OTLP_AUTHORIZATION" ]; then
              echo "grpc-web-up: note — no OTLP ingestion key set (PORTAL_OTLP_AUTHORIZATION /" >&2
              echo "  CLICKSTACK_INGESTION_API_KEY); the bridge's OTLP telemetry will be dropped" >&2
              echo "  by an auth'd collector. Set it to trace browser -> envoy -> gateway." >&2
            fi
            echo "==> starting grpc-web proxy ($runtime, ${image}):"
            echo "      :${toString grpcWebPort}  -> gateway  :${toString gatewayPort}"
            echo "      :${toString grpcWebSessionsPort}  -> sessions :${toString sessionsPort}"
            echo "      :${toString grpcWebFleetPort}  -> fleet    :${toString fleetPort}"
            # `--network host` (Linux) so envoy reaches the gateways on host loopback and
            # the browser (or an SSH tunnel) reaches envoy on the host proxy ports.
            "$runtime" run -d \
              --name "${name}" \
              --network host \
              -v "$effective_config:/etc/envoy/envoy.yaml:ro" \
              "${image}" \
              -c /etc/envoy/envoy.yaml >/dev/null
            echo "grpc-web proxy up. Start the gateways with:"
            echo "  agent --serve-all       (:${toString gatewayPort})"
            echo "  agent --serve-sessions  (:${toString sessionsPort})"
            echo "  agent --serve-fleet     (:${toString fleetPort})"
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

  # `nix run .#portal-redeploy` — the server tier of the portal in one verb, mirroring
  # `fleet-redeploy` (build → stop → serve → health-check). It exists because a manual
  # bring-up once left a *differently-named* grpc-web bridge squatting :8090 while
  # pointed at the wrong backend, so every main-tab RPC 404'd against the browser: the
  # symptom looked like a portal bug but was pure wiring. This app makes that
  # unreproducible — it sweeps ANY `agent-grpc-web*` container before recreating the
  # canonical one, and its health-check is the *actual browser path* (a grpc-web
  # round-trip through the bridge), not just "is the gateway up".
  #
  # It manages ONLY the --serve-all gateway (tracked via its own pidfile); a separately
  # run --serve-fleet / --serve-sessions is never touched. The browser tier is the
  # long-running `portal-web` (printed as the next step), kept separate so a redeploy
  # stays fast and never source-builds the Flutter web SDK on the gate path.
  #
  #   $1 | $PORTAL_CONFIG       gateway agent TOML (default config/agent.toml; run from repo root)
  #   $CONTAINER_RUNTIME        docker|podman (default docker; l2 is podman-only)
  #   $PORTAL_GATEWAY_PIDFILE   pid we track   (default $XDG_RUNTIME_DIR|/tmp / agent-serve-all.pid)
  #   $PORTAL_GATEWAY_LOG       serve log path (default $XDG_RUNTIME_DIR|/tmp / agent-serve-all.log)
  #   $PORTAL_HEALTH_RETRIES    probe attempts, 2s apart (default 30 ⇒ up to 60s)
  portal-redeploy = pkgs.writeShellApplication {
    name = "portal-redeploy";
    runtimeInputs = [
      pkgs.coreutils
      pkgs.curl
      versions.docker
      versions.podman
      grpc-web-up
    ];
    text = ''
      # writeShellApplication already sets `set -euo pipefail`.
      agent_bin="${agent}/bin/agent"
      runtime="''${CONTAINER_RUNTIME:-docker}"
      export CONTAINER_RUNTIME="$runtime"

      config="''${1:-''${PORTAL_CONFIG:-config/agent.toml}}"
      if [ ! -f "$config" ]; then
        echo "portal-redeploy: gateway config not found: $config" >&2
        echo "  pass the agent TOML as \$1 or set PORTAL_CONFIG, and run from the repo root." >&2
        exit 2
      fi

      if ! "$runtime" info >/dev/null 2>&1; then
        echo "portal-redeploy: container runtime '$runtime' not reachable — is it running?" >&2
        echo "  (on a podman-only host: CONTAINER_RUNTIME=podman nix run .#portal-redeploy)" >&2
        exit 1
      fi

      runtime_dir="''${XDG_RUNTIME_DIR:-/tmp}"
      pidfile="''${PORTAL_GATEWAY_PIDFILE:-$runtime_dir/agent-serve-all.pid}"
      log="''${PORTAL_GATEWAY_LOG:-$runtime_dir/agent-serve-all.log}"
      retries="''${PORTAL_HEALTH_RETRIES:-30}"

      echo "==> portal-redeploy: agent=$agent_bin config=$config runtime=$runtime"

      # 1. Stop the previous gateway if we are tracking a live one. This is scoped to the
      #    --serve-all gateway via our own pidfile and never signals a --serve-fleet.
      if [ -f "$pidfile" ] && oldpid="$(cat "$pidfile" 2>/dev/null)" && [ -n "$oldpid" ] \
        && kill -0 "$oldpid" 2>/dev/null; then
        echo "==> [1/4] stopping previous gateway (pid $oldpid)"
        kill "$oldpid" 2>/dev/null || true
        for _ in $(seq 1 10); do
          kill -0 "$oldpid" 2>/dev/null || break
          sleep 1
        done
        kill -9 "$oldpid" 2>/dev/null || true
      else
        echo "==> [1/4] no live previous gateway to stop"
      fi

      # 2. Start the freshly-built gateway (--serve-all, :${toString gatewayPort}).
      echo "==> [2/4] starting gateway (--serve-all), log $log"
      nohup "$agent_bin" --serve-all --config "$config" > "$log" 2>&1 &
      newpid=$!
      echo "$newpid" > "$pidfile"
      echo "    pid $newpid"

      # 3. Sweep ANY stale bridge (a differently-named proxy on :${toString grpcWebPort}
      #    pointed at the wrong backend is the exact failure this app prevents), then let
      #    the idempotent grpc-web-up recreate the canonical bridge.
      echo "==> [3/4] (re)creating grpc-web bridge (sweeping any stale agent-grpc-web* first)"
      names="$("$runtime" ps -a --format '{{.Names}}' | grep -E '^agent-grpc-web' || true)"
      if [ -n "$names" ]; then
        while IFS= read -r c; do
          if [ "$c" != "${name}" ]; then
            echo "    removing stale bridge container: $c"
            "$runtime" rm -f "$c" >/dev/null 2>&1 || true
          fi
        done <<< "$names"
      fi
      grpc-web-up

      # 4. Health-check the FULL browser path: a grpc-web round-trip THROUGH the bridge
      #    (verifies bridge routing + CORS, not merely that the gateway is up). The empty
      #    frame 'AAAAAAA=' is a zero-length request message; a healthy path returns
      #    HTTP 200 with a grpc-status:0 trailer.
      echo "==> [4/4] waiting for the gateway to answer through the bridge :${toString grpcWebPort}"
      probe_url="http://127.0.0.1:${toString grpcWebPort}/agent.v1.PromptService/GetActivePersonality"
      for _ in $(seq 1 "$retries"); do
        if ! kill -0 "$newpid" 2>/dev/null; then
          echo "portal-redeploy: gateway exited early — last log lines:" >&2
          tail -n 20 "$log" >&2 || true
          exit 1
        fi
        code="$(curl -sS -o /dev/null -w '%{http_code}' -X POST \
          -H 'Content-Type: application/grpc-web-text' \
          -H 'Accept: application/grpc-web-text' \
          -H 'x-grpc-web: 1' \
          --data 'AAAAAAA=' "$probe_url" 2>/dev/null || true)"
        if [ "$code" = "200" ]; then
          echo "==> portal-redeploy OK — gateway :${toString gatewayPort} live and reachable"
          echo "    through the bridge :${toString grpcWebPort} (pid $newpid)"
          echo "    serve the browser tier with:"
          echo "      PORTAL_WEB_HOST=0.0.0.0 \\"
          echo "      PORTAL_GRPC_WEB_URL=http://<l2-ip>:${toString grpcWebPort} nix run .#portal-web"
          echo "    NOTE: a browser that already loaded the portal caches it via a service"
          echo "          worker — hard-reload (Cmd/Ctrl+Shift+R) or unregister the SW to"
          echo "          pick up a re-wired bridge."
          exit 0
        fi
        sleep 2
      done

      echo "portal-redeploy: bridge did not return 200 for the grpc-web probe after $((retries * 2))s" >&2
      echo "  last gateway log lines:" >&2
      tail -n 20 "$log" >&2 || true
      exit 1
    '';
  };

  # `nix run .#portal-e2e` — the opt-in **Layer B** live end-to-end
  # (docs/design/portal-gui-testing/04). It brings up the real server tier + the
  # Envoy grpc-web bridge, drives the real Flutter *web* app headlessly through
  # the bridge (`browser -> envoy -> gateway -> seam`), and proves each curated
  # *mutating* action from the **observability system**: the `:${toString metricsPort}`
  # metrics delta (correct RPC fired + `ok`), the read RPC on `:${toString gatewayPort}`
  # (state changed), and a curated OTLP `grpc.server` span in ClickHouse (the
  # tracing pipe). It emits a rich per-case JSONL report and renders it.
  #
  # NOT a check: it spawns servers + a browser + dials sockets, which agent-seddon
  # keeps out of the hermetic `nix flake check` sandbox (like `serve-smoke` /
  # `e2e-live`). Exit codes are the shared 0/1/2 contract (0 ok, 1 harness, 2
  # contract).
  #
  # Browser: `flutter drive` launches the web build in headless chromium via
  # chromedriver. The nixpkgs `chromium` the flake input carries has no cached
  # binary (it would source-build), so — like `portal-web`/`grpc-web-up`, which
  # already fetch the Flutter web SDK / the envoy image at runtime — the browser +
  # matched driver are resolved at run time from the ambient `nixpkgs` registry
  # (fully binary-cached) and can be overridden with `PORTAL_E2E_CHROMIUM` /
  # `PORTAL_E2E_CHROMEDRIVER`.
  #
  # Fleet safety: a `--serve-fleet` (or any seam) ALREADY running counts as *up* —
  # this app uses it and never restarts or kills it. It tracks only the seams IT
  # starts (pidfiles under its own workdir) and tears down only those.
  portal-e2e = pkgs.writeShellApplication {
    name = "portal-e2e";
    runtimeInputs = [
      agent
      versions.flutter
      versions.grpcurl
      versions.jq
      versions.docker
      versions.podman
      pkgs.nix # resolve the cached chromium + chromedriver at runtime
      pkgs.curl
      pkgs.coreutils
      pkgs.gnugrep
      pkgs.gawk
      pkgs.git # perf rows (inc 09) stamp commit_sha / branch / git_dirty
      grpc-web-up
    ];
    text = ''
      set -uo pipefail
    ''
    + harness.contract
    + ''

      agent_bin="${agent}/bin/agent"
      runtime="''${CONTAINER_RUNTIME:-docker}"
      export CONTAINER_RUNTIME="$runtime"
      run_id="''${PORTAL_E2E_RUN_ID:-$$}"

      config="''${PORTAL_CONFIG:-config/agent.toml}"
      if [ ! -f "$config" ]; then
        echo "portal-e2e: gateway config not found: $config (run from the repo root, or set PORTAL_CONFIG)" >&2
        exit 1
      fi
      if ! "$runtime" info >/dev/null 2>&1; then
        echo "portal-e2e: container runtime '$runtime' not reachable — is it running?" >&2
        echo "  (on a podman-only host: CONTAINER_RUNTIME=podman nix run .#portal-e2e)" >&2
        exit 1
      fi

      workdir="$(mktemp -d)"
      cd_pid=""
      # Teardown: kill ONLY the seams we started (a pidfile each in $workdir) plus
      # our chromedriver. A seam we found already running has no pidfile here, so
      # it is never signalled (the operator's --serve-fleet is safe).
      # shellcheck disable=SC2329  # invoked indirectly via the EXIT trap
      teardown() {
        [ -n "$cd_pid" ] && kill "$cd_pid" 2>/dev/null || true
        for pf in "$workdir"/*.pid; do
          [ -f "$pf" ] || continue
          p="$(cat "$pf" 2>/dev/null || true)"
          [ -n "$p" ] && kill "$p" 2>/dev/null || true
        done
        rm -rf "$workdir"
      }
      trap teardown EXIT

      # --- helpers ---------------------------------------------------------------
      # A gRPC health probe. `agent`'s seams answer grpc.health.v1 when served.
      health() { grpcurl -plaintext "$1" grpc.health.v1.Health/Check >/dev/null 2>&1; }

      wait_health() {
        local addr="$1" n="''${2:-40}"
        for _ in $(seq 1 "$n"); do health "$addr" && return 0; sleep 1; done
        return 1
      }

      # ensure_seam REQUIRED LABEL FLAG ADDR — if ADDR is already healthy, use it
      # (never restart). Otherwise start `agent FLAG` tracked by our own pidfile.
      # REQUIRED=1 seams that fail are a harness error; REQUIRED=0 (optional) seams
      # that fail just leave their page to preflight as skipped (a WARN, not a fail).
      ensure_seam() {
        local required="$1" label="$2" flag="$3" addr="$4"
        if health "$addr"; then
          echo "portal-e2e: $label already up ($addr) — using it, not restarting"
          return 0
        fi
        echo "portal-e2e: starting agent $flag ($addr)"
        nohup "$agent_bin" "$flag" --config "$config" >"$workdir/$label.log" 2>&1 &
        echo "$!" >"$workdir/$label.pid"
        if ! wait_health "$addr"; then
          if [ "$required" -eq 1 ]; then
            echo "FAIL(harness): required seam $label ($flag) never became healthy" >&2
          else
            echo "portal-e2e: [warn] optional seam $label ($flag) not available — its page will preflight as skipped"
          fi
          tail -n 5 "$workdir/$label.log" >&2 || true
          return 1
        fi
      }

      # metric_val FILE RPC — the ok-outcome counter for
      # agent_grpc_server_rpc_total{outcome="ok",rpc="RPC",...}. The scrape is
      # SERVER-produced and thus untrusted (CLAUDE.md): match the exact family +
      # labels, take the trailing field, and accept ONLY a run of digits — an
      # empty/negative/non-numeric/NaN value collapses to 0 (fail closed), so a
      # hostile value can never make a delta look positive.
      metric_val() {
        local f="$1" rpc="$2" v
        v="$(grep -F 'agent_grpc_server_rpc_total{' "$f" 2>/dev/null \
              | grep -F 'outcome="ok"' \
              | grep -F "rpc=\"$rpc\"" \
              | awk '{print $NF}' | tail -n1)"
        case "$v" in
          "" | *[!0-9]* ) echo 0 ;;
          * ) echo "$v" ;;
        esac
      }

      # append_record — one rich per-case JSONL row for the inc-08 renderer
      # (page -> element -> case). outcome is pass|fail|skip.
      append_record() {
        # page element rpc outcome ms detail
        jq -cn --arg page "$1" --arg el "$2" --arg rpc "$3" --arg oc "$4" \
              --argjson ms "$5" --arg detail "$6" \
          '{page:$page, case:("e2e_"+($rpc|split("/")|last)), layer:"e2e",
            element_id:$el, outcome:$oc, duration_ms:$ms, backend:"up",
            rpc_fired:[$rpc], description:$detail}' >>"$workdir/report.jsonl"
      }

      # --- 1. resolve the headless browser + driver -----------------------------
      chromium_bin="''${PORTAL_E2E_CHROMIUM:-}"
      chromedriver_bin="''${PORTAL_E2E_CHROMEDRIVER:-}"
      if [ -z "$chromium_bin" ]; then
        echo "portal-e2e: resolving chromium from the nixpkgs registry (cached)…"
        chromium_bin="$(nix build --no-link --print-out-paths nixpkgs#chromium)/bin/chromium"
      fi
      if [ -z "$chromedriver_bin" ]; then
        echo "portal-e2e: resolving chromedriver from the nixpkgs registry (cached)…"
        chromedriver_bin="$(nix build --no-link --print-out-paths nixpkgs#chromedriver)/bin/chromedriver"
      fi
      if [ ! -x "$chromium_bin" ] || [ ! -x "$chromedriver_bin" ]; then
        echo "FAIL(harness): could not resolve chromium/chromedriver binaries" >&2
        exit 1
      fi
      # chromedriver auto-detects the browser by finding `chromium` on PATH; also
      # export CHROME_EXECUTABLE for flutter's own probing.
      chromium_dir="$(dirname "$chromium_bin")"
      chromedriver_dir="$(dirname "$chromedriver_bin")"
      export PATH="$chromium_dir:$chromedriver_dir:$PATH"
      export CHROME_EXECUTABLE="$chromium_bin"

      # --- 2. bring up the server tier + bridge ---------------------------------
      # --serve-all carries the gateway (:${toString gatewayPort}) + metrics
      # (:${toString metricsPort}); --serve-sessions + --serve-fleet light up the
      # Agent + Fleet tabs (else those pages preflight as skipped).
      ensure_seam 1 serve-all --serve-all "127.0.0.1:${toString gatewayPort}" || { note_fail 1; contract_exit ""; }
      ensure_seam 0 serve-sessions --serve-sessions "127.0.0.1:${toString sessionsPort}" || true
      ensure_seam 0 serve-fleet --serve-fleet "127.0.0.1:${toString fleetPort}" || true

      echo "portal-e2e: (re)creating the grpc-web bridge"
      grpc-web-up || { echo "FAIL(harness): grpc-web bridge did not come up" >&2; note_fail 1; contract_exit ""; }

      # Wait for the metrics endpoint (the harness reads deltas from it).
      metrics_url="http://127.0.0.1:${toString metricsPort}/metrics"
      for _ in $(seq 1 30); do
        if curl -sf -o /dev/null "$metrics_url"; then break; fi
        sleep 1
      done
      if ! curl -sf -o "$workdir/metrics.before" "$metrics_url"; then
        echo "FAIL(harness): metrics endpoint $metrics_url unreachable" >&2
        note_fail 1; contract_exit ""
      fi

      # --- 3. drive the web app through the bridge -------------------------------
      # `flutter drive -d web-server` serves + drives its OWN instance of the app;
      # what matters is the app's grpc-web calls go to the Envoy bridge (via the
      # --dart-define endpoints), so the path is the browser's own.
      ( cd portal && flutter create --platforms=web --project-name agent_portal . >/dev/null 2>&1 || true )
      # The suite hands its per-action records back through `reportData`, which the
      # driver writes here; drop a prior run's file so we never read stale actions.
      resp="portal/build/integration_response_data.json"
      rm -f "$resp"

      "$chromedriver_bin" --port=4444 >"$workdir/chromedriver.log" 2>&1 &
      cd_pid=$!
      sleep 2

      echo "portal-e2e: driving the web app (headless chromium) through :${toString grpcWebPort}"
      drive_rc=0
      ( cd portal && flutter drive \
          --driver=test_driver/integration_test.dart \
          --target=integration_test/portal_e2e_test.dart \
          -d web-server --browser-name=chrome \
          --web-browser-flag=--headless=new \
          --web-browser-flag=--no-sandbox \
          --web-browser-flag=--disable-gpu \
          --web-browser-flag=--disable-dev-shm-usage \
          --web-browser-flag=--window-size=1600,1200 \
          --dart-define=PORTAL_GRPC_WEB_URL=http://127.0.0.1:${toString grpcWebPort} \
          --dart-define=PORTAL_SESSIONS_GRPC_WEB_URL=http://127.0.0.1:${toString grpcWebSessionsPort} \
          --dart-define=PORTAL_FLEET_GRPC_WEB_URL=http://127.0.0.1:${toString grpcWebFleetPort} \
          --dart-define=E2E_RUN_ID="$run_id" ) >"$workdir/drive.log" 2>&1 || drive_rc=$?
      if [ "$drive_rc" -ne 0 ]; then
        echo "FAIL(harness): flutter drive exited $drive_rc — last log lines:" >&2
        tail -n 25 "$workdir/drive.log" >&2 || true
        note_fail 1; contract_exit ""
      fi

      # Prefer the driver's `reportData` file (a browser `print` does not surface on
      # `flutter drive` stdout for the web device); fall back to the marker line.
      if [ -f "$resp" ] && jq -e . "$resp" >/dev/null 2>&1; then
        actions="$(cat "$resp")"
      else
        actions="$(grep -F 'PORTAL_E2E_ACTIONS ' "$workdir/drive.log" | tail -n1 | sed 's/^.*PORTAL_E2E_ACTIONS //')"
      fi
      if [ -z "$actions" ] || ! echo "$actions" | jq -e '.actions' >/dev/null 2>&1; then
        echo "FAIL(harness): no parseable action records from the drive (reportData missing)" >&2
        note_fail 1; contract_exit ""
      fi

      # --- 4. observability assertions per curated action -----------------------
      curl -sf -o "$workdir/metrics.after" "$metrics_url" || cp "$workdir/metrics.before" "$workdir/metrics.after"

      n="$(echo "$actions" | jq '.actions | length')"
      echo "portal-e2e: verifying $n driven action(s) from observability"
      for i in $(seq 0 $((n - 1))); do
        page="$(echo "$actions" | jq -r ".actions[$i].page")"
        el="$(echo "$actions" | jq -r ".actions[$i].element")"
        rpc="$(echo "$actions" | jq -r ".actions[$i].rpc")"
        value="$(echo "$actions" | jq -r ".actions[$i].value")"
        drove="$(echo "$actions" | jq -r ".actions[$i].outcome")"
        ms="$(echo "$actions" | jq -r ".actions[$i].ms")"

        if [ "$drove" = "skipped" ]; then
          echo "  [skip] $page/$el ($rpc) — $(echo "$actions" | jq -r ".actions[$i].detail")"
          append_record "$page" "$el" "$rpc" "skip" 0 "driver skipped: $(echo "$actions" | jq -r ".actions[$i].detail")"
          continue
        fi
        if [ "$drove" != "ok" ]; then
          echo "CONTRACT: $page/$el ($rpc) — driver reported '$drove': $(echo "$actions" | jq -r ".actions[$i].detail")" >&2
          append_record "$page" "$el" "$rpc" "fail" "$ms" "driver: $drove"
          note_fail 2; continue
        fi

        # (a) correct RPC fired + ok — a positive metrics delta.
        before="$(metric_val "$workdir/metrics.before" "$rpc")"
        after="$(metric_val "$workdir/metrics.after" "$rpc")"
        delta=$((after - before))
        if [ "$delta" -ge 1 ]; then
          echo "  [ok]   metrics: $rpc delta=$delta (outcome=ok)"
        else
          echo "CONTRACT: $page/$el — no ok-metrics delta for $rpc (before=$before after=$after)" >&2
          append_record "$page" "$el" "$rpc" "fail" "$ms" "no metrics delta"
          note_fail 2; continue
        fi

        # (b) intended state changed — the matching read RPC on the gateway. The
        # service name is dot-separated (agent.v1.<Service>), so match on the
        # service+method suffix (no leading slash).
        state_ok=1
        mapped=1
        case "$rpc" in
          *ProviderRegistryService/Put)
            got="$(grpcurl -d "{\"id\":\"$value\"}" -plaintext 127.0.0.1:${toString gatewayPort} \
                    agent.v1.ProviderRegistryService/Get 2>/dev/null || true)"
            echo "$got" | jq -e --arg id "$value" '.id == $id' >/dev/null 2>&1 || state_ok=0 ;;
          *ProviderRegistryService/Enable)
            got="$(grpcurl -d "{\"id\":\"$value\"}" -plaintext 127.0.0.1:${toString gatewayPort} \
                    agent.v1.ProviderRegistryService/Get 2>/dev/null || true)"
            echo "$got" | jq -e '.enabled == true' >/dev/null 2>&1 || state_ok=0 ;;
          *PromptService/SetActivePersonality)
            got="$(grpcurl -d '{}' -plaintext 127.0.0.1:${toString gatewayPort} \
                    agent.v1.PromptService/GetActivePersonality 2>/dev/null || true)"
            echo "$got" | jq -e --arg id "$value" '(.id // .name // "") == $id' >/dev/null 2>&1 || state_ok=0 ;;
          *) mapped=0; echo "  [warn] no read-RPC mapping for $rpc — proven by metrics delta only" ;;
        esac
        if [ "$mapped" -eq 1 ] && [ "$state_ok" -eq 0 ]; then
          echo "CONTRACT: $page/$el — read-RPC did not reflect the change for $rpc (value=$value)" >&2
          append_record "$page" "$el" "$rpc" "fail" "$ms" "state unchanged"
          note_fail 2; continue
        fi
        [ "$mapped" -eq 1 ] && echo "  [ok]   state: read-RPC reflects '$value'"

        append_record "$page" "$el" "$rpc" "pass" "$ms" "metrics delta + read-RPC verified"
      done

      # --- 5. curated OTLP span (the tracing pipe; best-effort) ------------------
      # inc 10 is the authoritative cross-hop trace proof; here we confirm the
      # gateway exported a recent grpc.server span into ClickHouse. Best-effort:
      # a ClickHouse that is down/unreachable is a WARN, not a contract failure.
      span_n=0
      if "$runtime" ps --format '{{.Names}}' 2>/dev/null | grep -qx "${chContainer}"; then
        for _ in $(seq 1 15); do
          span_n="$("$runtime" exec "${chContainer}" clickhouse-client -q \
            "SELECT count() FROM default.otel_traces WHERE ServiceName='agent-gateway' AND SpanName='grpc.server' AND Timestamp > now() - INTERVAL 3 MINUTE" 2>/dev/null || echo 0)"
          case "$span_n" in "" | *[!0-9]* ) span_n=0 ;; esac
          if [ "$span_n" -ge 1 ]; then break; fi
          sleep 2
        done
      fi
      if [ "$span_n" -ge 1 ]; then
        echo "portal-e2e: curated span check — $span_n recent agent-gateway grpc.server span(s) in ClickHouse"
      else
        echo "portal-e2e: [warn] no recent gateway span found in ClickHouse (obs down, or telemetry disabled) — see inc 10 for the authoritative trace proof"
      fi

      # --- 5b. tidy up the rows this run created --------------------------------
      # The Router upstreams carry a unique per-run id; remove them so repeated
      # runs don't accumulate registry entries. Best-effort (an active-personality
      # change has no prior value to restore, so it is left as set).
      for uid in $(echo "$actions" | jq -r '.actions[] | select(.rpc|endswith("/Put")) | .value' | sort -u); do
        [ -n "$uid" ] || continue
        grpcurl -d "{\"id\":\"$uid\"}" -plaintext 127.0.0.1:${toString gatewayPort} \
          agent.v1.ProviderRegistryService/Delete >/dev/null 2>&1 || true
      done

      # --- 5c. perf rows -> agent.portal_gui_perf (inc 09; best-effort trend record) ---
      # Every driven action doubles as a timing sample. We record the client-perceived
      # interaction latency (from the driver) and the SERVER-side truth (the :${toString metricsPort}
      # histogram delta for the RPC), tag each RPC sample with the gateway span's
      # trace_id (looked up in the agent CH's default.otel_traces by the short span name), and
      # stream the batch to the same agent ClickHouse over HTTP JSONEachRow. This is
      # observability, NOT a gate: any failure here is a warning, never a contract fail.
      : >"$workdir/perf.jsonl"

      # git / host provenance (best-effort; a detached or dirty tree still records).
      commit_sha="$(git rev-parse HEAD 2>/dev/null || echo "")"
      branch="$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo "")"
      git_dirty=0
      if [ -n "$(git status --porcelain 2>/dev/null)" ]; then git_dirty=1; fi
      host="$(uname -n 2>/dev/null || echo "")"
      pr_number="''${PORTAL_PR_NUMBER:-0}"
      case "$pr_number" in "" | *[!0-9]* ) pr_number=0 ;; esac

      # Full gRPC path -> the SERVER span's short op name (an info_span "grpc.server"
      # carrying the short name in SpanAttributes['rpc'], crates/agent-grpc/src/server/*).
      # "" for an RPC we do not map -> no trace lookup for it.
      short_rpc() {
        case "$1" in
          *ProviderRegistryService/Put) echo registry.put ;;
          *ProviderRegistryService/Enable) echo registry.enable ;;
          *ProviderRegistryService/Get) echo registry.get ;;
          *PromptService/SetActivePersonality) echo prompt.set_active_personality ;;
          *) echo "" ;;
        esac
      }

      # Newest gateway trace_id for a short RPC name within the run window. The value is
      # SERVER-supplied (untrusted): accept ONLY hex, else "" — so a hostile trace id
      # can never reach the row (and JSONEachRow carries it as data, never SQL).
      # Best-effort: no ClickHouse, export lag, or tracing off -> "".
      trace_for() {
        local sr="$1" tid=""
        [ -n "$sr" ] || { echo ""; return; }
        "$runtime" ps --format '{{.Names}}' 2>/dev/null | grep -qx "${chContainer}" || { echo ""; return; }
        for _ in $(seq 1 6); do
          tid="$("$runtime" exec "${chContainer}" clickhouse-client -q \
            "SELECT TraceId FROM default.otel_traces WHERE ServiceName='agent-gateway' AND SpanAttributes['rpc']='$sr' AND Timestamp > now() - INTERVAL 5 MINUTE ORDER BY Timestamp DESC LIMIT 1" 2>/dev/null || echo "")"
          if [ -n "$tid" ]; then break; fi
          sleep 1
        done
        case "$tid" in
          "" | *[!0-9a-fA-F]* ) echo "" ;;
          * ) echo "$tid" ;;
        esac
      }

      # Sum a histogram field (sum|count) for a full-path RPC. Server-produced, so
      # accept ONLY a clean non-negative decimal per line (a hostile value -> 0).
      hist_field() {
        grep -F "agent_grpc_server_rpc_seconds_$2{" "$1" 2>/dev/null \
          | grep -F "rpc=\"$3\"" \
          | awk '{ v=$NF; if (v ~ /^[0-9]+(\.[0-9]+)?$/) s+=v } END { printf "%.9f", s+0 }'
      }

      # emit one validated perf row: page element rpc metric phase value_ms outcome trace_id
      emit_perf_row() {
        awk -v x="$6" 'BEGIN{ exit !(x ~ /^[0-9]+(\.[0-9]+)?$/) }' || return 0
        jq -cn \
          --arg run_id "$run_id" --argjson pr_number "$pr_number" \
          --arg commit_sha "$commit_sha" --arg branch "$branch" \
          --argjson git_dirty "$git_dirty" --arg host "$host" --arg layer "e2e" \
          --arg page "$1" --arg element_id "$2" --arg test_name "portal-e2e" \
          --arg phase "$5" --arg step "" --arg rpc_method "$3" \
          --arg metric "$4" --argjson value_ms "$6" --argjson iteration 1 \
          --arg outcome "$7" --arg trace_id "$8" \
          '{run_id:$run_id, pr_number:$pr_number, commit_sha:$commit_sha, branch:$branch,
            git_dirty:$git_dirty, host:$host, layer:$layer, page:$page,
            element_id:$element_id, test_name:$test_name, phase:$phase, step:$step,
            rpc_method:$rpc_method, metric:$metric, value_ms:$value_ms,
            iteration:$iteration, outcome:$outcome, trace_id:$trace_id}' \
          >>"$workdir/perf.jsonl"
      }

      # Build rows from the report the assertion loop already wrote (its outcome is the
      # verified pass|fail|skip). One interaction_ms row per action; one grpc_server_ms
      # row per non-skipped mapped RPC (server truth), both tagged with the trace_id.
      while IFS= read -r rec; do
        [ -n "$rec" ] || continue
        p_page="$(echo "$rec" | jq -r '.page')"
        p_el="$(echo "$rec" | jq -r '.element_id')"
        p_rpc="$(echo "$rec" | jq -r '.rpc_fired[0] // ""')"
        p_oc="$(echo "$rec" | jq -r '.outcome')"
        p_ms="$(echo "$rec" | jq -r '.duration_ms')"
        p_tid=""
        if [ "$p_oc" != "skip" ]; then p_tid="$(trace_for "$(short_rpc "$p_rpc")")"; fi
        emit_perf_row "$p_page" "$p_el" "$p_rpc" "interaction_ms" "action" "$p_ms" "$p_oc" "$p_tid"
        if [ "$p_oc" != "skip" ] && [ -n "$p_rpc" ]; then
          sb="$(hist_field "$workdir/metrics.before" sum "$p_rpc")"
          sa="$(hist_field "$workdir/metrics.after" sum "$p_rpc")"
          cb="$(hist_field "$workdir/metrics.before" count "$p_rpc")"
          ca="$(hist_field "$workdir/metrics.after" count "$p_rpc")"
          server_ms="$(awk -v sb="$sb" -v sa="$sa" -v cb="$cb" -v ca="$ca" \
            'BEGIN{ dc=ca-cb; ds=sa-sb; if (dc>0 && ds>=0) printf "%.3f", (ds/dc)*1000 }')"
          if [ -n "$server_ms" ]; then emit_perf_row "$p_page" "$p_el" "$p_rpc" "grpc_server_ms" "rpc" "$server_ms" "$p_oc" "$p_tid"; fi
        fi
      done <"$workdir/report.jsonl"

      if [ -s "$workdir/perf.jsonl" ]; then
        perf_rows="$(wc -l <"$workdir/perf.jsonl" | tr -d ' ')"
        {
          printf 'INSERT INTO agent.portal_gui_perf FORMAT JSONEachRow\n'
          cat "$workdir/perf.jsonl"
        } >"$workdir/perf.post"
        if curl -sf --data-binary @"$workdir/perf.post" "http://127.0.0.1:${toString chHttpPort}/" >/dev/null 2>&1; then
          echo "portal-e2e: inserted $perf_rows perf row(s) into agent.portal_gui_perf (run_id=$run_id)"
        else
          echo "portal-e2e: [warn] perf insert skipped — agent ClickHouse :${toString chHttpPort} down, or table missing (run 'nix run .#clickhouse-migrate')"
        fi
      fi

      # --- 6. report ------------------------------------------------------------
      report="''${PORTAL_E2E_REPORT:-$workdir/report.jsonl}"
      [ "$report" != "$workdir/report.jsonl" ] && cp "$workdir/report.jsonl" "$report" 2>/dev/null || true
      echo ""
      echo "== portal-e2e report (page -> element -> case) =="
      ${portal-test-report}/bin/portal-test-report "$workdir/report.jsonl" || true
      echo ""
      echo "report JSONL: $report"

      contract_exit "PASS: portal-e2e drove the curated mutating subset over the real wire; every non-skipped action proven by metrics delta + read-RPC."
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
    portal-redeploy
    portal-e2e
    ;
}
