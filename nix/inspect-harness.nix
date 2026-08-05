# `nix run .#inspect` — grade the REAL agent with UK AISI's Inspect AI framework.
#
# A custom SOLVER (test/inspect/agent_solver.py) shells out to `agent --config <toml>
# "<prompt>"` one-shot and hands the agent's `=== ANSWER ===` banner back to Inspect as the
# sample completion, so Inspect grades the WHOLE agent, not a raw model. The default task set
# is our own hermetic, deterministically-graded samples (test/inspect/tasks.py); point
# `INSPECT_TASK` at any `inspect_evals/<name>` benchmark to run a standardized one through the
# same solver. See docs/inspect.md.
#
# NOT a `nix flake check`: it needs a running generator model (the one the agent uses) and a
# network socket — none of which the hermetic sandbox has. Sits beside e2e-live / eval /
# swebench. (Agentic `inspect_evals` tasks additionally need Docker; the default doesn't.)
#
# Exit codes (the shared 0/1/2 contract, nix/lib/contract.sh) — BENCHMARK semantics:
#   0 — ran to completion and produced a valid eval log (the accuracy is the measurement) —
#       or the score met `INSPECT_MIN_SCORE`.
#   1 — HARNESS failure: no generator model, the eval errored, or samples failed to complete.
#   2 — REGRESSION gate: `INSPECT_MIN_SCORE > 0` and the score fell below it (percent).
#
# Env knobs:
#   generator (the model the AGENT uses): AGENT_E2E_BASE_URL / _MODEL / _API_KEY,
#     AGENT_E2E_INSECURE_TLS, AGENT_E2E_MAX_TOKENS / _CONTEXT_WINDOW.
#   judge (only when INSPECT_MODEL_GRADED=1): AGENT_E2E_JUDGE_BASE_URL / _MODEL /
#     _API_KEY_FILE / _INSECURE_TLS — adds a model-graded rubric on top of the deterministic
#     check for our own tasks.
#   INSPECT_TASK        task spec (default "tasks.py" = our hermetic set; e.g.
#                       "inspect_evals/gsm8k" runs a standardized benchmark via our solver).
#   INSPECT_LIMIT       cap samples (e.g. "10" or "10-20").
#   INSPECT_MIN_SCORE   regression floor as a PERCENT 0..100 (default 0 = report only).
#   INSPECT_AGENT_TIMEOUT  per-sample agent wall-clock seconds (default 300; read by the solver).
#   INSPECT_OUTPUT_DIR  if set, the eval logs are copied here to keep.
# Extra args are forwarded to `inspect eval`.
{
  pkgs,
  lib,
  versions,
  harness,
  agent,
}:
let
  # The python that has inspect_ai (the `inspect` CLI + framework) and inspect_evals (the
  # standardized benchmark suite) importable.
  pythonEnv = pkgs.python3.withPackages (_ps: [
    versions.inspect-ai
    versions.inspect-evals
  ]);
