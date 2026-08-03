# nix/serve-smoke.nix
#
# `nix run .#serve-smoke` — the opt-in REAL-WIRE breadth probe of the gRPC seam
# surface. `loadtest-wire` drives two seams hard for throughput; this drives EVERY
# served seam once for reachability: it starts an actual `agent --serve-all`
# process and, over the network via server reflection, asserts that
#
#   - the overall server reports `grpc.health.v1` SERVING,
#   - every seam the gateway advertises can be `grpcurl describe`d (its schema
#     reflects over the wire — proof it is truly registered, not just compiled),
#   - a CPU-only critical subset is actually present (a seam silently dropped from
#     the gateway fails here), and
#   - two real unary RPCs round-trip (Memory/Recall, TokenizerService/Count).
#
# It runs the whole sequence over **both TCP and UDS**, one server per transport in
# turn (like `loadtest-wire`), so the seam surface is proven on each transport. The
# server boot / health-wait / dial / teardown and the exit-code contract are the
# shared `nix/lib/{serve-wire,contract}.sh` snippets (see loadtest-wire).
#
# NOT a check: like `e2e-live`/`loadtest-wire` it spawns a server process and dials
# a socket, which agent-seddon keeps out of the hermetic `nix flake check` sandbox.
# It needs no model, though — every probed seam is CPU-only (file/approx backends),
# so it runs anywhere the agent binary builds. Set `SERVE_SMOKE_TRANSPORTS="tcp"`
# (or `"uds"`) to pin one.
#
# Exit codes (the shared contract): 0 ok, 1 harness, 2 contract.
{
  pkgs,
  lib,
  versions,
  agent,
  harness,
}:
pkgs.writeShellApplication {
  name = "serve-smoke";
  runtimeInputs = [
    agent
    versions.grpcurl
    pkgs.coreutils
    pkgs.gnugrep
  ];
  text = ''
    set -uo pipefail

    TRANSPORTS="''${SERVE_SMOKE_TRANSPORTS:-tcp uds}"
  ''
  + harness.contract
  + harness.serveWire
  + ''

    # CPU-only seams the default, model-free config always builds — so `--serve-all`
    # must advertise every one of them. A seam dropped from the gateway (e.g. a
    # registration lost in a refactor) fails the contract here. Seams that need a
    # model or external config (Provider stream, LlmPoolService, EmbedService, …) are
    # NOT required — the describe-everything loop still exercises them if present.
    CRITICAL_SERVICES="
      agent.v1.Memory
      agent.v1.ToolService
      agent.v1.ContextService
      agent.v1.Policy
      agent.v1.SearchService
      agent.v1.RepoService
      agent.v1.SessionService
      agent.v1.ScannerService
      agent.v1.ReferenceService
      agent.v1.SchedulerService
      agent.v1.TokenizerService
      agent.v1.TaskService
      agent.v1.Episodic
      agent.v1.Semantic
      agent.v1.ModeService
      agent.v1.PromptService
    "

    # A hermetic, model-free config: file/approx backends only, no metrics port (so
    # nothing binds a fixed port and the two transports can run back to back).
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

    [metrics]
    enabled = false
    EOF

    run_transport() {
      local transport="$1"
      dial_for "$transport" || return 1

      echo "serve-smoke: starting 'agent --serve-all' on $transport ($listen) ..."
      start_serve_all "$transport" || return 1
      echo "serve-smoke: [$transport] healthy (grpc.health.v1 SERVING)."

      local rc=0

      # ---- Enumerate the advertised seam surface via reflection ------------------
      local services count
      if ! services="$(grpcurl "''${dial[@]}" list 2>"$work/list.$transport.err")"; then
        echo "FAIL(harness): [$transport] grpcurl list failed" >&2
        cat "$work/list.$transport.err" >&2
        stop_server
        return 1
      fi
      # Only the agent's own seams (skip grpc.health/grpc.reflection).
      services="$(echo "$services" | grep '^agent\.v1\.' || true)"
      count="$(echo "$services" | grep -c . || true)"
      echo "serve-smoke: [$transport] gateway advertises $count agent.v1 seam service(s):"
      echo "$services"

      # ---- Every advertised seam must describe over the wire ---------------------
      local svc
      for svc in $services; do
        if ! grpcurl "''${dial[@]}" describe "$svc" >/dev/null 2>&1; then
          echo "CONTRACT[$transport]: advertised seam '$svc' failed to describe over reflection" >&2
          [ "$rc" -lt 2 ] && rc=2
        fi
      done

      # ---- The CPU-only critical subset must be present --------------------------
      for svc in $CRITICAL_SERVICES; do
        if ! echo "$services" | grep -qxF "$svc"; then
          echo "CONTRACT[$transport]: expected seam '$svc' is not served by --serve-all" >&2
          [ "$rc" -lt 2 ] && rc=2
        fi
      done

      # ---- Two real unary RPCs must round-trip -----------------------------------
      # `-d` is a flag, so it must precede the address (which `dial` carries).
      if ! grpcurl -d '{"text":"serve-smoke","limit":3}' "''${dial[@]}" \
          agent.v1.Memory.Recall >/dev/null 2>"$work/recall.$transport.err"; then
        echo "CONTRACT[$transport]: agent.v1.Memory/Recall round-trip failed" >&2
        cat "$work/recall.$transport.err" >&2
        [ "$rc" -lt 2 ] && rc=2
      fi
      if ! grpcurl -d '{"text":"the quick brown fox jumps over the lazy dog"}' "''${dial[@]}" \
          agent.v1.TokenizerService.Count >/dev/null 2>"$work/count.$transport.err"; then
        echo "CONTRACT[$transport]: agent.v1.TokenizerService/Count round-trip failed" >&2
        cat "$work/count.$transport.err" >&2
        [ "$rc" -lt 2 ] && rc=2
      fi

      [ "$rc" -eq 0 ] && echo "serve-smoke: [$transport] all seams describe; critical subset present; round-trips OK."

      stop_server
      return "$rc"
    }

    for t in $TRANSPORTS; do
      echo ""
      echo "========================= transport: $t ========================="
      run_transport "$t" || note_fail "$?"
    done

    echo ""
    contract_exit "PASS: --serve-all advertised, described, and served every seam over [$TRANSPORTS]."
  '';
}
