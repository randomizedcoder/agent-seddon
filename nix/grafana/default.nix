# nix/grafana/default.nix
#
# Grafana container lifecycle as Nix apps (docker) — dashboards over the Prometheus
# datasource. Mirrors the clickhouse/clickstack/prometheus modules.
#
# Networking (Linux): runs with `--network host` so Grafana serves on host :3000
# and reaches Prometheus at 127.0.0.1:9090. Provisioning (the datasource + the
# dashboard) is bind-mounted, so the "agent-seddon" dashboard appears on first
# start. Anonymous Admin access is enabled for a friction-free *local* view — do
# not expose this container to a network.
{
  pkgs,
  lib,
  versions,
}:

let
  name = versions.grafanaContainerName;
  # Fully-qualified so podman (whose short-name resolution is disabled on the
  # headless l2 box) resolves it; docker treats the docker.io/ prefix as a no-op.
  image = "docker.io/${versions.grafanaImage}";
  port = toString versions.grafanaPort;

  provisioning = ./provisioning;
  dashboards = ./dashboards;

  # Shared container-lifecycle apps (the identical *-down/*-client/*-logs bodies).
  c = import ../lib/mk-container-app.nix { inherit pkgs versions; };
in
{
  grafana-up = pkgs.writeShellApplication {
    name = "grafana-up";
    runtimeInputs = c.runtimes ++ [ versions.curl ];
    text = ''
        set -euo pipefail
        ${c.pickRuntime}

        if ! "$runtime" info >/dev/null 2>&1; then
          echo "grafana-up: '$runtime' not reachable — is it installed/running?" >&2
          exit 1
        fi

        if "$runtime" ps -a --format '{{.Names}}' | grep -qx "${name}"; then
          echo "==> container '${name}' already exists; (re)starting it"
          "$runtime" start "${name}" >/dev/null
        else
          echo "==> starting Grafana (${image})"
          # `--network host` (Linux): Grafana serves on host :${port} and reaches
          # Prometheus at 127.0.0.1:9090 (the provisioned datasource).
          "$runtime" run -d \
            --name "${name}" \
            --network host \
            -e GF_SERVER_HTTP_PORT="${port}" \
            -e GF_AUTH_ANONYMOUS_ENABLED=true \
            -e GF_AUTH_ANONYMOUS_ORG_ROLE=Admin \
            -e GF_AUTH_DISABLE_LOGIN_FORM=true \
            -v "${provisioning}:/etc/grafana/provisioning:ro" \
            -v "${dashboards}:/var/lib/grafana/dashboards:ro" \
            "${image}" >/dev/null
        fi

        echo -n "==> waiting for Grafana to be ready"
        for _ in $(seq 1 60); do
          if curl -sf -o /dev/null "http://localhost:${port}/api/health" 2>/dev/null; then
            echo " ready"
            break
          fi
          echo -n "."
          sleep 1
        done

        cat <<EOF

      Grafana is up.
        UI:        http://localhost:${port}   (anonymous Admin; Dashboards → agent-seddon)
        Datasource + dashboard are provisioned from ${provisioning}

      Make sure Prometheus is up (nix run .#prometheus-up) and the agent is running
      with [metrics] enabled = true.
        Stop:      nix run .#grafana-down
      EOF
    '';
  };

  grafana-down = c.down {
    name = "grafana";
    container = name;
  };
}
