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
in
{
  clickstack-up = pkgs.writeShellApplication {
    name = "clickstack-up";
    runtimeInputs = [
      versions.docker
      versions.curl
    ];
    text = ''
        set -euo pipefail

        if ! docker info >/dev/null 2>&1; then
          echo "clickstack-up: docker daemon not reachable — is it running?" >&2
          exit 1
        fi

        if docker ps -a --format '{{.Names}}' | grep -qx "${name}"; then
          echo "==> container '${name}' already exists; (re)starting it"
          docker start "${name}" >/dev/null
        else
          echo "==> starting ClickStack / HyperDX all-in-one (${image})"
          # Publish only the UI + OTLP receivers on 127.0.0.1 (host-local). The
          # bundled ClickHouse stays internal (query it via clickstack-client).
          docker run -d \
            --name "${name}" \
            -p 127.0.0.1:${uiPort}:8080 \
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

  clickstack-down = pkgs.writeShellApplication {
    name = "clickstack-down";
    runtimeInputs = [ versions.docker ];
    text = ''
      set -euo pipefail
      if docker ps -a --format '{{.Names}}' | grep -qx "${name}"; then
        echo "==> removing container '${name}' (data is discarded)"
        docker rm -f "${name}" >/dev/null
        echo "done"
      else
        echo "container '${name}' not found — nothing to do"
      fi
    '';
  };

  clickstack-logs = pkgs.writeShellApplication {
    name = "clickstack-logs";
    runtimeInputs = [ versions.docker ];
    text = ''
      set -euo pipefail
      exec docker logs -f "${name}"
    '';
  };

  # `nix run .#clickstack-client -- <args>` → clickhouse-client inside the
  # all-in-one, e.g. `-- -q 'SELECT count() FROM default.otel_traces'`.
  clickstack-client = pkgs.writeShellApplication {
    name = "clickstack-client-wrapper";
    runtimeInputs = [ versions.docker ];
    text = ''
      set -euo pipefail
      if ! docker ps --format '{{.Names}}' | grep -qx "${name}"; then
        echo "clickstack-client: container '${name}' is not running — run 'nix run .#clickstack-up' first" >&2
        exit 1
      fi
      exec docker exec -i "${name}" clickhouse-client "$@"
    '';
  };
}
