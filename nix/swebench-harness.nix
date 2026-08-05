# `nix run .#swebench` — benchmark the REAL agent on SWE-bench.
#
# Two phases in one scratch dir (docs/swebench.md):
#   1. INFERENCE — `test/swebench/predict.py` drives the agent one-shot over each selected
#      instance's checked-out repo and captures its `git diff` as `model_patch`, writing
#      predictions.jsonl.
#   2. EVALUATION — `python -m swebench.harness.run_evaluation` (swebench pinned in
#      nix/versions.nix) applies each patch + the gold test_patch inside a per-instance
#      Docker image and runs FAIL_TO_PASS / PASS_TO_PASS, emitting a report JSON.
# The metric is `% resolved`; re-run over time to watch it move.
#
# NOT a `nix flake check`: it needs Docker + a running model + network + large disk — none
# of which the hermetic sandbox has. Sits beside e2e-live / eval / redteam.
#
# Exit codes (the shared 0/1/2 contract, nix/lib/contract.sh) — BENCHMARK semantics:
#   0 — the benchmark ran to completion and produced a valid report (the resolved rate is
#       the measurement; a benchmark that measures is a success, whatever the score) — OR
#       the score met `SWEBENCH_MIN_RESOLVED`.
#   1 — HARNESS failure: no model, Docker down, swebench crashed, no predictions/report.
#   2 — REGRESSION gate: `SWEBENCH_MIN_RESOLVED > 0` and resolved fell below it.
#
# Env knobs:
#   generator (the model the AGENT uses): AGENT_E2E_BASE_URL / _MODEL / _API_KEY,
#     AGENT_E2E_INSECURE_TLS, AGENT_E2E_MAX_TOKENS / _CONTEXT_WINDOW.
#   SWEBENCH_DATASET   lite (default) | verified | full | <raw HF dataset id>
#   SWEBENCH_LIMIT / SWEBENCH_INSTANCE_IDS   scope down for a fast iteration run
#   SWEBENCH_MAX_WORKERS (default 4)   parallel Docker eval workers
#   SWEBENCH_NAMESPACE (default "swebench")   pull prebuilt images; "" builds locally
#   SWEBENCH_RUN_ID (default "local")   names the report + Docker artifacts
#   SWEBENCH_CACHE_DIR (default $XDG_CACHE_HOME/agent-seddon/swebench)   repo-clone cache
#   SWEBENCH_MIN_RESOLVED (default 0)   regression floor (count of resolved instances)
#   SWEBENCH_OUTPUT_DIR   if set, predictions.jsonl + report are copied here to keep
# Extra args are forwarded to `run_evaluation`.
{
  pkgs,
  lib,
  versions,
  harness,
  agent,
}:
let
  # The python that has swebench (and its deps: datasets/docker/gitpython/...) importable,
  # so `python -m swebench.harness.run_evaluation` and `predict.py` both run under it.
  pythonEnv = pkgs.python3.withPackages (_ps: [ versions.swebench ]);
