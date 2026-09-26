# Postgres container apps (up / down / client / logs) for the transactional config
# store (`agent-config-store` postgres tier, config C41 / A2). The mirror of
# `nix/clickhouse/default.nix`: a bespoke `postgres-up` (the readiness barrier
# lives here) plus `down`/`client`/`logs` from the shared container-app factory.
#
# Production-shaped (PG-04): the data lives in a PERSISTENT named volume (survives
# `postgres-down`), the server is started with tuning flags, and the password is a
# run-time value ($AGENT_PG_PASSWORD, dev default otherwise) rather than a baked-in
# constant. It is still opt-in — this is how `nix run .#integration`'s
# `pg-integration` harness (and a developer by hand) gets a real server; it is never
# part of `nix flake check`. The NixOS-native service (PG-05) is the production
# deployment; this container is the quick local/CI spin. The server applies no schema
# — `agent-config-store` creates its own on connect (`migrate_on_start`), so
# `postgres-up` only needs the DB up and reachable.
{
  pkgs,
  lib,
  versions,
}:
let
  name = versions.postgresContainerName;
  # Fully-qualified so podman (whose short-name resolution is disabled on the
  # headless l2 box) resolves it; docker treats the docker.io/ prefix as a no-op.
  image = "docker.io/${versions.postgresImage}";
  port = toString versions.postgresPort;
  db = versions.postgresDatabase;
  user = versions.postgresUser;
  password = versions.postgresPassword;
  dataVolume = versions.postgresDataVolume;
  # Tuning flags (`-c key=value`), passed to the `postgres` server via the image's
  # entrypoint (everything after the image name is forwarded to the server).
  tuning = lib.concatStringsSep " " [
    "-c shared_buffers=${versions.postgresSharedBuffers}"
    "-c max_connections=${toString versions.postgresMaxConnections}"
    "-c work_mem=${versions.postgresWorkMem}"
    "-c effective_cache_size=${versions.postgresEffectiveCacheSize}"
  ];

  # Shared container-lifecycle apps (the identical *-down/*-client bodies).
  c = import ../lib/mk-container-app.nix { inherit pkgs versions; };

  postgres-up = pkgs.writeShellApplication {
    name = "postgres-up";
    runtimeInputs = c.runtimes;
    text = ''
        set -euo pipefail
        ${c.pickRuntime}

        if ! "$runtime" info >/dev/null 2>&1; then
          echo "postgres-up: '$runtime' not reachable — is it installed/running?" >&2
          exit 1
        fi

        # The password is a run-time value: $AGENT_PG_PASSWORD wins, else the dev/CI
        # default. It is consumed by the image ONLY on first init of an empty data
        # volume; on a restart with existing data the role keeps its original
        # password (standard postgres-image behaviour), so changing it later means
        # removing the volume ('$runtime' volume rm "${dataVolume}").
        pw="''${AGENT_PG_PASSWORD:-${password}}"

        if "$runtime" ps -a --format '{{.Names}}' | grep -qx "${name}"; then
          echo "==> container '${name}' already exists; (re)starting it"
          "$runtime" start "${name}" >/dev/null
        else
          echo "==> starting Postgres (${image})"
          # Published on 127.0.0.1 only (host-local). Data lives in the named volume
          # '${dataVolume}' so it SURVIVES 'postgres-down' (which removes only the
          # container) — the mirror of a real deployment. Tuning is applied as
          # server startup flags.
          "$runtime" run -d \
            --name "${name}" \
            -e POSTGRES_USER="${user}" \
            -e POSTGRES_PASSWORD="$pw" \
            -e POSTGRES_DB="${db}" \
            -v "${dataVolume}":/var/lib/postgresql/data \
            -p 127.0.0.1:${port}:5432 \
            "${image}" ${tuning} >/dev/null
        fi

        echo -n "==> waiting for Postgres to accept connections"
        # Barrier: poll `pg_isready` INSIDE the container (no host psql needed),
        # sleeping only BETWEEN probes — the health probe is the barrier.
        ready=0
        for _ in $(seq 1 60); do
          if "$runtime" exec "${name}" pg_isready -U "${user}" -d "${db}" >/dev/null 2>&1; then
            echo " ready"
            ready=1
            break
          fi
          echo -n "."
          sleep 1
        done
        if [ "$ready" -ne 1 ]; then
          echo "" >&2
          echo "postgres-up: server did not become ready in time" >&2
          exit 1
        fi

        cat <<EOF

      Postgres is up.
        Port: localhost:${port}   Database: ${db}   User: ${user}
        DSN:  postgres://${user}:$pw@127.0.0.1:${port}/${db}
        Data: volume '${dataVolume}' (survives postgres-down)

        Client: nix run .#postgres-client -- -c 'SELECT 1'
        Logs:   nix run .#postgres-logs
        Stop:   nix run .#postgres-down   (keeps the data volume)
      EOF
    '';
  };

  # Removes the CONTAINER only; the named data volume '${dataVolume}' persists, so a
  # later `postgres-up` resumes the same database (remove the volume by hand to reset).
  postgres-down = c.down {
    name = "postgres";
    container = name;
  };

  # `nix run .#postgres-logs` → follow the server logs.
  postgres-logs = c.logs {
    name = "postgres";
    container = name;
  };

  # `nix run .#postgres-client -- <args>` → psql inside the container, e.g.
  # `-- -c 'SELECT count(*) FROM cards'`. PGPASSWORD/-U/-d are pre-set.
  postgres-client = c.client {
    name = "postgres";
    container = name;
    exec = "psql -U ${user} -d ${db}";
  };
in
{
  inherit
    postgres-up
    postgres-down
    postgres-client
    postgres-logs
    ;
}
