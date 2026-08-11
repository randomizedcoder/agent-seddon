# nix/eval-all.nix
#
# `nix run .#eval-all` — run the whole EVAL/BENCHMARK family in one shot and print a
# comparison table. The companion to `nix run .#integration` (which aggregates the CI/wire
# tier): this aggregates the model-driven eval harnesses that grade the agent, plus the
# SWE-agent comparison baseline.
#
# It orchestrates the existing apps as black boxes — each already prints its own progress and
# returns the shared 0/1/2 contract (nix/lib/contract.sh); no harness logic is re-implemented.
#
#   Tier A — fast, model-only (no Docker): inspect, openai-evals, eval, redteam.
#     eval/redteam additionally need a judge key; skipped-with-notice if it is absent.
#   Tier B — model + Docker (slow): swebench (smoke set), swe-agent (baseline). Auto-skipped
#     if Docker is unreachable. EVAL_ALL_QUICK=1 scopes Tier B to ONE hermetic instance.
#
# Unlike integration (which tolerates a missing model), EVERY eval-all harness needs a model,
# so it REFUSES up front (exit 1) if the generator endpoint is unreachable — fail fast, not 6
# partial failures.
#
# Flags / env: --fast | EVAL_ALL_FAST=1 (skip Tier B); --quick | EVAL_ALL_QUICK=1 (Tier B on
# one instance); EVAL_ALL_OUTPUT_DIR (default a mktemp dir) keeps every harness's log + report.
# Generator/judge selection: the shared AGENT_E2E_* / AGENT_E2E_JUDGE_* env (see docs/eval.md).
#
# Exit codes (the shared contract): 0 all ran clean, 1 a harness couldn't run, 2 a harness
# reported a quality/security/regression failure. A low BENCHMARK score is exit 0 (benchmark
# semantics), so only eval/redteam quality/breach raise it to 2. The table is the real output.
{
  pkgs,
  lib,
  harness,
  inspect,
  openai-evals,
  eval,
  redteam,
  swebench,
  swe-agent,
  graph-arena,
}:
pkgs.writeShellApplication {
  name = "eval-all";
  runtimeInputs = [
    pkgs.curl
    pkgs.coreutils
    pkgs.gnugrep
    pkgs.gnused
    pkgs.docker # `docker info` gate for Tier B
    inspect
    openai-evals
    eval
    redteam
    swebench
    swe-agent
    graph-arena
  ];
  text = ''
    set -uo pipefail

    FAST="''${EVAL_ALL_FAST:-0}"
    QUICK="''${EVAL_ALL_QUICK:-0}"
    for arg in "$@"; do
      case "$arg" in
        --fast) FAST=1 ;;
        --quick) QUICK=1 ;;
        *) echo "eval-all: unknown flag '$arg' (accepts --fast | --quick)" >&2; exit 1 ;;
      esac
    done

    OUT="''${EVAL_ALL_OUTPUT_DIR:-$(mktemp -d)}"
    mkdir -p "$OUT"
    SUMMARY="$OUT/summary.tsv"
    : > "$SUMMARY"

    # Invoke each sub-app by its EXPLICIT store path, not a bare PATH name: `eval` is a shell
    # builtin, so a bare `eval` would run the no-op builtin instead of the promptfoo harness.
    INSPECT_BIN=${inspect}/bin/inspect
    OPENAI_EVALS_BIN=${openai-evals}/bin/openai-evals
    EVAL_BIN=${eval}/bin/eval
    REDTEAM_BIN=${redteam}/bin/redteam
    SWEBENCH_BIN=${swebench}/bin/swebench
    SWE_AGENT_BIN=${swe-agent}/bin/swe-agent
    GRAPH_ARENA_BIN=${graph-arena}/bin/graph-arena
  ''
  + harness.contract
  + ''

    # Is a model endpoint reachable? (honors a self-signed generator via -k) — used both for
    # the up-front refuse and as documentation of what the harnesses need.
    model_reachable() {
      local base="$1"
      [ -n "$base" ] || return 1
      local o=(-sf -m 5)
      [ "''${AGENT_E2E_INSECURE_TLS:-0}" = 1 ] && o+=(-k)
      curl "''${o[@]}" -H "Authorization: Bearer ''${AGENT_E2E_API_KEY:-none}" "$base/models" >/dev/null 2>&1 && return 0
      curl "''${o[@]}" "''${base%/v1}/api/tags" >/dev/null 2>&1
    }

    # Run one harness as a black box: stream + capture its output, fold its 0/1/2 exit into
    # `worst`, and record a summary row (its own `=== … summary … ===` line).
    run_eval() {
      local name="$1"
      shift
      local log="$OUT/$name.log"
      echo ""
      echo "################### eval-all: $name ###################"
      # writeShellApplication runs us under `set -e`; a sub-app's non-zero exit (e.g. a hard
      # refuse) must NOT abort the whole suite — we aggregate via note_fail. Suspend errexit
      # around the black-box call (and the grep, which exits 1 on no-match).
      set +e
      "$@" 2>&1 | tee "$log"
      local rc=''${PIPESTATUS[0]}
      local summary
      summary="$(grep -aE '=== .*[Ss]ummary' "$log" 2>/dev/null | tail -1 | sed -e 's/^=* *//' -e 's/ *=*$//')"
      set -e
      note_fail "$rc"
      [ -n "$summary" ] || summary="(ran; no summary line — see $name.log)"
      printf '%s\t%s\t%s\n' "$name" "$rc" "$summary" >> "$SUMMARY"
    }

    skip_eval() { printf '%s\t%s\t%s\n' "$1" "skip" "$2" >> "$SUMMARY"; echo ""; echo "eval-all: SKIP $1 — $2"; }

    # ---- up-front generator preflight (refuse, don't skip — every harness needs it) ----
    GEN_BASE_URL="''${AGENT_E2E_BASE_URL:-http://localhost:11434/v1}"
    GEN_MODEL="''${AGENT_E2E_MODEL:-llama3.1:latest}"
    if ! model_reachable "$GEN_BASE_URL"; then
      echo "FAIL(harness): no generator model at $GEN_BASE_URL — every eval-all harness needs one." >&2
      echo "  set AGENT_E2E_BASE_URL / _MODEL / _API_KEY (see docs/eval-all.md)." >&2
      exit 1
    fi
    echo "eval-all: generator $GEN_MODEL @ $GEN_BASE_URL   output=$OUT   fast=$FAST quick=$QUICK"

    # ---- Tier A: fast, model-only ----
    # NB: `export` (not a `VAR=val cmd` prefix) — a prefix assignment on a shell FUNCTION
    # call is not propagated to the function's subprocesses.
    export INSPECT_OUTPUT_DIR="$OUT/inspect"
    run_eval inspect "$INSPECT_BIN"
    export OPENAI_EVALS_OUTPUT_DIR="$OUT/openai-evals"
    run_eval openai-evals "$OPENAI_EVALS_BIN"

    JUDGE_KEY="''${AGENT_E2E_JUDGE_API_KEY_FILE:-$HOME/Downloads/runpod/glm/glm-api-key}"
    if [ -r "$JUDGE_KEY" ]; then
      run_eval eval "$EVAL_BIN"
      run_eval redteam "$REDTEAM_BIN"
    else
      skip_eval eval "no judge key (set AGENT_E2E_JUDGE_API_KEY_FILE)"
      skip_eval redteam "no judge key (set AGENT_E2E_JUDGE_API_KEY_FILE)"
    fi

    # graph-arena (cognition-graph 06): a CHEAP baseline-vs-gate A/B slice —
    # one rep, two arms — so eval-all stays tractable; the full 5-arm sweep is
    # `nix run .#graph-arena` directly. Needs the judge (critic + rubric judge).
    if [ -r "$JUDGE_KEY" ]; then
      export ARENA_OUTPUT_DIR="$OUT/graph-arena"
      run_eval graph-arena "$GRAPH_ARENA_BIN" --arms baseline,simple --reps 1
    else
      skip_eval graph-arena "no judge key (set AGENT_E2E_JUDGE_API_KEY_FILE)"
    fi

    # ---- Tier B: model + Docker (slow) ----
    if [ "$FAST" = 1 ]; then
      skip_eval swebench "EVAL_ALL_FAST=1 (Tier B skipped)"
      skip_eval swe-agent "EVAL_ALL_FAST=1 (Tier B skipped)"
    elif ! docker info >/dev/null 2>&1; then
      skip_eval swebench "Docker not reachable"
      skip_eval swe-agent "Docker not reachable"
    else
      # QUICK scopes the Docker tier to the FIRST curated hermetic smoke instance, so a
      # shakedown stays tractable (a full smoke run is minutes-to-hours per instance with a
      # model that fills its iteration budget).
      FIRST_ID="$(grep -vE '^[[:space:]]*#|^[[:space:]]*$' ${../test/swebench/smoke.txt} | head -1 | tr -d '[:space:]')"

      export SWEBENCH_DATASET=smoke
      export SWEBENCH_OUTPUT_DIR="$OUT/swebench"
      [ "$QUICK" = 1 ] && export SWEBENCH_INSTANCE_IDS="$FIRST_ID"
      run_eval swebench "$SWEBENCH_BIN"

      export SWEBENCH_OUTPUT_DIR="$OUT/swe-agent"
      [ "$QUICK" = 1 ] && export SWE_AGENT_INSTANCE_IDS="$FIRST_ID"
      run_eval swe-agent "$SWE_AGENT_BIN"
    fi

    # ---- comparison table ----
    echo ""
    echo "==================== eval-all summary ===================="
    printf '%-14s %-6s %s\n' "HARNESS" "EXIT" "RESULT"
    while IFS="$(printf '\t')" read -r n rc s; do
      printf '%-14s %-6s %s\n' "$n" "$rc" "$s"
    done < "$SUMMARY"
    echo ""
    echo "  artifacts (logs + reports): $OUT"
    echo ""
    contract_exit "PASS: eval-all — all harnesses ran (see the table above)."
  '';
}
