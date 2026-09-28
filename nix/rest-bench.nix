# nix/rest-bench.nix
#
# `rest-bench` — the opt-in artifact behind the "recommend gRPC" guidance
# (gap-analysis §4, docs/design/rest-openapi/): how much latency does the REST/JSON
# compat surface add over native gRPC for the same call?
#
# It boots `agent --serve-all` on loopback TCP (via the shared serve-wire harness),
# brings up the Envoy `grpc_json_transcoder` listener (`grpc-web-up`), then runs the
# SAME logical read two ways against the SAME gateway:
#   * the gRPC leg — `ghz` (reflection, pooled HTTP/2) straight at `:50100`;
#   * the REST leg — `hey` (pooled keep-alive HTTP) at the transcoder (`:8094`);
# and prints a side-by-side p50/p95 table with the transcoding-overhead delta.
#
# Like-for-like: BOTH legs are pooled. The REST leg used to be a fresh-process,
# fresh-TCP-per-request `curl` loop, which dodged the keep-alive path browsers use —
# so it hid the ~40ms Nagle/delayed-ACK stall that only bit persistent connections and
# canceled out at p50, making transcoding look far cheaper OR costlier than it is. `hey`
# reuses connections just like `ghz --connections 8`, so the delta is real overhead.
#
# It is NOT a `nix flake check`: throughput is machine-dependent AND it needs a
# live server + a container (the hermetic check sandbox forbids both). So it is
# exposed as an app only, never registered in `checks` or in `nix/integration.nix`.
# It self-skips (exit 0) when no container runtime is reachable, honouring
# $CONTAINER_RUNTIME (default docker) like every container app.
#
# Two RPCs, both empty-request side-effect-free reads served on the `--serve-all`
# gateway (`--config` gives the CLI a source_path, the condition that wires the config
# seam), so they isolate transcoding cost from any real work:
#   * `ConfigService.Status`   → `GET /v1/config/status` — the small read (~116 B): the
#     headline "what does the compat surface cost per call" number.
#   * `ConfigService.GetValues` → `GET /v1/config/values` — the large read (~34 KB whole
#     config): a witness that the large-body keep-alive path no longer stalls ~40ms.
#
# Exit codes (the shared 0/1/2 contract): 0 clean or skipped, 1 a harness failure
# (a server/leg never produced data), 2 unused (a bench measures, it does not judge).
{
  pkgs,
  lib,
  versions,
  agent,
  grpc-web-up,
  grpc-web-down,
  harness,
}:
let
  restPort = 8094; # nix/portal/envoy-spec.nix `ports.rest`
  # The same descriptor the transcoder loads — referenced only for the operational
  # note (its size + service count are part of "what REST costs you").
  restDescriptor = import ./rest-descriptor.nix { inherit pkgs versions; };
