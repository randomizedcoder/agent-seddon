# Real-wire server harness shared by loadtest-wire + serve-smoke: an auto-cleaned
# workdir, an `agent --serve-all` process with a gRPC health-wait, and per-transport
# dial setup over TCP + UDS, and TCP with TLS / mTLS. Requires `agent` + `grpcurl` on
# PATH (and `pki-dev` for the `tls` / `mtls` transports).
#
# Concatenated into a `writeShellApplication` after contract.sh (so shellcheck sees
# it in-context). Sets globals: `work`, `srv_pid`, and — via `dial_for` — `listen`,
# the `dial` grpcurl args, `ghz_target`, `tls_addr` / `tls_ca` (TLS transports only)
# and `server_extra` (config appended to `$work/agent.toml` for this transport).

work="$(mktemp -d)"
srv_pid=""
server_extra=""
tls_addr=""
tls_ca=""
# shellcheck disable=SC2329  # invoked indirectly via the EXIT trap
_serve_wire_cleanup() {
  [ -n "$srv_pid" ] && kill "$srv_pid" 2>/dev/null || true
  rm -rf "$work"
}
trap _serve_wire_cleanup EXIT

# ensure_pki — mint the dev PKI (`pki-dev`, step-cli offline) into `$work/pki` once.
ensure_pki() {
  [ -f "$work/pki/ca/root.crt" ] && return 0
  pki-dev --out "$work/pki" --service agent --service cli >"$work/pki.log" 2>&1 || {
    echo "FAIL(harness): pki-dev could not mint the dev PKI" >&2
    cat "$work/pki.log" >&2
    return 1
  }
}

# dial_for TRANSPORT — set `listen`, the `dial` grpcurl args, and `ghz_target` for
# `tcp`, `uds`, `tls` (server cert) or `mtls` (server cert + required client cert)
# (returns 1 on an unknown transport). `ghz_target` is consumed by loadtest-wire's
# ghz runs, which only drive `tcp` / `uds`; serve-smoke ignores it.
# shellcheck disable=SC2034
dial_for() {
  server_extra=""
  tls_addr=""
  tls_ca=""
  case "$1" in
    tls | mtls)
      ensure_pki || return 1
      local pki="$work/pki" port=50101
      [ "$1" = mtls ] && port=50102
      listen="https://127.0.0.1:$port"
      tls_addr="127.0.0.1:$port"
      tls_ca="$pki/ca/root.crt"
      ghz_target="$tls_addr"
      server_extra="
[grpc.tls]
cert = \"$pki/agent/cert.pem\"
key = \"$pki/agent/key.pem\"
"
      dial=(-cacert "$tls_ca")
      if [ "$1" = mtls ]; then
        server_extra+="client_ca = \"$tls_ca\"
"
        dial+=(-cert "$pki/cli/cert.pem" -key "$pki/cli/key.pem")
      fi
      dial+=("$tls_addr")
      ;;
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
  local config="$work/agent.toml"
  if [ -n "$server_extra" ]; then
    config="$work/agent.$transport.toml"
    { cat "$work/agent.toml"; printf '%s' "$server_extra"; } >"$config"
  fi
  agent --config "$config" --serve-all --listen "$listen" \
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
