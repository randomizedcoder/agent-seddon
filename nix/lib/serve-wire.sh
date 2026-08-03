# Real-wire server harness shared by loadtest-wire + serve-smoke: an auto-cleaned
# workdir, an `agent --serve-all` process with a gRPC health-wait, and per-transport
# dial setup over TCP + UDS. Requires `agent` + `grpcurl` on PATH.
#
# Concatenated into a `writeShellApplication` after contract.sh (so shellcheck sees
# it in-context). Sets globals: `work`, `srv_pid`, and — via `dial_for` — `listen`,
# the `dial` grpcurl args, and `ghz_target`.

work="$(mktemp -d)"
srv_pid=""
# shellcheck disable=SC2329  # invoked indirectly via the EXIT trap
_serve_wire_cleanup() {
  [ -n "$srv_pid" ] && kill "$srv_pid" 2>/dev/null || true
  rm -rf "$work"
}
trap _serve_wire_cleanup EXIT

# dial_for TRANSPORT — set `listen`, the `dial` grpcurl args, and `ghz_target` for
# `tcp` or `uds` (returns 1 on an unknown transport). `ghz_target` is consumed by
# loadtest-wire's ghz runs; serve-smoke ignores it.
# shellcheck disable=SC2034
dial_for() {
  case "$1" in
    tcp)
      listen="127.0.0.1:50100"
      ghz_target="127.0.0.1:50100"
      dial=(-plaintext "127.0.0.1:50100")
      ;;
    uds)
      listen="unix:$work/gw.sock"
      ghz_target="unix://$work/gw.sock"
      # grpcurl dials a UDS via the `unix://` address scheme, NOT its `-unix` flag
      # (which expects host:port and errors on a bare path).
      dial=(-plaintext "unix://$work/gw.sock")
      ;;
    *)
      echo "unknown transport: $1" >&2
      return 1
      ;;
  esac
}

# stop_server — kill the current `agent --serve-all` and reset `srv_pid`.
stop_server() {
  [ -n "$srv_pid" ] && kill "$srv_pid" 2>/dev/null || true
  [ -n "$srv_pid" ] && wait "$srv_pid" 2>/dev/null || true
  srv_pid=""
}

# start_serve_all TRANSPORT — boot `agent --serve-all` on `$listen` (config at
# `$work/agent.toml`, `$dial` already set by dial_for), then wait up to ~10s for
# grpc.health.v1 SERVING. Refuses rather than races. 0 = healthy; 1 = never came
# up (a HARNESS failure).
start_serve_all() {
  local transport="$1"
  agent --config "$work/agent.toml" --serve-all --listen "$listen" \
    >"$work/server.$transport.log" 2>&1 &
  srv_pid=$!
  local ready=0
  for _ in $(seq 1 50); do
    if ! kill -0 "$srv_pid" 2>/dev/null; then
      echo "FAIL(harness): $transport server exited during startup" >&2
      tail -n 30 "$work/server.$transport.log" >&2
      srv_pid=""
      return 1
    fi
    if grpcurl "${dial[@]}" grpc.health.v1.Health/Check >/dev/null 2>&1; then
      ready=1
      break
    fi
    sleep 0.2
  done
  if [ "$ready" -ne 1 ]; then
    echo "FAIL(harness): $transport server never became healthy" >&2
    tail -n 30 "$work/server.$transport.log" >&2
    stop_server
    return 1
  fi
  return 0
}
