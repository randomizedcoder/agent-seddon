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
# turn (like `loadtest-wire`), so the seam surface is proven on each transport.
#
# NOT a check: like `e2e-live`/`loadtest-wire` it spawns a server process and dials
# a socket, which agent-seddon keeps out of the hermetic `nix flake check` sandbox.
# It needs no model, though — every probed seam is CPU-only (file/approx backends),
# so it runs anywhere the agent binary builds. Set `SERVE_SMOKE_TRANSPORTS="tcp"`
# (or `"uds"`) to pin one.
#
# Exit codes are split like e2e-live/loadtest-wire, because the failures have
# different owners:
#   0 — every transport came up healthy, every advertised seam described, the
#       critical subset was present, and both round-trips returned.
#   1 — HARNESS failure: a server never came up / healthy, or grpcurl itself errored.
#   2 — CONTRACT violation: a seam failed to describe, the critical subset was
#       incomplete, or a round-trip RPC failed. The thing the tier exists to catch.
{
  pkgs,
  lib,
  versions,
  agent,
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

    work="$(mktemp -d)"
    srv_pid=""
    # shellcheck disable=SC2329  # invoked indirectly via the EXIT trap
    cleanup() {
      [ -n "$srv_pid" ] && kill "$srv_pid" 2>/dev/null || true
      rm -rf "$work"
    }
    trap cleanup EXIT

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

    worst=0                                   # 0 ok, 1 harness, 2 contract (max wins)
    note_fail() { [ "$1" -gt "$worst" ] && worst="$1"; }

    run_transport() {
      local transport="$1" listen
      local -a dial
      case "$transport" in
        tcp)
          listen="127.0.0.1:50100"
          dial=(-plaintext "127.0.0.1:50100")
          ;;
        uds)
          listen="unix:$work/gw.sock"
          # grpcurl dials a UDS via the `unix://` address scheme, not `-unix`.
          dial=(-plaintext "unix://$work/gw.sock")
          ;;
        *)
          echo "unknown transport: $transport" >&2
          return 1
          ;;
      esac

      echo "serve-smoke: starting 'agent --serve-all' on $transport ($listen) ..."
      agent --config "$work/agent.toml" --serve-all --listen "$listen" \
        > "$work/server.$transport.log" 2>&1 &
      srv_pid=$!

      # Wait for grpc.health.v1 SERVING (up to ~10s). Refuse rather than race.
      local ready=0
      for _ in $(seq 1 50); do
        if ! kill -0 "$srv_pid" 2>/dev/null; then
          echo "FAIL(harness): $transport server exited during startup" >&2
          tail -n 30 "$work/server.$transport.log" >&2
          srv_pid=""
          return 1
        fi
        if grpcurl "''${dial[@]}" grpc.health.v1.Health/Check >/dev/null 2>&1; then
          ready=1
          break
        fi
        sleep 0.2
      done
      if [ "$ready" -ne 1 ]; then
        echo "FAIL(harness): $transport server never became healthy" >&2
        tail -n 30 "$work/server.$transport.log" >&2
        kill "$srv_pid" 2>/dev/null || true
        wait "$srv_pid" 2>/dev/null || true
        srv_pid=""
        return 1
      fi
      echo "serve-smoke: [$transport] healthy (grpc.health.v1 SERVING)."

      local rc=0

      # ---- Enumerate the advertised seam surface via reflection ------------------
      local services count
      if ! services="$(grpcurl "''${dial[@]}" list 2>"$work/list.$transport.err")"; then
        echo "FAIL(harness): [$transport] grpcurl list failed" >&2
        cat "$work/list.$transport.err" >&2
        kill "$srv_pid" 2>/dev/null || true; wait "$srv_pid" 2>/dev/null || true; srv_pid=""
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

      kill "$srv_pid" 2>/dev/null || true
      wait "$srv_pid" 2>/dev/null || true
      srv_pid=""
      return "$rc"
    }

    for t in $TRANSPORTS; do
      echo ""
      echo "========================= transport: $t ========================="
      run_transport "$t" || note_fail "$?"
    done

    echo ""
    case "$worst" in
      0) echo "PASS: --serve-all advertised, described, and served every seam over [$TRANSPORTS]." ;;
      2) echo "FAIL(contract): see CONTRACT lines above." >&2 ;;
      *) echo "FAIL(harness): see FAIL lines above." >&2 ;;
    esac
    exit "$worst"
  '';
}
