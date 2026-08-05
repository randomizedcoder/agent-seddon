# nix/integration.nix
#
# `nix run .#integration` — run the whole opt-in integration tier in one shot.
# agent-seddon keeps the socket/model-driving harnesses out of `nix flake check`
# (they need a server, a socket, or a real model); this is the single entry point
# that runs them, so "run all the integration tests" is one command.
#
# It orchestrates the existing apps as black boxes (each already prints its own
# progress and returns the shared 0/1/2 exit-code contract), aggregating the worst
# outcome via nix/lib/contract.sh — no harness logic is re-implemented here.
#
#   Model-free tier (always): loadtest, loadtest-loop, loadtest-wire, serve-smoke.
#   Model tier (auto): e2e-live, e2e-expect, e2e-multi — run only when a model is
#     configured AND reachable (AGENT_E2E_BASE_URL), else skipped with a notice, so
#     the aggregate is runnable on a machine with no model.
#
# Flags: --no-model (skip the model tier), --model-only (run only the model tier,
# and run it even if the reachability probe is unset). Env: AGENT_E2E_* (see
# e2e-live/e2e-multi) select + point the model tier.
#
# Exit codes (the shared contract): 0 all clean, 1 a harness failure, 2 a contract
# / model-quality failure.
{
  pkgs,
  lib,
  harness,
  loadtest,
  loadtest-loop,
  loadtest-wire,
  serve-smoke,
  e2e-live,
  e2e-expect,
  e2e-multi,
  eval,
}:
pkgs.writeShellApplication {
  name = "integration";
  runtimeInputs = [
    pkgs.curl
    pkgs.coreutils
    loadtest
    loadtest-loop
    loadtest-wire
    serve-smoke
    e2e-live
    e2e-expect
    e2e-multi
    eval
  ];
  text = ''
    set -uo pipefail

    WITH_MODEL=auto # auto | only | no
    for arg in "$@"; do
      case "$arg" in
        --no-model) WITH_MODEL=no ;;
        --model-only) WITH_MODEL=only ;;
        *) echo "integration: unknown flag '$arg' (accepts --no-model | --model-only)" >&2; exit 1 ;;
      esac
    done
  ''
  + harness.contract
  + ''

    # Run one sub-harness; fold its 0/1/2 exit into `worst`.
    run_step() {
      local label="$1"
      shift
      echo ""
      echo "############### integration: $label ###############"
      "$@"
      note_fail "$?"
    }

    # Is a model endpoint reachable? A cheap GET — used only to decide whether to
    # include the model tier (the e2e apps do their own hard reachability refusal).
    model_reachable() {
      local base="$1"
      [ -n "$base" ] || return 1
      curl -sf -m 5 -H "Authorization: Bearer ''${AGENT_E2E_API_KEY:-none}" \
        "$base/models" >/dev/null 2>&1 && return 0
      curl -sf -m 5 "''${base%/v1}/api/tags" >/dev/null 2>&1
    }

    if [ "$WITH_MODEL" != only ]; then
      echo "integration: model-free tier (no model required) ..."
      run_step "loadtest — overload scenario"      loadtest --scenario overload --cap 4 --concurrency 64 --requests 1000 --require-shed
      run_step "loadtest-loop — full-loop probe"   loadtest-loop --concurrency 8 --runs 128
      run_step "loadtest-wire — real wire (tcp+uds)" loadtest-wire
      run_step "serve-smoke — seam breadth (tcp+uds)" serve-smoke
    fi

    if [ "$WITH_MODEL" != no ]; then
      if [ "$WITH_MODEL" = only ] || model_reachable "''${AGENT_E2E_BASE_URL:-}"; then
        echo ""
        echo "integration: model tier (AGENT_E2E_* endpoint) ..."
        run_step "e2e-live — one-shot real model"    e2e-live
        run_step "e2e-expect — multi-turn REPL"      e2e-expect
        run_step "e2e-multi — concurrent + judge"    e2e-multi
        # `eval` (promptfoo quality) additionally needs a judge key — include it only
        # when that is present, else note the skip (its own run would refuse hard).
        if [ -r "''${AGENT_E2E_JUDGE_API_KEY_FILE:-$HOME/Downloads/runpod/glm/glm-api-key}" ]; then
          run_step "eval — promptfoo coding-task quality" eval
        else
          echo ""
          echo "integration: SKIP eval — no judge key (set AGENT_E2E_JUDGE_API_KEY_FILE)."
        fi
      else
        echo ""
        echo "integration: SKIP model tier — no reachable model at AGENT_E2E_BASE_URL."
        echo "  set AGENT_E2E_BASE_URL / _MODEL / _API_KEY to include e2e-live/e2e-expect/e2e-multi,"
        echo "  or pass --model-only to force the model tier."
      fi
    fi

    echo ""
    contract_exit "PASS: integration suite complete."
  '';
}
