# nix/rest-bench.nix
#
# `rest-bench` — the opt-in artifact behind the "recommend gRPC" guidance
# (gap-analysis §4, docs/design/rest-openapi/): how much latency does the REST/JSON
# compat surface add over native gRPC for the same call?
#
# It boots `agent --serve-all` on loopback TCP (via the shared serve-wire harness),
# brings up the Envoy `grpc_json_transcoder` listener (`grpc-web-up`), then runs the
# SAME logical read two ways against the SAME gateway:
#   * the gRPC leg — `ghz` (reflection) straight at `:50100`;
#   * the REST leg — a concurrent `curl` loop at the transcoder (`:8094`);
# and prints a side-by-side p50/p95 table with the transcoding-overhead delta.
#
# It is NOT a `nix flake check`: throughput is machine-dependent AND it needs a
# live server + a container (the hermetic check sandbox forbids both). So it is
# exposed as an app only, never registered in `checks` or in `nix/integration.nix`.
# It self-skips (exit 0) when no container runtime is reachable, honouring
# $CONTAINER_RUNTIME (default docker) like every container app.
#
# Representative RPC = `ConfigService.GetValues` (empty request → `GET
# /v1/config/values`): a small, side-effect-free read that isolates transcoding
# cost from any real work, and is served on the `--serve-all` gateway because
# `--config` gives the CLI a source_path (the condition that wires the config seam).
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
    # from the process cwd (`.agent/graph.textproto`), so a real checkout that seeds
    # one (e.g. l2's, which references a `glm` provider) would fail the agent build
    # under this model-free config. Fail-hermetic regardless of the cwd's `.agent/`.
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
    # seconds-per-line → ms at a percentile (nearest-rank on the sorted samples).
    curl_pct_ms() {
      sort -n "$1" | awk -v p="$2" '
        {a[NR]=$0}
        END{ if(NR==0){print "0"; exit}
             i=int((p/100)*NR); if(i<1)i=1; if(i>NR)i=NR;
             printf "%.3f", a[i]*1000 }'
    }

    echo "==> rest-bench: gRPC leg — ghz $REQS reqs @ $CONC concurrency"
    if ! ghz --insecure --call agent.v1.ConfigService.GetValues \
        -d '{}' -c "$CONC" -n "$REQS" --connections 8 \
        -O json -o "$work/grpc.json" "$ghz_target" 2> "$work/ghz.err"; then
      echo "FAIL(harness): ghz gRPC leg did not run" >&2
      cat "$work/ghz.err" >&2 || true
      note_fail 1
      contract_exit "done"
    fi
    grpc_ok="$(jq -r '.statusCodeDistribution.OK // 0' "$work/grpc.json")"
    if [ "$grpc_ok" -le 0 ]; then
      echo "FAIL(harness): ghz gRPC leg produced no OK responses" >&2
      note_fail 1
      contract_exit "done"
    fi
    grpc_p50="$(ghz_pct_ms "$work/grpc.json" 50)"
    grpc_p95="$(ghz_pct_ms "$work/grpc.json" 95)"
    grpc_rps="$(jq -r '.rps | floor' "$work/grpc.json")"
    grpc_avg="$(jq -r '.average / 1e6' "$work/grpc.json")"

    echo "==> rest-bench: REST leg — $REQS curl GETs @ $CONC concurrency"
    : > "$work/rest.txt"
    seq "$REQS" | xargs -P "$CONC" -I{} \
      curl -s -o /dev/null -w '%{time_total}\n' "$REST/v1/config/values" >> "$work/rest.txt"
    rest_n="$(grep -c . "$work/rest.txt" || true)"
    if [ "''${rest_n:-0}" -le 0 ]; then
      echo "FAIL(harness): REST leg produced no samples" >&2
      note_fail 1
      contract_exit "done"
    fi
    rest_p50="$(curl_pct_ms "$work/rest.txt" 50)"
    rest_p95="$(curl_pct_ms "$work/rest.txt" 95)"

    # Overhead deltas (REST − gRPC), in ms, two decimals.
    d50="$(awk -v r="$rest_p50" -v g="$grpc_p50" 'BEGIN{printf "%.2f", r-g}')"
    d95="$(awk -v r="$rest_p95" -v g="$grpc_p95" 'BEGIN{printf "%.2f", r-g}')"

    desc_bytes="$(wc -c < "${restDescriptor}/agent_descriptor.pb" | tr -d ' ')"
    svc_n="$(grep -c . "${restDescriptor}/services.txt" || true)"

    echo ""
    echo "################ rest-bench: ConfigService.GetValues ($REQS reqs, $CONC conc) ################"
    printf '  %-8s %10s %10s %10s\n' "leg" "p50(ms)" "p95(ms)" "rps"
    printf '  %-8s %10s %10s %10s\n' "gRPC" "$grpc_p50" "$grpc_p95" "$grpc_rps"
    printf '  %-8s %10s %10s %10s\n' "REST" "$rest_p50" "$rest_p95" "-"
    echo ""
    echo "  transcoding overhead (REST − gRPC):  p50 +''${d50}ms   p95 +''${d95}ms"
    echo "  gRPC average: ''${grpc_avg}ms over $grpc_ok OK responses"
    echo "  descriptor Envoy loads: ''${desc_bytes} bytes, $svc_n services transcoded"
    echo "  → the compat REST surface costs the above per call; prefer gRPC for hot paths."

    stop_server
    contract_exit "PASS: rest-bench — REST vs gRPC latency measured (see the overhead delta above)."
  '';
}
