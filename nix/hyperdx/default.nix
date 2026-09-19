# nix/hyperdx/default.nix
#
# HyperDX (ClickStack) as three containers — the DECOMPOSED, bring-your-own-ClickHouse
# form — so the whole observability stack shares the SINGLE agent ClickHouse instead of
# the all-in-one image's un-disableable bundled copy. See nix/versions.nix for why.
#
#   MongoDB              app state (users/teams/dashboards + the CH connection defs)
#   OTel collector       OTLP :4317/:4318 in → writes to ClickHouse (native :9000),
#                        auto-creates the otel_* schema, pulls its pipeline from OpAMP
#   HyperDX app          UI :8080 + API :8000 + OpAMP :4320
#
# All three run `--network host` (Linux/l2), so the compose service-DNS names collapse
# to `127.0.0.1` and every hop is on host loopback: the collector writes to the agent
# ClickHouse at `127.0.0.1:${native}`, the app reads it over HTTP `127.0.0.1:${http}`,
# and otel_* lands in DB `default` while `agent.*` stays in DB `agent` — one server, so
# `trace_id` is a same-server (cross-DB) JOIN.
#
# Runtime: honours `CONTAINER_RUNTIME` (default `docker`); on the docker-less l2 box run
# every verb with `CONTAINER_RUNTIME=podman` (rootless).
{
  pkgs,
  lib,
  versions,
}:

let
  appName = versions.hyperdxAppContainerName;
  otelName = versions.hyperdxOtelContainerName;
  mongoName = versions.mongoContainerName;

  # The collector + app images are already registry-qualified (podman on l2 has no
  # unqualified-search list); mongo needs the docker.io/ prefix (a no-op under docker).
  appImage = versions.hyperdxAppImage;
  otelImage = versions.hyperdxCollectorImage;
  mongoImage = "docker.io/${versions.mongoImage}";

  uiPort = toString versions.hyperdxUiPort;
  apiPort = toString versions.hyperdxApiPort;
  opampPort = toString versions.hyperdxOpampPort;
  mongoPort = toString versions.mongoPort;
  otlpGrpc = toString versions.otlpGrpcPort;
  otlpHttp = toString versions.otlpHttpPort;

  chName = versions.clickhouseContainerName;
  chHttp = toString versions.clickhouseHttpPort;
  chNative = toString versions.clickhouseNativePort;
  # otel_* live in the `default` DB (HyperDX's expectation); `agent.*` stays in `agent`
  # on the same server, so the two JOIN on trace_id.
  otelDb = "default";

  mongoVolume = "agent-seddon-hyperdx-mongo-data";

  # Seed the app's ClickHouse connection + base sources so the UI works headlessly
  # (no manual "add a source" clicks). HyperDX applies these ONCE, at first team
  # creation via password registration (not on later restarts, and not for an invited
  # user — hyperdx issue #2921). Built with builtins.toJSON so the quoting is correct
  # by construction; the result has no single quotes / no `${`, so it drops straight
  # into a single-quoted shell `-e VAR='…'`.
  defaultConnections = builtins.toJSON [
    {
      name = "Local ClickHouse";
      host = "http://127.0.0.1:${chHttp}";
      username = "default";
      password = "";
    }
  ];
  # Only the base otel_logs / otel_traces sources — the rollup / materialized-view
  # entries the stock compose ships are DELIBERATELY omitted: with the legacy schema
  # (CREATE_LEGACY_SCHEMA=true, required for ClickHouse 24.8) those tables are never
  # created and a source pointing at them errors.
  defaultSources = builtins.toJSON [
    {
      name = "Logs";
      kind = "log";
      connection = "Local ClickHouse";
      from = {
        databaseName = otelDb;
        tableName = "otel_logs";
      };
      timestampValueExpression = "Timestamp";
      displayedTimestampValueExpression = "Timestamp";
      implicitColumnExpression = "Body";
      serviceNameExpression = "ServiceName";
      bodyExpression = "Body";
      eventAttributesExpression = "LogAttributes";
      resourceAttributesExpression = "ResourceAttributes";
      defaultTableSelectExpression = "Timestamp,ServiceName,SeverityText,Body";
      severityTextExpression = "SeverityText";
      traceIdExpression = "TraceId";
      spanIdExpression = "SpanId";
      traceSourceId = "Traces";
    }
    {
      name = "Traces";
      kind = "trace";
      connection = "Local ClickHouse";
      from = {
        databaseName = otelDb;
        tableName = "otel_traces";
      };
      timestampValueExpression = "Timestamp";
      displayedTimestampValueExpression = "Timestamp";
      implicitColumnExpression = "SpanName";
      serviceNameExpression = "ServiceName";
      eventAttributesExpression = "SpanAttributes";
      resourceAttributesExpression = "ResourceAttributes";
      defaultTableSelectExpression = "Timestamp,ServiceName,StatusCode,round(Duration/1e6),SpanName";
      traceIdExpression = "TraceId";
      spanIdExpression = "SpanId";
      durationExpression = "Duration";
      durationPrecision = 9;
      parentSpanIdExpression = "ParentSpanId";
      spanNameExpression = "SpanName";
      spanKindExpression = "SpanKind";
      statusCodeExpression = "StatusCode";
      statusMessageExpression = "StatusMessage";
      logSourceId = "Logs";
    }
  ];

  c = import ../lib/mk-container-app.nix { inherit pkgs versions; };
