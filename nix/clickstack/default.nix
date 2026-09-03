# nix/clickstack/default.nix
#
# ClickStack (HyperDX all-in-one) container lifecycle as Nix apps (docker). It
# bundles an OpenTelemetry collector (OTLP :4317/:4318), ClickHouse, and the
# HyperDX UI (:8080) in one image — the receiver the agent's OTLP tracing exports
# to. Data lives in the container's writable layer; `clickstack-down` removes it.
#
# This is separate from the plain `clickhouse` container (which backs the native
# `agent_events` sink). We deliberately do NOT publish ClickStack's bundled
# ClickHouse ports (8123/9000), so the two can run side by side.
{
  pkgs,
  lib,
  versions,
}:

let
  name = versions.clickstackContainerName;
  image = versions.clickstackImage;
  uiPort = toString versions.clickstackUiPort;
  otlpGrpcPort = toString versions.clickstackOtlpGrpcPort;
  otlpHttpPort = toString versions.clickstackOtlpHttpPort;

  # Shared container-lifecycle apps (the identical *-down/*-client/*-logs bodies).
  c = import ../lib/mk-container-app.nix { inherit pkgs versions; };
in
{
  clickstack-up = pkgs.writeShellApplication {
    name = "clickstack-up";
    runtimeInputs = [
      versions.docker
      versions.podman
      versions.curl
    ];
    text = ''
        set -euo pipefail
        runtime="''${CONTAINER_RUNTIME:-docker}"

        if ! "$runtime" info >/dev/null 2>&1; then
          echo "clickstack-up: '$runtime' not reachable — is it installed/running?" >&2
          exit 1
        fi

        if "$runtime" ps -a --format '{{.Names}}' | grep -qx "${name}"; then
          echo "==> container '${name}' already exists; (re)starting it"
          "$runtime" start "${name}" >/dev/null
        else
          echo "==> starting ClickStack / HyperDX all-in-one (${image})"
          # UI publishes on CLICKSTACK_UI_HOST (default 127.0.0.1; set 0.0.0.0 to
          # reach the HyperDX UI from the LAN). OTLP receivers stay host-local —
          # the agent exports to them over loopback. Bundled ClickHouse stays
          # internal (query it via clickstack-client).
          "$runtime" run -d \
            --name "${name}" \
            -p "''${CLICKSTACK_UI_HOST:-127.0.0.1}:${uiPort}:8080" \
            -p 127.0.0.1:${otlpGrpcPort}:4317 \
            -p 127.0.0.1:${otlpHttpPort}:4318 \
            "${image}" >/dev/null
        fi

        echo -n "==> waiting for the HyperDX UI to come up (can take ~30-60s)"
        for _ in $(seq 1 120); do
          if curl -sf -o /dev/null "http://localhost:${uiPort}" 2>/dev/null; then
            echo " ready"
            break
          fi
          echo -n "."
          sleep 1
        done

        cat <<EOF

      ClickStack (HyperDX) is up.
        UI:        http://localhost:${uiPort}
        OTLP gRPC: localhost:${otlpGrpcPort}   (set [telemetry] otlp_endpoint = "http://localhost:${otlpGrpcPort}")
        OTLP HTTP: localhost:${otlpHttpPort}

      First run: open the UI, create a local account, then copy the *Ingestion
      API Key* from Team Settings into your config's
        [telemetry] otlp_headers = "authorization=<ingestion-key>"

        Query traces: nix run .#clickstack-client -- -q 'SHOW TABLES FROM default'
        Logs:         nix run .#clickstack-logs
        Stop:         nix run .#clickstack-down
      EOF
    '';
  };

  clickstack-down = c.down {
    name = "clickstack";
    container = name;
  };

  clickstack-logs = c.logs {
    name = "clickstack";
    container = name;
  };

  # `nix run .#clickstack-client -- <args>` → clickhouse-client inside the
  # all-in-one, e.g. `-- -q 'SELECT count() FROM default.otel_traces'`.
  clickstack-client = c.client {
    name = "clickstack";
    container = name;
    exec = "clickhouse-client";
  };
}
