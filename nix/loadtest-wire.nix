# nix/loadtest-wire.nix
#
# `nix run .#loadtest-wire` — the opt-in REAL-WIRE load tier of the load/overload
# harness (docs/design/loadtest, increment 06). The Rust `loadtest` example drives
# seams in-process with hand-rolled clients; this one starts an actual
# `agent --serve-all` process and hammers it over the network with **ghz**, the
# standard gRPC load generator, via server reflection — then scrapes the server's
# own `/metrics` and prints ghz's client-side numbers next to the server-side
# histogram for the same requests.
#
# It runs the whole sequence over **both TCP and UDS** (like the in-process
# `loadtest` scenarios), one server process per transport in turn — `--serve-all`
# binds a single endpoint and a single `/metrics` port (`:9700`), so the transports
# can't share a process and are exercised sequentially. Set
# `LOADTEST_WIRE_TRANSPORTS="tcp"` (or `"uds"`) to pin one.
#
# The server boot / health-wait / dial / teardown and the exit-code contract are the
# shared nix/lib/{serve-wire,contract}.sh snippets (also used by serve-smoke).
#
# NOT a check: it needs a running server + a socket, which the hermetic check
# sandbox forbids (like `e2e-live`). It needs no model, though — the target seams
# (memory over the file backend, tokenizer over the approx backend) are CPU-only,
# so it runs anywhere the agent binary builds.
#
# Exit codes are split like e2e-live, because the failures have different owners:
#   0 — ghz drove the seams on every transport, the server recorded the same load
#       on /metrics, and an overload burst produced RESOURCE_EXHAUSTED on the wire.
#   1 — HARNESS failure: a server never came up / healthy, ghz errored, or
#       /metrics was unreachable. Our bug.
#   2 — CONTRACT violation: a server served the load but did not shed under
#       overload, or the scraped count did not reflect the requests. The thing the
#       tier exists to catch.
{
  pkgs,
  lib,
  versions,
  agent,
  harness,
}:
pkgs.writeShellApplication {
  name = "loadtest-wire";
  runtimeInputs = [
    agent
    versions.ghz
    versions.grpcurl
    pkgs.curl
    pkgs.jq
    pkgs.coreutils
  ];
  text = ''
    set -uo pipefail

    # Tunables (env-overridable, like e2e-live).
    CONC="''${LOADTEST_WIRE_CONC:-50}"
    REQS="''${LOADTEST_WIRE_REQS:-5000}"
    CAP="''${LOADTEST_WIRE_CAP:-16}"          # server admission cap (max_in_flight)
    OVERLOAD_CONC="''${LOADTEST_WIRE_OVERLOAD_CONC:-200}"
    TRANSPORTS="''${LOADTEST_WIRE_TRANSPORTS:-tcp uds}"
    METRICS_ADDR="127.0.0.1:9700"             # GATEWAY.metrics_port
  ''
  + harness.contract
  + harness.serveWire
  + ''

    # A hermetic, model-free config: light seams only, metrics on, a small in-flight
    # cap so an overload burst is reachable on a laptop.
    cat > "$work/agent.toml" <<EOF
    [agent]
    provider = "openai-compat"
    policy   = "auto-approve"
    working_dir = "$work"

    [provider]
    base_url = "http://127.0.0.1:1/v1"
    model    = "unused-no-model-needed"
    api_key  = "none"

    [memory]
    backend       = "file"
    episodic_path = "$work/.agent/episodic.jsonl"
    semantic_dir  = "$work/.agent/memory"

    [tokenizer]
    backend = "approx"

    [search]
    auto_index = false

    [grpc]
    max_in_flight = $CAP

    [metrics]
    enabled = true
    EOF

    # Scrape a single metric sample sum from /metrics (a specific labelled series).
    metric_sum() { # $1 = an awk regex anchored at the series name
      curl -sf "http://$METRICS_ADDR/metrics" 2>/dev/null \
        | awk -v pat="$1" '$0 ~ pat {v=$NF} END {print (v==""?0:v)}'
    }

    # p99 latency (ms) out of a ghz JSON report.
    ghz_p99_ms() { jq -r '(.latencyDistribution[]? | select(.percentage==99) | .latency) // 0 | ./1e6' "$1"; }

    # Run the full ghz sequence against one transport. Echoes results; returns
    # 0 ok / 1 harness / 2 contract.
    run_transport() {
      local transport="$1"
      dial_for "$transport" || return 1

      echo "loadtest-wire: starting 'agent --serve-all' on $transport ($listen, cap=$CAP) ..."
      start_serve_all "$transport" || return 1
      echo "loadtest-wire: $transport server healthy."

      local rc=0

      # ---- Baseline: Memory/Recall over the wire, correlate with /metrics --------
      local before after ok rps avg_ms p99_ms server_delta
      before="$(metric_sum '^agent_memory_op_seconds_count[{]op="recall"')"
      if ghz --insecure --call agent.v1.Memory.Recall \
          -d '{"text":"load","limit":3}' \
          -c "$CONC" -n "$REQS" --connections 8 \
          -O json -o "$work/recall.$transport.json" "$ghz_target" 2> "$work/ghz.err"; then
        after="$(metric_sum '^agent_memory_op_seconds_count[{]op="recall"')"
        ok="$(jq -r '.statusCodeDistribution.OK // 0' "$work/recall.$transport.json")"
        rps="$(jq -r '.rps | floor' "$work/recall.$transport.json")"
        avg_ms="$(jq -r '.average / 1e6' "$work/recall.$transport.json")"
        p99_ms="$(ghz_p99_ms "$work/recall.$transport.json")"
        server_delta="$(awk -v a="$before" -v b="$after" 'BEGIN{printf "%d", b-a}')"
        echo ""
        echo "# loadtest-wire [$transport] — agent.v1.Memory/Recall (ghz over the real wire)"
        printf 'client (ghz)   ok=%s  rps=%s  avg=%.2fms  p99=%.2fms\n' "$ok" "$rps" "$avg_ms" "$p99_ms"
        printf 'server (/metrics)  agent_memory_op_seconds_count{op=recall} +%s\n' "$server_delta"
        # Contract: every OK ghz saw must be a recall the server recorded (small
        # slack for a sample landing mid-scrape).
        if [ "$ok" -gt 0 ] && [ "$server_delta" -lt "$((ok - CONC))" ]; then
          echo "CONTRACT[$transport]: server recorded +$server_delta recalls but ghz saw $ok OK" >&2
          rc=2
        fi
      else
        echo "FAIL(harness): ghz Recall run failed on $transport" >&2
        cat "$work/ghz.err" >&2
        rc=1
      fi

      # ---- Throughput-only: tokenizer Count (no server histogram) ----------------
      if [ "$rc" -ne 1 ] && ghz --insecure --call agent.v1.TokenizerService.Count \
          -d '{"text":"the quick brown fox jumps over the lazy dog"}' \
          -c "$CONC" -n "$REQS" --connections 8 \
          -O json -o "$work/count.$transport.json" "$ghz_target" 2>/dev/null; then
        printf '# loadtest-wire [%s] — agent.v1.TokenizerService/Count\nclient (ghz)   rps=%s  p99=%.2fms\n' \
          "$transport" "$(jq -r '.rps | floor' "$work/count.$transport.json")" "$(ghz_p99_ms "$work/count.$transport.json")"
      fi

      # ---- Overload: burst past the admission cap, expect RESOURCE_EXHAUSTED ------
      if [ "$rc" -ne 1 ]; then
        echo ""
        echo "loadtest-wire: [$transport] overload burst  (c=$OVERLOAD_CONC vs cap=$CAP) ..."
        # Snapshot the server-side shed counter around the burst, so the delta is
        # this burst's sheds (not any incidental sheds from the baseline runs).
        local shed_before shed_after server_shed shed unav
        shed_before="$(metric_sum '^agent_grpc_overload_shed_total ')"
        ghz --insecure --call agent.v1.Memory.Recall \
          -d '{"text":"load","limit":3}' \
          -c "$OVERLOAD_CONC" -n "$((OVERLOAD_CONC * 20))" --connections 16 \
          -O json -o "$work/overload.$transport.json" "$ghz_target" 2>/dev/null || true
        shed_after="$(metric_sum '^agent_grpc_overload_shed_total ')"
        server_shed="$(awk -v a="$shed_before" -v b="$shed_after" 'BEGIN{printf "%d", b-a}')"
        shed="$(jq -r '.statusCodeDistribution.ResourceExhausted // 0' "$work/overload.$transport.json")"
        unav="$(jq -r '.statusCodeDistribution.Unavailable // 0' "$work/overload.$transport.json")"
        echo "loadtest-wire: [$transport] overload status distribution:"
        jq -r '.statusCodeDistribution | to_entries[] | "  \(.key): \(.value)"' "$work/overload.$transport.json"
        printf 'server (/metrics)  agent_grpc_overload_shed_total +%s   (ghz saw %s ResourceExhausted)\n' \
          "$server_shed" "$shed"
        if [ "$shed" -le 0 ]; then
          echo "CONTRACT[$transport]: overload burst (c=$OVERLOAD_CONC, cap=$CAP) produced no RESOURCE_EXHAUSTED" >&2
          tail -n 20 "$work/server.$transport.log" >&2
          [ "$rc" -lt 2 ] && rc=2
        elif [ "$server_shed" -le 0 ]; then
          echo "CONTRACT[$transport]: ghz saw $shed sheds but agent_grpc_overload_shed_total did not move" >&2
          [ "$rc" -lt 2 ] && rc=2
        else
          echo "loadtest-wire: [$transport] shed $shed request(s) with RESOURCE_EXHAUSTED (+$unav Unavailable); server counter +$server_shed."
        fi
      fi

      # Tear down this transport's server before the next binds :9700 / the port.
      stop_server
      return "$rc"
    }

    for t in $TRANSPORTS; do
      echo ""
      echo "========================= transport: $t ========================="
      run_transport "$t" || note_fail "$?"
    done

    echo ""
    contract_exit "PASS: ghz drove the seams over [$TRANSPORTS], /metrics reflected the load, and the overload burst shed RESOURCE_EXHAUSTED on each transport.
    Point a dashboard at the live server while it runs:  nix run .#prometheus-up  (scrapes :9700)  |  nix run .#grafana-up  (agent-seddon dashboard)"
  '';
}
