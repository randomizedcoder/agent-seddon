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
  agent,
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
                              allow_headers: keep-alive,user-agent,cache-control,content-type,content-transfer-encoding,x-grpc-web,x-user-agent,grpc-timeout,x-agent-user-id,x-agent-session-id
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
                              allow_headers: keep-alive,user-agent,cache-control,content-type,content-transfer-encoding,x-grpc-web,x-user-agent,grpc-timeout,x-agent-user-id,x-agent-session-id
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
                              allow_headers: keep-alive,user-agent,cache-control,content-type,content-transfer-encoding,x-grpc-web,x-user-agent,grpc-timeout,x-agent-user-id,x-agent-session-id
                              max_age: "1728000"
                              expose_headers: grpc-status,grpc-message
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
      echo "      :${toString grpcWebFleetPort}  -> fleet    :${toString fleetPort}"
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
in
{
  inherit
    gen-dart
    portal
    portal-web
    grpc-web-up
    grpc-web-down
    portal-redeploy
    ;
}