in
pkgs.writeShellApplication {
  name = "rest-bench";
  runtimeInputs = [
    agent
    grpc-web-up
    grpc-web-down
    versions.ghz
    versions.hey
    versions.grpcurl
    versions.docker
    versions.podman
    pkgs.curl
    pkgs.jq
    pkgs.coreutils
    pkgs.gnugrep
  ];
  text = ''
    set -uo pipefail
  ''
  + harness.contract
  + harness.serveWire
  + ''

    REST="http://127.0.0.1:${toString restPort}"
    REQS="''${REQS:-2000}"
    CONC="''${CONC:-50}"
    export PORTAL_AUTH=off # token-less loopback gateway ⇒ grpc-web-up must not fetch a JWKS

    # Opt-in: skip-with-notice (exit 0) on a bare machine (the REST leg needs the
    # Envoy transcoder container). serve-wire already created `$work` + an EXIT trap.
    runtime="''${CONTAINER_RUNTIME:-docker}"
    if ! "$runtime" info >/dev/null 2>&1; then
      echo "rest-bench: SKIP — container runtime ($runtime) not reachable (the REST leg needs the Envoy transcoder container)."
      contract_exit "PASS: rest-bench skipped (no container runtime)."
    fi

    # Extend serve-wire's cleanup to also drop the transcoder container.
    # shellcheck disable=SC2329  # invoked indirectly via the EXIT trap below.
    rest_bench_cleanup() { grpc-web-down >/dev/null 2>&1 || true; _serve_wire_cleanup; }
    trap rest_bench_cleanup EXIT

    # A hermetic, model-free config (serve-wire boots `--serve-all` from $work/agent.toml).
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

    # Disable the cognition graph: it is default-on and its document is discovered
    # from the process cwd (.agent/graph.textproto), so a real checkout that seeds
    # one (e.g. l2, whose graph references a glm provider) would fail the agent build
    # under this model-free config. Fail-hermetic regardless of the cwd .agent tree.
    [graph]
    store = ""

    [metrics]
    enabled = false
    EOF

    echo "==> rest-bench: starting 'agent --serve-all' (tcp, 127.0.0.1:50100)"
    dial_for tcp || { note_fail 1; contract_exit "done"; }
    start_serve_all tcp || { note_fail 1; contract_exit "done"; }

    echo "==> rest-bench: bringing up the grpc_json_transcoder listener ($REST)"
    if ! grpc-web-up; then
      echo "FAIL(harness): grpc-web bridge (transcoder) did not come up" >&2
      note_fail 1
      contract_exit "done"
    fi
    ready=0
    for _ in $(seq 1 30); do
      if curl -sf -o /dev/null "$REST/v1/config/status"; then ready=1; break; fi
      sleep 1
    done
    if [ "$ready" -ne 1 ]; then
      echo "FAIL(harness): REST listener never answered on $REST" >&2
      note_fail 1
      contract_exit "done"
    fi

    # ns → ms, at a given ghz latency percentile.
    ghz_pct_ms() { jq -r "(.latencyDistribution[]? | select(.percentage==$2) | .latency) // 0 | ./1e6" "$1"; }
    # `hey -o csv`: col 1 is response-time in SECONDS, with a header row to skip. →
    # ms at a percentile (nearest-rank on the sorted samples).
    hey_pct_ms() {
      tail -n +2 "$1" | cut -d, -f1 | sort -n | awk -v p="$2" '
        {a[NR]=$0}
        END{ if(NR==0){print "0"; exit}
             i=int((p/100)*NR); if(i<1)i=1; if(i>NR)i=NR;
             printf "%.3f", a[i]*1000 }'
    }

    # Bench one RPC both ways (pooled ghz vs pooled hey) and print its table.
    # Args: <label> <grpc-method> <rest-path>. Returns non-zero on a harness failure
    # (a leg produced no data) so the caller can fail the contract.
    bench_rpc() {
      rpc_label="$1"; rpc_method="$2"; rpc_path="$3"
      gj="$work/grpc-$rpc_label.json"; hc="$work/rest-$rpc_label.csv"

      echo "==> rest-bench[$rpc_label]: gRPC leg — ghz $REQS reqs @ $CONC conc ($rpc_method)"
      if ! ghz --insecure --call "$rpc_method" \
          -d '{}' -c "$CONC" -n "$REQS" --connections 8 \
          -O json -o "$gj" "$ghz_target" 2> "$work/ghz-$rpc_label.err"; then
        echo "FAIL(harness): ghz gRPC leg did not run ($rpc_label)" >&2
        cat "$work/ghz-$rpc_label.err" >&2 || true
        return 1
      fi
      grpc_ok="$(jq -r '.statusCodeDistribution.OK // 0' "$gj")"
      if [ "$grpc_ok" -le 0 ]; then
        echo "FAIL(harness): ghz gRPC leg produced no OK responses ($rpc_label)" >&2
        return 1
      fi

      echo "==> rest-bench[$rpc_label]: REST leg — hey $REQS reqs @ $CONC conc (pooled, $rpc_path)"
      if ! hey -c "$CONC" -n "$REQS" -o csv "$REST$rpc_path" > "$hc" 2> "$work/hey-$rpc_label.err"; then
        echo "FAIL(harness): hey REST leg did not run ($rpc_label)" >&2
        cat "$work/hey-$rpc_label.err" >&2 || true
        return 1
      fi
      rest_n="$(tail -n +2 "$hc" | grep -c . || true)"
      if [ "''${rest_n:-0}" -le 0 ]; then
        echo "FAIL(harness): REST leg produced no samples ($rpc_label)" >&2
        return 1
      fi

      grpc_p50="$(ghz_pct_ms "$gj" 50)"; grpc_p95="$(ghz_pct_ms "$gj" 95)"
      grpc_rps="$(jq -r '.rps | floor' "$gj")"
      rest_p50="$(hey_pct_ms "$hc" 50)"; rest_p95="$(hey_pct_ms "$hc" 95)"
      # Overhead deltas (REST − gRPC), in ms, two decimals.
      d50="$(awk -v r="$rest_p50" -v g="$grpc_p50" 'BEGIN{printf "%.2f", r-g}')"
      d95="$(awk -v r="$rest_p95" -v g="$grpc_p95" 'BEGIN{printf "%.2f", r-g}')"

      echo ""
      echo "############ rest-bench: $rpc_label ($REQS reqs, $CONC conc, both legs pooled) ############"
      printf '  %-8s %10s %10s %10s\n' "leg" "p50(ms)" "p95(ms)" "rps"
      printf '  %-8s %10s %10s %10s\n' "gRPC" "$grpc_p50" "$grpc_p95" "$grpc_rps"
      printf '  %-8s %10s %10s %10s\n' "REST" "$rest_p50" "$rest_p95" "-"
      echo "  transcoding overhead (REST − gRPC):  p50 +''${d50}ms   p95 +''${d95}ms"
    }

    # Small read = the headline overhead; large read = a witness for the keep-alive path.
    if ! bench_rpc status agent.v1.ConfigService.Status /v1/config/status; then
      note_fail 1
      contract_exit "done"
    fi
    if ! bench_rpc values agent.v1.ConfigService.GetValues /v1/config/values; then
      note_fail 1
      contract_exit "done"
    fi

    desc_bytes="$(wc -c < "${restDescriptor}/agent_descriptor.pb" | tr -d ' ')"
    svc_n="$(grep -c . "${restDescriptor}/services.txt" || true)"

    echo ""
    echo "  descriptor Envoy loads: ''${desc_bytes} bytes, $svc_n services transcoded"
    echo "  → with both legs pooled, transcoding costs ~1-2ms per call; the 'values' large-read"
    echo "    row confirms the keep-alive path no longer stalls. Prefer gRPC for hot paths."

    stop_server
    contract_exit "PASS: rest-bench — REST vs gRPC latency measured (see the overhead delta above)."
  '';
}
