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
# NOT a check: it needs a running server + a socket, which the hermetic check
# sandbox forbids (like `e2e-live`). It needs no model, though — the target seams
# (memory over the file backend, tokenizer over the approx backend) are CPU-only,
# so it runs anywhere the agent binary builds.
#
# Exit codes are split like e2e-live, because the failures have different owners:
#   0 — ghz drove the seam, the server recorded the same load on /metrics, and an
#       overload burst produced RESOURCE_EXHAUSTED on the wire.
#   1 — HARNESS failure: the server never came up / healthy, ghz errored, or
#       /metrics was unreachable. Our bug.
#   2 — CONTRACT violation: the server served the load but did not shed under
#       overload, or the scraped count did not reflect the requests. The thing the
#       tier exists to catch.
{
  pkgs,
  lib,
  versions,
  agent,
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
    GRPC_ADDR="127.0.0.1:50100"               # GATEWAY.tcp_port
    METRICS_ADDR="127.0.0.1:9700"             # GATEWAY.metrics_port

    work="$(mktemp -d)"
    srv_pid=""
    cleanup() {
      [ -n "$srv_pid" ] && kill "$srv_pid" 2>/dev/null || true
      rm -rf "$work"
    }
    trap cleanup EXIT

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

    echo "loadtest-wire: starting 'agent --serve-all' (cap=$CAP) ..."
    agent --config "$work/agent.toml" --serve-all > "$work/server.log" 2>&1 &
    srv_pid=$!

    # Wait for grpc.health.v1 to report SERVING (up to ~10s). Refuse rather than
    # race: a load run against a not-yet-bound server is a false failure.
    ready=0
    for _ in $(seq 1 50); do
      if ! kill -0 "$srv_pid" 2>/dev/null; then
        echo "FAIL(harness): server exited during startup" >&2
        tail -n 30 "$work/server.log" >&2
        exit 1
      fi
      if grpcurl -plaintext "$GRPC_ADDR" grpc.health.v1.Health/Check >/dev/null 2>&1; then
        ready=1
        break
      fi
      sleep 0.2
    done
    if [ "$ready" -ne 1 ]; then
      echo "FAIL(harness): server never became healthy at $GRPC_ADDR" >&2
      tail -n 30 "$work/server.log" >&2
      exit 1
    fi
    echo "loadtest-wire: server healthy."

    # Scrape a single metric sample sum from /metrics (a specific labelled series).
    metric_sum() { # $1 = series name incl. labels prefix, matches lines starting with it
      curl -sf "http://$METRICS_ADDR/metrics" 2>/dev/null \
        | awk -v pat="$1" '$0 ~ pat {v=$NF} END {print (v==""?0:v)}'
    }

    # ---- Baseline: drive Memory/Recall over the wire, correlate with /metrics ----
    before="$(metric_sum '^agent_memory_op_seconds_count[{]op="recall"')"
    echo "loadtest-wire: ghz -> agent.v1.Memory/Recall  (c=$CONC, n=$REQS) ..."
    if ! ghz --insecure \
        --call agent.v1.Memory.Recall \
        -d '{"text":"load","limit":3}' \
        -c "$CONC" -n "$REQS" \
        --connections 8 \
        -O json -o "$work/recall.json" \
        "$GRPC_ADDR" 2> "$work/ghz.err"; then
      echo "FAIL(harness): ghz run failed" >&2
      cat "$work/ghz.err" >&2
      exit 1
    fi
    after="$(metric_sum '^agent_memory_op_seconds_count[{]op="recall"')"

    rps="$(jq -r '.rps            | floor' "$work/recall.json")"
    avg_ms="$(jq -r '.average     / 1e6'   "$work/recall.json")"
    p99_ms="$(jq -r '(.latencyDistribution[] | select(.percentage==99) | .latency) // 0 | ./1e6' "$work/recall.json")"
    ok="$(jq -r '.statusCodeDistribution.OK // 0' "$work/recall.json")"
    server_delta="$(awk -v a="$before" -v b="$after" 'BEGIN{printf "%d", b-a}')"

    echo ""
    echo "# loadtest-wire — agent.v1.Memory/Recall (ghz over the real wire)"
    printf 'client (ghz)   ok=%s  rps=%s  avg=%.2fms  p99=%.2fms\n' "$ok" "$rps" "$avg_ms" "$p99_ms"
    printf 'server (/metrics)  agent_memory_op_seconds_count{op=recall} +%s\n' "$server_delta"

    # Contract: every OK ghz saw must be a recall the server recorded (allow a small
    # slack for a sample landing mid-scrape).
    if [ "$ok" -gt 0 ] && [ "$server_delta" -lt "$((ok - CONC))" ]; then
      echo "CONTRACT: server recorded +$server_delta recalls but ghz saw $ok OK" >&2
      exit 2
    fi

    # ---- Throughput-only: tokenizer Count (no server histogram, pure wire perf) ----
    echo ""
    echo "loadtest-wire: ghz -> agent.v1.TokenizerService/Count  (c=$CONC, n=$REQS) ..."
    if ghz --insecure \
        --call agent.v1.TokenizerService.Count \
        -d '{"text":"the quick brown fox jumps over the lazy dog"}' \
        -c "$CONC" -n "$REQS" --connections 8 \
        -O json -o "$work/count.json" "$GRPC_ADDR" 2>/dev/null; then
      trps="$(jq -r '.rps | floor' "$work/count.json")"
      tp99="$(jq -r '(.latencyDistribution[] | select(.percentage==99) | .latency) // 0 | ./1e6' "$work/count.json")"
      printf '# loadtest-wire — agent.v1.TokenizerService/Count\nclient (ghz)   rps=%s  p99=%.2fms\n' "$trps" "$tp99"
    fi

    # ---- Overload: burst past the admission cap, expect RESOURCE_EXHAUSTED ----
    echo ""
    echo "loadtest-wire: overload burst  (c=$OVERLOAD_CONC vs cap=$CAP) ..."
    ghz --insecure \
      --call agent.v1.Memory.Recall \
      -d '{"text":"load","limit":3}' \
      -c "$OVERLOAD_CONC" -n "$((OVERLOAD_CONC * 20))" --connections 16 \
      -O json -o "$work/overload.json" "$GRPC_ADDR" 2>/dev/null || true

    # ghz reports gRPC statuses by name; the admission layer sheds ResourceExhausted.
    shed="$(jq -r '.statusCodeDistribution.ResourceExhausted // 0' "$work/overload.json")"
    unav="$(jq -r '.statusCodeDistribution.Unavailable // 0' "$work/overload.json")"
    echo "loadtest-wire: overload status distribution:"
    jq -r '.statusCodeDistribution | to_entries[] | "  \(.key): \(.value)"' "$work/overload.json"

    if [ "$shed" -le 0 ]; then
      echo "CONTRACT: overload burst (c=$OVERLOAD_CONC, cap=$CAP) produced no RESOURCE_EXHAUSTED" >&2
      echo "  the admission layer must shed under overload — see server.log:" >&2
      tail -n 20 "$work/server.log" >&2
      exit 2
    fi

    echo ""
    echo "PASS: ghz drove the seams over the wire, /metrics reflected the load, and the"
    echo "      overload burst shed $shed request(s) with RESOURCE_EXHAUSTED (+$unav Unavailable)."
    echo ""
    echo "Point a dashboard at the live server while it runs:"
    echo "  nix run .#prometheus-up   # scrapes :9700"
    echo "  nix run .#grafana-up      # agent-seddon dashboard"
  '';
}
