# nix/lib/mk-container-app.nix
#
# The container-lifecycle apps that are byte-identical across the container modules
# (clickhouse / clickstack / prometheus / grafana): the `*-down` remover, the
# `*-client` exec wrapper, and the `*-logs` follower. Only the `*-up` app is
# genuinely per-service (network mode, port maps, schema/dashboard provisioning),
# so it stays in each module.
#
# Runtime: each app honours `CONTAINER_RUNTIME` (default `docker`), so a
# docker-less, podman-only host (e.g. the headless l2 box) runs the same apps with
# `CONTAINER_RUNTIME=podman`. podman's CLI is drop-in for run/ps/rm/exec/logs.
#
#   c = import ../lib/mk-container-app.nix { inherit pkgs versions; };
#   { name = "clickhouse"; container = versions.clickhouseContainerName; }
#   → c.down {...}  c.client { exec = "clickhouse-client"; }  c.logs {...}
{
  pkgs,
  versions,
}:
let
  runtimes = [
    versions.docker
    versions.podman
  ];
  # Shell prelude: resolve the runtime once per app.
  pickRuntime = ''runtime="''${CONTAINER_RUNTIME:-docker}"'';
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
      runtimeInputs = runtimes;
      text = ''
        set -euo pipefail
        ${pickRuntime}
        if "$runtime" ps -a --format '{{.Names}}' | grep -qx "${container}"; then
          echo "==> removing container '${container}' (data is discarded)"
          "$runtime" rm -f "${container}" >/dev/null
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
      runtimeInputs = runtimes;
      text = ''
        set -euo pipefail
        ${pickRuntime}
        if ! "$runtime" ps --format '{{.Names}}' | grep -qx "${container}"; then
          echo "${name}-client: container '${container}' is not running — run 'nix run .#${name}-up' first" >&2
          exit 1
        fi
        exec "$runtime" exec -i "${container}" ${exec} "$@"
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
      runtimeInputs = runtimes;
      text = ''
        set -euo pipefail
        ${pickRuntime}
        exec "$runtime" logs -f "${container}"
      '';
    };
}
