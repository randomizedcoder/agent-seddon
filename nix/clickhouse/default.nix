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
  # Server settings (config.d) and the users override (users.d); see each file.
  serverOverride = ./config.xml;
  usersOverride = ./users.xml;
  # Where the rendered admin-hash override is mounted (security-hardening S16).
  credsMount = "/etc/clickhouse-server/users.d/50-agent-credentials.xml";

  # Shared container-lifecycle apps (the identical *-down/*-client/*-logs bodies).
  c = import ../lib/mk-container-app.nix { inherit pkgs versions; };

  # `clickhouse-creds` — the per-login password files + rendered admin override
  # (test/clickhouse/ch_creds.py, tested by the `ch-creds-tests` check). Every
  # ClickHouse app below goes through it; `nix run .#clickhouse-creds -- path writer`
  # tells an operator which file to point `[telemetry] password_file` at.
  clickhouse-creds = pkgs.writeShellApplication {
    name = "clickhouse-creds";
    runtimeInputs = [ pkgs.python3 ];
    text = ''
      exec python3 "${../../test/clickhouse}/ch_creds.py" "$@"
    '';
  };

  # Shell prelude shared by the apps that run SQL as the admin: the admin password is
  # read from its 0600 file into the environment and handed to `clickhouse-client` via
  # `exec -e CLICKHOUSE_PASSWORD` — never on a command line. `admin_client ARGS…` runs
  # clickhouse-client inside the container as the admin.
  adminPrelude = ''
    clickhouse-creds ensure
    CLICKHOUSE_PASSWORD="$(cat "$(clickhouse-creds path admin)")"
    export CLICKHOUSE_PASSWORD
    admin_client() { "$runtime" exec -i -e CLICKHOUSE_PASSWORD "${name}" clickhouse-client "$@"; }
    # Refuse a container created before S16: it has no admin password and still lets
    # users without a row policy read every row. Recreating it discards the telemetry
    # in its writable layer, so that is the operator's call, not ours.
    require_hardened() {
      if ! "$runtime" inspect -f '{{range .Mounts}}{{.Destination}} {{end}}' "${name}" \
          | grep -q "${credsMount}"; then
        echo "clickhouse: container '${name}' predates the credential lockdown (S16):" >&2
        echo "  its admin has no password and its row-policy default is open." >&2
        echo "  Recreate it (this DISCARDS its telemetry):" >&2
        echo "    nix run .#clickhouse-down && nix run .#clickhouse-up" >&2
        exit 1
      fi
    }
    apply_schema() {
      echo "==> applying schema (database '${db}')"
      admin_client --multiquery < "${schema}"
      echo "==> setting the agent_writer / agent_reader / agent_viewer passwords"
      clickhouse-creds alter-sql | admin_client --multiquery
    }
  '';
in
{
  inherit clickhouse-creds;

  clickhouse-up = pkgs.writeShellApplication {
    name = "clickhouse-up";
    runtimeInputs = c.runtimes ++ [
      versions.curl
      clickhouse-creds
    ];
    text = ''
        set -euo pipefail
        ${c.pickRuntime}
        ${adminPrelude}

        if ! "$runtime" info >/dev/null 2>&1; then
          echo "clickhouse-up: '$runtime' not reachable — is it installed/running?" >&2
          exit 1
        fi

        if "$runtime" ps -a --format '{{.Names}}' | grep -qx "${name}"; then
          require_hardened
          echo "==> container '${name}' already exists; (re)starting it"
          "$runtime" start "${name}" >/dev/null
        else
          echo "==> starting ClickHouse (${image})"
          # Ports are published on 127.0.0.1 only (host-local). Every login needs a
          # password: the admin's hash comes from the rendered override mounted below.
          # The schema is applied by apply_schema (not the image's initdb hook, which
          # would run it as the admin before the passwords exist).
          "$runtime" run -d \
            --name "${name}" \
            -p 127.0.0.1:${httpPort}:8123 \
            -p 127.0.0.1:${nativePort}:9000 \
            -v "${serverOverride}:/etc/clickhouse-server/config.d/agent.xml:ro" \
            -v "${usersOverride}:/etc/clickhouse-server/users.d/99-allow-remote-default.xml:ro" \
            -v "$(clickhouse-creds path server-xml):${credsMount}:ro" \
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

        # Apply the schema idempotently, then set the SQL users' passwords (they are
        # created unable to log in until this runs).
        apply_schema

        cat <<EOF

      ClickHouse is up.
        HTTP:    http://localhost:${httpPort}   (/ping, /play)
        Native:  localhost:${nativePort}        (clickhouse-client --port ${nativePort})
        Database: ${db}   Tables: agent_events, agent_logs, agent_usage

        Logins (passwords in $(dirname "$(clickhouse-creds path admin)"), 0600):
          agent_writer  the agent:  [telemetry] user = "agent_writer"
                                    [telemetry] password_file = "$(clickhouse-creds path writer)"
          agent_reader  tenant-scoped reads:  reader_user / reader_password_file
          agent_viewer  dashboards (HyperDX, Grafana)
          default       admin (schema, access management)

        Query:   nix run .#clickhouse-client -- -q 'SHOW TABLES FROM ${db}'
        Migrate: nix run .#clickhouse-migrate   (re-apply schema after a binary update)
        Stop:    nix run .#clickhouse-down
      EOF
    '';
  };

  # Re-apply the schema to an ALREADY-RUNNING container without the up-flow's
  # create/wait — the redeploy verb for "the binary gained a table, migrate the DB".
  # schema.sql is all `CREATE TABLE IF NOT EXISTS`, so this is idempotent and safe to
  # run any time; it closes the gap where a long-lived container predates a schema
  # addition and the telemetry writer silently drops those rows (the doctor's
  # `clickhouse` probe now flags that drift; this fixes it).
  clickhouse-migrate = pkgs.writeShellApplication {
    name = "clickhouse-migrate";
    runtimeInputs = c.runtimes ++ [ clickhouse-creds ];
    text = ''
      set -euo pipefail
      ${c.pickRuntime}

      if ! "$runtime" ps --format '{{.Names}}' | grep -qx "${name}"; then
        echo "clickhouse-migrate: container '${name}' is not running — start it with" >&2
        echo "  nix run .#clickhouse-up" >&2
        exit 1
      fi
      ${adminPrelude}
      require_hardened
      apply_schema
      echo "==> schema applied (idempotent)"
    '';
  };

  clickhouse-down = c.down {
    name = "clickhouse";
    container = name;
  };

  # `nix run .#clickhouse-client -- <args>` → clickhouse-client inside the
  # container as the admin, e.g. `-- -q 'SELECT count() FROM agent.agent_events'`.
  # (Not the shared c.client: this one must carry the admin password.)
  clickhouse-client = pkgs.writeShellApplication {
    name = "clickhouse-client-wrapper";
    runtimeInputs = c.runtimes ++ [ clickhouse-creds ];
    text = ''
      set -euo pipefail
      ${c.pickRuntime}
      if ! "$runtime" ps --format '{{.Names}}' | grep -qx "${name}"; then
        echo "clickhouse-client: container '${name}' is not running — run 'nix run .#clickhouse-up' first" >&2
        exit 1
      fi
      CLICKHOUSE_PASSWORD="$(cat "$(clickhouse-creds path admin)")"
      export CLICKHOUSE_PASSWORD
      exec "$runtime" exec -i -e CLICKHOUSE_PASSWORD "${name}" clickhouse-client "$@"
    '';
  };
}