in
pkgs.writeShellApplication {
  name = "inspect";
  runtimeInputs = [
    agent
    pythonEnv
    pkgs.git # agent-git tools
    versions.ripgrep # `grep` tool fast path (the agent uses it)
    pkgs.docker # only for agentic inspect_evals sandbox tasks (not the default)
    pkgs.jq
    pkgs.curl
    pkgs.coreutils
    pkgs.gnused
    pkgs.gnugrep
  ];
  text = ''
    set -uo pipefail
  ''
  + harness.contract
  + ''

    # ---- generator: the model the AGENT uses (refuse, don't skip) ----
    GEN_BASE_URL="''${AGENT_E2E_BASE_URL:-http://localhost:11434/v1}"
    GEN_MODEL="''${AGENT_E2E_MODEL:-llama3.1:latest}"
    GEN_API_KEY="''${AGENT_E2E_API_KEY:-ollama}"
    gopt=(-sf -m 10)
    [ "''${AGENT_E2E_INSECURE_TLS:-0}" = 1 ] && gopt+=(-k)
    if ! curl "''${gopt[@]}" -H "Authorization: Bearer $GEN_API_KEY" "$GEN_BASE_URL/models" >/dev/null 2>&1 \
       && ! curl "''${gopt[@]}" "''${GEN_BASE_URL%/v1}/api/tags" >/dev/null 2>&1; then
      echo "FAIL(harness): no generator model at $GEN_BASE_URL" >&2
      echo "  start one (ollama serve && ollama pull $GEN_MODEL) or set AGENT_E2E_BASE_URL/_MODEL/_API_KEY." >&2
      exit 1
    fi

    # ---- optional judge: a model-graded rubric on top of the deterministic check ----
    if [ "''${INSPECT_MODEL_GRADED:-0}" = 1 ]; then
      JUDGE_BASE_URL="''${AGENT_E2E_JUDGE_BASE_URL:-https://213.173.96.56:8000/v1}"
      JUDGE_MODEL="''${AGENT_E2E_JUDGE_MODEL:-/model}"
      JUDGE_KEY_FILE="''${AGENT_E2E_JUDGE_API_KEY_FILE:-$HOME/Downloads/runpod/glm/glm-api-key}"
      JUDGE_INSECURE="''${AGENT_E2E_JUDGE_INSECURE_TLS:-1}"
      if [ ! -r "$JUDGE_KEY_FILE" ]; then
        echo "FAIL(harness): judge key $JUDGE_KEY_FILE unreadable — set AGENT_E2E_JUDGE_API_KEY_FILE." >&2
        exit 1
      fi
      jopt=(-sf -m 20); [ "$JUDGE_INSECURE" = 1 ] && jopt+=(-k)
      if ! curl "''${jopt[@]}" "$JUDGE_BASE_URL/models" -H "Authorization: Bearer $(cat "$JUDGE_KEY_FILE")" >/dev/null 2>&1; then
        echo "FAIL(harness): judge endpoint $JUDGE_BASE_URL unreachable — set AGENT_E2E_JUDGE_BASE_URL." >&2
        exit 1
      fi
      # Inspect's `openai/<model>` grader reads the endpoint + key from these env vars.
      export OPENAI_BASE_URL="$JUDGE_BASE_URL"
      OPENAI_API_KEY="$(cat "$JUDGE_KEY_FILE")"; export OPENAI_API_KEY
      export INSPECT_GRADER_MODEL="openai/$JUDGE_MODEL"
      echo "inspect: model-graded ON — judge $JUDGE_MODEL @ $JUDGE_BASE_URL"
    fi

    export INSPECT_TELEMETRY=""  # no phone-home

    # Copy the inspect kit to a writable scratch dir so `tasks.py`'s `import agent_solver`
    # resolves and the eval logs land somewhere ephemeral.
    work="$(mktemp -d)"
    # shellcheck disable=SC2064
    trap "rm -rf '$work'" EXIT
    cp -r ${../test/inspect}/. "$work/"
    chmod -R u+w "$work"
    cd "$work"

    TASK="''${INSPECT_TASK:-tasks.py}"
    # Our own file-based task carries the agent solver; a standardized `inspect_evals/<name>`
    # (or any registered task name) needs its default solver REPLACED with ours.
    solver_args=()
    case "$TASK" in
      *.py | *.py@*) : ;;                        # local file task — solver baked in
      *) solver_args=(--solver agent_solver.py) ;;
    esac

    eval_args=(
      "$TASK"
      --model mockllm/model            # required by Inspect; IGNORED (our solver never calls it)
      --log-dir "$work/logs"
      --log-format json
      --display none
    )
    [ "''${#solver_args[@]}" -gt 0 ] && eval_args+=("''${solver_args[@]}")
    [ -n "''${INSPECT_LIMIT:-}" ] && eval_args+=(--limit "$INSPECT_LIMIT")

    echo "inspect: generator $GEN_MODEL @ $GEN_BASE_URL   task=$TASK"
    echo "inspect: running the eval (driving the agent per sample) ..."
    set +e
    inspect eval "''${eval_args[@]}" "$@"
    ie_rc=$?
    set -e

    # Single fresh log dir per run; newest json is our eval log.
    # shellcheck disable=SC2012
    logf="$(ls -t "$work/logs"/*.json 2>/dev/null | head -1)"
    if [ -z "$logf" ] || [ ! -s "$logf" ]; then
      echo "FAIL(harness): inspect produced no eval log (exit $ie_rc)." >&2
      note_fail 1
      contract_exit "PASS: inspect"
    fi

    status=$(jq -r '(.status // "error")' "$logf" 2>/dev/null || echo error)
    total=$(jq -r '(.results.total_samples // 0)' "$logf" 2>/dev/null || echo 0)
    completed=$(jq -r '(.results.completed_samples // 0)' "$logf" 2>/dev/null || echo 0)
    # Accuracy of the first scorer (our default is `includes`); percent = accuracy*100.
    pct=$(jq -r '(((.results.scores[0].metrics.accuracy.value // (.results.scores[0].metrics | to_entries[0].value.value) // 0) * 100) | floor)' "$logf" 2>/dev/null || echo 0)
    pct="''${pct:-0}"
    MIN="''${INSPECT_MIN_SCORE:-0}"

    echo ""
    echo "=== inspect summary ===  task=$TASK  status=$status  completed=$completed/$total  score=$pct%"
    echo "  log: $(basename "$logf") (in the run's scratch dir)"

    if [ -n "''${INSPECT_OUTPUT_DIR:-}" ]; then
      mkdir -p "$INSPECT_OUTPUT_DIR"
      cp -r "$work/logs" "$INSPECT_OUTPUT_DIR/" 2>/dev/null || true
      echo "  kept eval logs in $INSPECT_OUTPUT_DIR"
    fi

    if [ "$status" != success ]; then
      echo "CONTRACT: inspect eval status=$status (not success) — the eval errored." >&2
      note_fail 1
    elif [ "$total" -eq 0 ] || [ "$completed" -lt "$total" ]; then
      echo "CONTRACT: only $completed/$total samples completed — solver/eval error." >&2
      note_fail 1
    elif [ "$MIN" -gt 0 ] && [ "$pct" -lt "$MIN" ]; then
      echo "CONTRACT: score $pct% < INSPECT_MIN_SCORE=$MIN% — regression." >&2
      note_fail 2
    fi

    echo ""
    contract_exit "PASS: inspect — task=$TASK score $pct% ($completed/$total)."
  '';
}
