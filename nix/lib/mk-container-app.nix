# nix/lib/mk-container-app.nix
#
# The container-lifecycle apps that are byte-identical across the docker modules
# (clickhouse / clickstack / prometheus / grafana): the `*-down` remover, the
# `*-client` exec wrapper, and the `*-logs` follower. Only the `*-up` app is
# genuinely per-service (network mode, port maps, schema/dashboard provisioning),
# so it stays in each module.
#
#   c = import ../lib/mk-container-app.nix { inherit pkgs versions; };
#   { name = "clickhouse"; container = versions.clickhouseContainerName; }
#   → c.down {...}  c.client { exec = "clickhouse-client"; }  c.logs {...}
{
  pkgs,
  versions,
}:
let
  docker = versions.docker;
in
{
  # `<name>-down` — remove the container (data discarded), idempotent.
  down =
    {
      name,
      container,
    }:
    pkgs.writeShellApplication {
      name = "${name}-down";
      runtimeInputs = [ docker ];
      text = ''
        set -euo pipefail
        if docker ps -a --format '{{.Names}}' | grep -qx "${container}"; then
          echo "==> removing container '${container}' (data is discarded)"
          docker rm -f "${container}" >/dev/null
          echo "done"
        else
          echo "container '${container}' not found — nothing to do"
        fi
      '';
    };

  # `<name>-client` — exec a client inside the running container. The binary is
  # `<name>-client-wrapper` so it doesn't shadow the real client on PATH.
  client =
    {
      name,
      container,
      exec,
    }:
    pkgs.writeShellApplication {
      name = "${name}-client-wrapper";
      runtimeInputs = [ docker ];
      text = ''
        set -euo pipefail
        if ! docker ps --format '{{.Names}}' | grep -qx "${container}"; then
          echo "${name}-client: container '${container}' is not running — run 'nix run .#${name}-up' first" >&2
          exit 1
        fi
        exec docker exec -i "${container}" ${exec} "$@"
      '';
    };

  # `<name>-logs` — follow the container logs.
  logs =
    {
      name,
      container,
    }:
    pkgs.writeShellApplication {
      name = "${name}-logs";
      runtimeInputs = [ docker ];
      text = ''
        set -euo pipefail
        exec docker logs -f "${container}"
      '';
    };
}