in
{
  hyperdx-up = pkgs.writeShellApplication {
    name = "hyperdx-up";
    runtimeInputs = c.runtimes ++ [ versions.curl ];
    text = ''
      set -euo pipefail
      ${c.pickRuntime}

      if ! "$runtime" info >/dev/null 2>&1; then
        echo "hyperdx-up: '$runtime' not reachable — is it installed/running?" >&2
        exit 1
      fi

      # The collector writes into the SINGLE agent ClickHouse, so it must be up first.
      if ! "$runtime" ps --format '{{.Names}}' | grep -qx "${chName}"; then
        echo "hyperdx-up: the agent ClickHouse ('${chName}') is not running." >&2
        echo "  Start it first:  nix run .#clickhouse-up" >&2
        exit 1
      fi

      # LAN use: HyperDX's session cookie binds to this URL, so a browser on another
      # box needs HYPERDX_FRONTEND_URL=http://<lan-ip>:${uiPort} (else login bounces).
      frontend_url="''${HYPERDX_FRONTEND_URL:-http://localhost:${uiPort}}"

      # Wait for a plain TCP listener (image-agnostic; bash /dev/tcp).
      wait_tcp() { # port label
        local p="$1" lbl="$2"
        echo -n "==> waiting for $lbl (127.0.0.1:$p)"
        for _ in $(seq 1 120); do
          if timeout 1 bash -c "echo > /dev/tcp/127.0.0.1/$p" 2>/dev/null; then
            echo " ready"
            return 0
          fi
          echo -n "."
          sleep 1
        done
        echo " TIMEOUT" >&2
        return 1
      }

      # --- 1. MongoDB (app state) ----------------------------------------------
      if "$runtime" ps -a --format '{{.Names}}' | grep -qx "${mongoName}"; then
        echo "==> container '${mongoName}' already exists; (re)starting it"
        "$runtime" start "${mongoName}" >/dev/null
      else
        echo "==> starting MongoDB (${mongoImage})"
        "$runtime" run -d \
          --name "${mongoName}" \
          --network host \
          -v "${mongoVolume}:/data/db" \
          "${mongoImage}" >/dev/null
      fi
      wait_tcp ${mongoPort} "MongoDB"

      # --- 2. HyperDX app (UI/API/OpAMP; needs Mongo) --------------------------
      # DEFAULT_CONNECTIONS/DEFAULT_SOURCES seed the CH data source over HTTP :${chHttp}.
      if "$runtime" ps -a --format '{{.Names}}' | grep -qx "${appName}"; then
        echo "==> container '${appName}' already exists; (re)starting it"
        "$runtime" start "${appName}" >/dev/null
      else
        echo "==> starting HyperDX app (${appImage})"
        "$runtime" run -d \
          --name "${appName}" \
          --network host \
          -e "MONGO_URI=mongodb://127.0.0.1:${mongoPort}/hyperdx" \
          -e "FRONTEND_URL=$frontend_url" \
          -e "HYPERDX_APP_URL=http://localhost" \
          -e "HYPERDX_APP_PORT=${uiPort}" \
          -e "HYPERDX_API_PORT=${apiPort}" \
          -e "OPAMP_PORT=${opampPort}" \
          -e "OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:${otlpHttp}" \
          -e "OTEL_SERVICE_NAME=hdx-oss-app" \
          -e "USAGE_STATS_ENABLED=false" \
          -e 'DEFAULT_CONNECTIONS=${defaultConnections}' \
          -e 'DEFAULT_SOURCES=${defaultSources}' \
          "${appImage}" >/dev/null
      fi
      echo -n "==> waiting for the HyperDX UI to come up (Node app; can take ~30-60s)"
      for _ in $(seq 1 120); do
        if curl -sf -o /dev/null "http://localhost:${uiPort}" 2>/dev/null; then
          echo " ready"
          break
        fi
        echo -n "."
        sleep 1
      done
      wait_tcp ${opampPort} "HyperDX OpAMP"

      # --- 3. OTel collector (OTLP in → agent ClickHouse; pipeline via OpAMP) ---
      # Writes over NATIVE :${chNative}; CREATE_LEGACY_SCHEMA keeps the schema on the
      # stable MergeTree form ClickHouse 24.8 supports (NOT the CH-25.x JSON schema).
      if "$runtime" ps -a --format '{{.Names}}' | grep -qx "${otelName}"; then
        echo "==> container '${otelName}' already exists; (re)starting it"
        "$runtime" start "${otelName}" >/dev/null
      else
        echo "==> starting the HyperDX OTel collector (${otelImage})"
        "$runtime" run -d \
          --name "${otelName}" \
          --network host \
          -e "CLICKHOUSE_ENDPOINT=tcp://127.0.0.1:${chNative}?dial_timeout=10s" \
          -e "CLICKHOUSE_USER=default" \
          -e "CLICKHOUSE_PASSWORD=" \
          -e "HYPERDX_OTEL_EXPORTER_CLICKHOUSE_DATABASE=${otelDb}" \
          -e "HYPERDX_OTEL_EXPORTER_CREATE_LEGACY_SCHEMA=true" \
          -e "OPAMP_SERVER_URL=http://127.0.0.1:${opampPort}" \
          -e "HYPERDX_LOG_LEVEL=info" \
          "${otelImage}" >/dev/null
      fi
      # Best-effort: the collector runs a `nop` pipeline and does NOT bind :${otlpGrpc}
      # until the app is onboarded (a HyperDX account exists) and pushes the real
      # pipeline via OpAMP — exactly the all-in-one's behaviour. So a first-boot
      # timeout here is EXPECTED, not a failure: warn and carry on.
      if ! wait_tcp ${otlpGrpc} "OTLP gRPC receiver"; then
        echo "==> [note] the collector has not bound :${otlpGrpc} yet — this is normal before"
        echo "    onboarding. Create the HyperDX account (below); it activates on its own."
      fi

      cat <<EOF

      HyperDX (decomposed) is up — one ClickHouse, three HyperDX containers.
        UI:        http://localhost:${uiPort}
        OTLP gRPC: localhost:${otlpGrpc}
        OTLP HTTP: localhost:${otlpHttp}
        Storage:   the single agent ClickHouse ('${chName}') — otel_* in DB '${otelDb}', agent.* in DB 'agent'

      To start ingesting traces:
        1. Open the UI and create a local account with a PASSWORD (this both fires the
           seeded ClickHouse source AND activates the collector's pipeline — an invited
           user does NOT; hyperdx issue #2921).
        2. The collector's OTLP receiver requires that team's INGESTION KEY as a bearer
           token. Copy it from the UI (Team Settings → API Keys) into the agent's config:
             [telemetry] otlp_endpoint = "http://localhost:${otlpGrpc}"
             [telemetry] otlp_headers  = "authorization=<ingestion-key>"
           (unauthenticated exports are rejected with UNAUTHENTICATED, silently dropping spans).

        Traces:  nix run .#clickhouse-client -- -q 'SELECT count() FROM ${otelDb}.otel_traces'
        Logs:    nix run .#hyperdx-logs            (app; -- otel | mongo for the others)
        Stop:    nix run .#hyperdx-down            (-- --volumes also drops Mongo's data)
      EOF
    '';
  };

  # Remove all three containers (data in the writable layers is discarded).
  # `hyperdx-down -- --volumes` also removes Mongo's named volume (users/teams/etc).
  hyperdx-down = pkgs.writeShellApplication {
    name = "hyperdx-down";
    runtimeInputs = c.runtimes;
    text = ''
      set -euo pipefail
      ${c.pickRuntime}
      for ct in "${otelName}" "${appName}" "${mongoName}"; do
        if "$runtime" ps -a --format '{{.Names}}' | grep -qx "$ct"; then
          echo "==> removing container '$ct'"
          "$runtime" rm -f "$ct" >/dev/null
        else
          echo "container '$ct' not found — nothing to do"
        fi
      done
      if [ "''${1:-}" = "--volumes" ]; then
        echo "==> removing Mongo volume '${mongoVolume}' (app state discarded)"
        "$runtime" volume rm "${mongoVolume}" >/dev/null 2>&1 || true
      fi
    '';
  };

  # Follow one component's logs: `hyperdx-logs` (app) | `hyperdx-logs -- otel|mongo`.
  hyperdx-logs = pkgs.writeShellApplication {
    name = "hyperdx-logs";
    runtimeInputs = c.runtimes;
    text = ''
      set -euo pipefail
      ${c.pickRuntime}
      case "''${1:-app}" in
        app) ct="${appName}" ;;
        otel | collector) ct="${otelName}" ;;
        mongo | db) ct="${mongoName}" ;;
        *)
          echo "hyperdx-logs: unknown component '$1' (use: app | otel | mongo)" >&2
          exit 2
          ;;
      esac
      exec "$runtime" logs -f "$ct"
    '';
  };
}
