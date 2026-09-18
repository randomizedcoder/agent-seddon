# nix/fleet-redeploy.nix
#
# `nix run .#fleet-redeploy` — a complete, in-order redeploy of the review fleet, so no
# step is forgotten. It exists because a manual redeploy once skipped the ClickHouse
# schema migrate: a long-lived container predated a new telemetry table, and the writer
# then *silently dropped* those rows (the review-analysis-depth live-sweep gotcha). This
# app makes the whole sequence one verb:
#
#   1. migrate  — re-apply the ClickHouse schema (idempotent; `clickhouse-migrate`)
#   2. stop     — stop the previous fleet (tracked via a pidfile)
#   3. serve    — start the freshly-built agent as `--serve-fleet`
#   4. doctor   — poll `agent doctor` until the deps (config/ClickHouse/provider) are healthy
#
# The freshly-built agent is baked in at eval time (`${agent}`), so `nix run` realizes the
# current source before step 1 — that IS the "build" step. All operator-specific paths come
# from env/args and are NEVER hardcoded (this file is committed; the fleet config, git
# credentials, and runtime dir are the operator's).
#
#   $1 | $FLEET_CONFIG          the fleet TOML (required)
#   $FLEET_GIT_CREDENTIALS      git credential-store file — set to enable private clones
#   $FLEET_PIDFILE             pid we track    (default $XDG_RUNTIME_DIR|/tmp / agent-fleet.pid)
#   $FLEET_LOG                 serve log path  (default $XDG_RUNTIME_DIR|/tmp / agent-fleet.log)
#   $FLEET_DOCTOR_RETRIES      doctor poll attempts, 2s apart (default 30 ⇒ up to 60s)
{
  pkgs,
  agent,
  clickhouse-migrate,
}:
pkgs.writeShellApplication {
  name = "fleet-redeploy";
  runtimeInputs = [
    pkgs.coreutils
    clickhouse-migrate
  ];
  text = ''
    # writeShellApplication already sets `set -euo pipefail`.
    agent_bin="${agent}/bin/agent"

    config="''${1:-''${FLEET_CONFIG:-}}"
    if [ -z "$config" ]; then
      echo "fleet-redeploy: no config — pass the fleet TOML as \$1 or set FLEET_CONFIG" >&2
      exit 2
    fi
    if [ ! -f "$config" ]; then
      echo "fleet-redeploy: config not found: $config" >&2
      exit 2
    fi

    runtime_dir="''${XDG_RUNTIME_DIR:-/tmp}"
    pidfile="''${FLEET_PIDFILE:-$runtime_dir/agent-fleet.pid}"
    log="''${FLEET_LOG:-$runtime_dir/agent-fleet.log}"
    retries="''${FLEET_DOCTOR_RETRIES:-30}"

    echo "==> fleet-redeploy: agent=$agent_bin config=$config"

    # 1. Migrate the ClickHouse schema (idempotent CREATE TABLE IF NOT EXISTS) so any new
    #    or changed table exists BEFORE the fleet writes to it — the forgotten step that
    #    silently dropped telemetry. clickhouse-migrate exits non-zero if the container is
    #    not running (then run `nix run .#clickhouse-up`), which aborts us fail-fast.
    echo "==> [1/4] applying ClickHouse schema (clickhouse-migrate)"
    clickhouse-migrate

    # 2. Stop the previous fleet if we are tracking a live one.
    if [ -f "$pidfile" ] && oldpid="$(cat "$pidfile" 2>/dev/null)" && [ -n "$oldpid" ] \
      && kill -0 "$oldpid" 2>/dev/null; then
      echo "==> [2/4] stopping previous fleet (pid $oldpid)"
      kill "$oldpid" 2>/dev/null || true
      for _ in $(seq 1 10); do
        kill -0 "$oldpid" 2>/dev/null || break
        sleep 1
      done
      kill -9 "$oldpid" 2>/dev/null || true
    else
      echo "==> [2/4] no live previous fleet to stop"
    fi

    # 3. Start the freshly-built fleet. Private clones need a git credential helper; enable
    #    it only when the operator points us at a credentials file (never hardcoded).
    echo "==> [3/4] starting fleet (--serve-fleet), log $log"
    if [ -n "''${FLEET_GIT_CREDENTIALS:-}" ]; then
      export GIT_CONFIG_COUNT=1
      export GIT_CONFIG_KEY_0=credential.helper
      export GIT_CONFIG_VALUE_0="store --file=$FLEET_GIT_CREDENTIALS"
    fi
    nohup "$agent_bin" --serve-fleet --config "$config" > "$log" 2>&1 &
    newpid=$!
    echo "$newpid" > "$pidfile"
    echo "    pid $newpid"

    # 4. Poll `agent doctor` (it dials config/ClickHouse/provider) until healthy, or fail
    #    if the serve process dies or doctor never passes.
    echo "==> [4/4] waiting for the fleet deps to pass agent doctor"
    for _ in $(seq 1 "$retries"); do
      if ! kill -0 "$newpid" 2>/dev/null; then
        echo "fleet-redeploy: fleet process exited early — last log lines:" >&2
        tail -n 20 "$log" >&2 || true
        exit 1
      fi
      if "$agent_bin" doctor --config "$config" >/dev/null 2>&1; then
        echo "==> fleet-redeploy OK — doctor healthy (pid $newpid)"
        "$agent_bin" doctor --config "$config" || true
        exit 0
      fi
      sleep 2
    done

    echo "fleet-redeploy: doctor did not pass after $((retries * 2))s — report follows:" >&2
    "$agent_bin" doctor --config "$config" || true
    exit 1
  '';
}
