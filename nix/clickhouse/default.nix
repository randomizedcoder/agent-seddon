# nix/clickhouse/default.nix
#
# ClickHouse container lifecycle as Nix apps (docker). The harness streams its
# transaction history / logs / usage here (see ./schema.sql and Phase 2 of the
# plan). Data lives in the container's writable layer — `clickhouse-down`
# removes it. Add a named volume here if you want persistence across restarts.
#
{
  pkgs,
  lib,
  versions,
}:

let
  name = versions.clickhouseContainerName;
  # Fully-qualified so podman (whose short-name resolution is disabled on the
  # headless l2 box) resolves it; docker treats the docker.io/ prefix as a no-op.
  image = "docker.io/${versions.clickhouseImage}";
  httpPort = toString versions.clickhouseHttpPort;
  nativePort = toString versions.clickhouseNativePort;
  db = versions.clickhouseDatabase;

  # The schema, materialized into the Nix store so we can bind-mount it.
  schema = ./schema.sql;
  # Dev override widening the `default` user's allowed networks (see users.xml).
  usersOverride = ./users.xml;

  # Shared container-lifecycle apps (the identical *-down/*-client/*-logs bodies).
  c = import ../lib/mk-container-app.nix { inherit pkgs versions; };
in
{
  clickhouse-up = pkgs.writeShellApplication {
    name = "clickhouse-up";
    runtimeInputs = c.runtimes ++ [ versions.curl ];
    text = ''
        set -euo pipefail
        ${c.pickRuntime}

        if ! "$runtime" info >/dev/null 2>&1; then
          echo "clickhouse-up: '$runtime' not reachable — is it installed/running?" >&2
          exit 1
        fi

        if "$runtime" ps -a --format '{{.Names}}' | grep -qx "${name}"; then
          echo "==> container '${name}' already exists; (re)starting it"
          "$runtime" start "${name}" >/dev/null
        else
          echo "==> starting ClickHouse (${image})"
          # Ports are published on 127.0.0.1 only (host-local); the users.xml
          # override lets the `default` user connect over HTTP from the host.
          "$runtime" run -d \
            --name "${name}" \
            -p 127.0.0.1:${httpPort}:8123 \
            -p 127.0.0.1:${nativePort}:9000 \
            -v "${schema}:/docker-entrypoint-initdb.d/00-schema.sql:ro" \
            -v "${usersOverride}:/etc/clickhouse-server/users.d/99-allow-remote-default.xml:ro" \
            "${image}" >/dev/null
        fi

        echo -n "==> waiting for ClickHouse to accept connections"
        for _ in $(seq 1 60); do
          if [ "$(curl -s "http://localhost:${httpPort}/ping" 2>/dev/null || true)" = "Ok." ]; then
            echo " ready"
            break
          fi
          echo -n "."
          sleep 1
        done

        # Re-apply the schema idempotently (the initdb mount only runs on a fresh
        # data dir; this covers an already-existing container/volume).
        echo "==> applying schema (database '${db}')"
        "$runtime" exec -i "${name}" clickhouse-client --multiquery < "${schema}"

        cat <<EOF

      ClickHouse is up.
        HTTP:    http://localhost:${httpPort}   (/ping, /play)
        Native:  localhost:${nativePort}        (clickhouse-client --port ${nativePort})
        Database: ${db}   Tables: agent_events, agent_logs, agent_usage

        Query:   nix run .#clickhouse-client -- -q 'SHOW TABLES FROM ${db}'
        Stop:    nix run .#clickhouse-down
      EOF
    '';
  };

  clickhouse-down = c.down {
    name = "clickhouse";
    container = name;
  };

  # `nix run .#clickhouse-client -- <args>` → clickhouse-client inside the
  # container, e.g. `-- -q 'SELECT count() FROM agent.agent_events'`.
  clickhouse-client = c.client {
    name = "clickhouse";
    container = name;
    exec = "clickhouse-client";
  };
}