in
pkgs.writeShellApplication {
  name = "swebench";
  runtimeInputs = [
    agent
    pythonEnv
    pkgs.docker # the eval harness shells out to `docker`
    pkgs.git # clone/checkout each instance repo for inference
    versions.ripgrep # `grep` tool fast path (the agent uses it)
    pkgs.jq
    pkgs.curl
    pkgs.coreutils
    pkgs.gnused
  ];
  text = ''
    set -uo pipefail
  ''
  + harness.contract
  + ''

    # ---- generator: the model the AGENT uses to write the fix (refuse, don't skip) ----
    GEN_BASE_URL="''${AGENT_E2E_BASE_URL:-http://localhost:11434/v1}"
    GEN_MODEL="''${AGENT_E2E_MODEL:-llama3.1:latest}"
    GEN_API_KEY="''${AGENT_E2E_API_KEY:-ollama}"
    gopt=(-sf -m 10)
    # Honor a self-signed remote generator (the agent sets insecure_tls to match).
    [ "''${AGENT_E2E_INSECURE_TLS:-0}" = 1 ] && gopt+=(-k)
    if ! curl "''${gopt[@]}" -H "Authorization: Bearer $GEN_API_KEY" "$GEN_BASE_URL/models" >/dev/null 2>&1 \
       && ! curl "''${gopt[@]}" "''${GEN_BASE_URL%/v1}/api/tags" >/dev/null 2>&1; then
      echo "FAIL(harness): no generator model at $GEN_BASE_URL" >&2
      echo "  start one (ollama serve && ollama pull $GEN_MODEL) or set AGENT_E2E_BASE_URL/_MODEL/_API_KEY." >&2
      exit 1
    fi

    # ---- Docker: SWE-bench grades each instance in a container (refuse, don't skip) ----
    if ! docker info >/dev/null 2>&1; then
      echo "FAIL(harness): Docker is not reachable — SWE-bench evaluation runs each instance" >&2
      echo "  in a container. Start the daemon (and ensure your user can reach it)." >&2
      exit 1
    fi

    # ---- resolve the dataset alias -> HuggingFace id ----
    case "''${SWEBENCH_DATASET:-lite}" in
      lite)     HF="princeton-nlp/SWE-bench_Lite" ;;
      verified) HF="princeton-nlp/SWE-bench_Verified" ;;
      full)     HF="princeton-nlp/SWE-bench" ;;
      *)        HF="''${SWEBENCH_DATASET}" ;;
    esac
    RUN_ID="''${SWEBENCH_RUN_ID:-local}"
    WORKERS="''${SWEBENCH_MAX_WORKERS:-4}"
    NS="''${SWEBENCH_NAMESPACE-swebench}"   # unset -> "swebench"; set-empty -> build locally
    MIN_RESOLVED="''${SWEBENCH_MIN_RESOLVED:-0}"
    MODEL_NAME="agent-seddon"
    CACHE_DIR="''${SWEBENCH_CACHE_DIR:-''${XDG_CACHE_HOME:-$HOME/.cache}/agent-seddon/swebench}"
    mkdir -p "$CACHE_DIR"
    # Pin the HuggingFace cache into OUR cache dir so `datasets.load_dataset` never depends
    # on $HOME/.cache/huggingface being writable (it can be root-owned from a prior Docker
    # run). Respects an operator-set HF_HOME.
    export HF_HOME="''${HF_HOME:-$CACHE_DIR/hf}"
    mkdir -p "$HF_HOME"

    echo "swebench: dataset $HF   generator $GEN_MODEL @ $GEN_BASE_URL   workers $WORKERS   run_id $RUN_ID"

    # Scratch for predictions + swebench's per-run logs/report (repos are cached OUTSIDE it).
    work="$(mktemp -d)"
    # shellcheck disable=SC2064
    trap "rm -rf '$work'" EXIT
    cp -r ${../test/swebench}/. "$work/"
    chmod -R u+w "$work"
    cd "$work"

    # ---- Phase 1: inference — drive the agent, collect predictions ----
    echo "swebench: [1/2] inference — driving the agent over the selected instances ..."
    export SWEBENCH_HF_DATASET="$HF"
    export SWEBENCH_PREDICTIONS="$work/predictions.jsonl"
    export SWEBENCH_CACHE_DIR="$CACHE_DIR/repos"
    export SWEBENCH_MODEL_NAME="$MODEL_NAME"
    set +e
    python predict.py
    predict_rc=$?
    set -e
    if [ ! -s "$work/predictions.jsonl" ]; then
      echo "FAIL(harness): inference produced no predictions (exit $predict_rc)." >&2
      note_fail 1
      contract_exit "PASS: swebench"
    fi

    # ---- Phase 2: evaluation — Docker-grade the patches ----
    echo "swebench: [2/2] evaluation — grading patches in Docker (this pulls/builds images) ..."
    eval_args=(
      --dataset_name "$HF"
      --predictions_path "$work/predictions.jsonl"
      --run_id "$RUN_ID"
      --max_workers "$WORKERS"
    )
    [ -n "''${NS+x}" ] && eval_args+=(--namespace "$NS")
    if [ -n "''${SWEBENCH_INSTANCE_IDS:-}" ]; then
      # shellcheck disable=SC2206
      ids=(''${SWEBENCH_INSTANCE_IDS//,/ })
      eval_args+=(--instance_ids "''${ids[@]}")
    fi
    set +e
    python -m swebench.harness.run_evaluation "''${eval_args[@]}" "$@"
    eval_rc=$?
    set -e

    # swebench writes `<model>.<run_id>.json` in the CWD.
    report="$work/$MODEL_NAME.$RUN_ID.json"
    if [ ! -s "$report" ]; then
      echo "FAIL(harness): swebench produced no report (exit $eval_rc)." >&2
      note_fail 1
      contract_exit "PASS: swebench"
    fi

    total=$(jq -r '(.total_instances // 0)' "$report" 2>/dev/null || echo 0)
    resolved=$(jq -r '(.resolved_instances // 0)' "$report" 2>/dev/null || echo 0)
    errs=$(jq -r '(.error_instances // 0)' "$report" 2>/dev/null || echo 0)
    empty=$(jq -r '(.empty_patch_instances // 0)' "$report" 2>/dev/null || echo 0)
    pct=0
    [ "$total" -gt 0 ] && pct=$(( resolved * 100 / total ))

    echo ""
    echo "=== swebench summary ===  resolved=$resolved/$total ($pct%)  empty_patch=$empty  error=$errs"
    echo "  report: $MODEL_NAME.$RUN_ID.json (in the run's scratch dir)"

    if [ -n "''${SWEBENCH_OUTPUT_DIR:-}" ]; then
      mkdir -p "$SWEBENCH_OUTPUT_DIR"
      cp "$work/predictions.jsonl" "$report" "$SWEBENCH_OUTPUT_DIR/" 2>/dev/null || true
      # Per-instance agent logs (stdout+stderr) — invaluable when an instance crashed.
      [ -d "$work/logs" ] && cp -r "$work/logs" "$SWEBENCH_OUTPUT_DIR/" 2>/dev/null || true
      echo "  kept predictions.jsonl + report + logs/ in $SWEBENCH_OUTPUT_DIR"
    fi

    if [ "$MIN_RESOLVED" -gt 0 ] && [ "$resolved" -lt "$MIN_RESOLVED" ]; then
      echo "CONTRACT: resolved $resolved < SWEBENCH_MIN_RESOLVED=$MIN_RESOLVED — regression." >&2
      note_fail 2
    fi

    echo ""
    contract_exit "PASS: swebench — resolved $resolved/$total ($pct%)."
  '';
}
