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
      agent.v1.ReviewFleetService
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

    # A file-backed review-fleet roster, so --serve-all advertises the Fleet control
    # plane (review-fleet C3) and the CRUD + token-never-returned roundtrip below can
    # exercise it over the wire. Absent file ⇒ empty roster (a Put populates it).
    [review_fleet]
    store = "file"
    file  = "$work/.agent/review-fleet.json"

    # A file-backed RBAC role-card store, so --serve-all advertises the RoleService
    # control plane (config C1b) and the Put→Get roundtrip below can exercise it over
    # the wire. Absent file ⇒ only the three built-in roles; a Put persists a card.
    [role]
    store = "file"
    file  = "$work/.agent/roles.json"

    # A file-backed forge-card store, so --serve-all advertises the ForgeRegistry
    # control plane (config C36 / D1) and the Put→Get roundtrip below can exercise it
    # over the wire.
    [forge_registry]
    store = "file"
    file  = "$work/.agent/forges.json"

    # A file-backed durable scheduler (config C2c), so --serve-all advertises the
    # Scheduler control plane over the persistent `StoreScheduler` and the
    # Schedule→List roundtrip below can exercise it over the wire. `enabled` wires the
    # seam; jobs only FIRE under `agent --scheduler`, so serving alone starts nothing.
    [scheduler]
    enabled = true
    store   = "file"
    path    = "$work/.agent/scheduler.json"

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

      # ---- Review-fleet control plane: CRUD roundtrip + token-never-returned -----
      # Put a row carrying a `token_ref` REFERENCE, then read it back via Get. The
      # reply must echo the reference and NEVER a resolved secret (there is no secret
      # to resolve — the whole point of the reference). Proves the C3 seam is truly on
      # the wire and its no-token-leak contract holds end to end.
      local put_json='{"id":"smoke","user":"acme","repo":"acme__web","backend":"github","token_ref":"env:SMOKE_GH_TOKEN","poll_secs":300,"enabled":true}'
      if ! grpcurl -d "$put_json" "''${dial[@]}" \
          agent.v1.ReviewFleetService.Put >/dev/null 2>"$work/fleet_put.$transport.err"; then
        echo "CONTRACT[$transport]: ReviewFleetService/Put round-trip failed" >&2
        cat "$work/fleet_put.$transport.err" >&2
        [ "$rc" -lt 2 ] && rc=2
      else
        local got
        got="$(grpcurl -d '{"id":"smoke"}' "''${dial[@]}" \
          agent.v1.ReviewFleetService.Get 2>"$work/fleet_get.$transport.err" || true)"
        if ! echo "$got" | grep -q 'env:SMOKE_GH_TOKEN'; then
          echo "CONTRACT[$transport]: ReviewFleetService/Get did not return the token_ref reference" >&2
          [ "$rc" -lt 2 ] && rc=2
        fi
      fi

      # ---- RBAC role control plane: Put→Get roundtrip (config C1b) --------------
      # Put an operator-defined role card (validated action/resource STRINGS), then
      # read it back via Get. Proves the RoleService seam is truly on the wire and the
      # card round-trips through the shared store. The three built-in roles are never
      # stored; a card may not reuse one of their ids (validated at the seam).
      local role_json='{"id":"smoke_reviewer","actions_on_all":["read"],"pairs":[{"action":"approve","resource_type":"fleet"}]}'
      if ! grpcurl -d "$role_json" "''${dial[@]}" \
          agent.v1.RoleService.Put >/dev/null 2>"$work/role_put.$transport.err"; then
        echo "CONTRACT[$transport]: RoleService/Put round-trip failed" >&2
        cat "$work/role_put.$transport.err" >&2
        [ "$rc" -lt 2 ] && rc=2
      else
        local role_got
        role_got="$(grpcurl -d '{"id":"smoke_reviewer"}' "''${dial[@]}" \
          agent.v1.RoleService.Get 2>"$work/role_get.$transport.err" || true)"
        if ! echo "$role_got" | grep -q 'smoke_reviewer'; then
          echo "CONTRACT[$transport]: RoleService/Get did not return the persisted card" >&2
          [ "$rc" -lt 2 ] && rc=2
        fi
      fi

      # ---- Forge registry control plane: Put→Get roundtrip (config C36 / D1) -----
      # Put a github forge card (kind/base_url/token_ref/repo_encoding), then read it
      # back via Get. Proves the ForgeRegistryService seam is truly on the wire and
      # the card round-trips through the shared store. `token_ref` is a reference
      # (env:), never a raw token; an empty base_url ⇒ the kind's registered default.
      local forge_json='{"id":"smoke_gh","kind":"github","enabled":true,"token_ref":"env:GH_TOKEN","repo_encoding":"owner_name"}'
      if ! grpcurl -d "$forge_json" "''${dial[@]}" \
          agent.v1.ForgeRegistryService.Put >/dev/null 2>"$work/forge_put.$transport.err"; then
        echo "CONTRACT[$transport]: ForgeRegistryService/Put round-trip failed" >&2
        cat "$work/forge_put.$transport.err" >&2
        [ "$rc" -lt 2 ] && rc=2
      else
        local forge_got
        forge_got="$(grpcurl -d '{"id":"smoke_gh"}' "''${dial[@]}" \
          agent.v1.ForgeRegistryService.Get 2>"$work/forge_get.$transport.err" || true)"
        if ! echo "$forge_got" | grep -q 'smoke_gh'; then
          echo "CONTRACT[$transport]: ForgeRegistryService/Get did not return the persisted card" >&2
          [ "$rc" -lt 2 ] && rc=2
        fi
      fi

      # ---- Durable scheduler control plane: Schedule→List roundtrip (config C2c) --
      # Schedule a recurring job, then List it back. Proves the SchedulerService seam
      # is truly on the wire over the durable `StoreScheduler` and the job persists
      # through the shared store. A far-future interval keeps it from ever firing
      # during the smoke (there is no `--scheduler` driver here anyway).
      local sched_json='{"spec":"every 3600s","goal":"smoke recurring goal"}'
      if ! grpcurl -d "$sched_json" "''${dial[@]}" \
          agent.v1.SchedulerService.Schedule >/dev/null 2>"$work/sched_put.$transport.err"; then
        echo "CONTRACT[$transport]: SchedulerService/Schedule round-trip failed" >&2
        cat "$work/sched_put.$transport.err" >&2
        [ "$rc" -lt 2 ] && rc=2
      else
        local sched_got
        sched_got="$(grpcurl -d '{}' "''${dial[@]}" \
          agent.v1.SchedulerService.List 2>"$work/sched_list.$transport.err" || true)"
        if ! echo "$sched_got" | grep -q 'smoke recurring goal'; then
          echo "CONTRACT[$transport]: SchedulerService/List did not return the persisted job" >&2
          [ "$rc" -lt 2 ] && rc=2
        fi
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
