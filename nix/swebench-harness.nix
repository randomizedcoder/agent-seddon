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
#       the score met `SWEBENCH_MIN_RESOLVED`. In MODE=validate, all GOLD patches resolved.
#   1 — HARNESS failure: no model, Docker down, swebench crashed, no predictions/report;
#       or (MODE=validate) a gold patch failed to resolve — the eval ENV is broken.
#   2 — REGRESSION gate: `SWEBENCH_MIN_RESOLVED > 0` and resolved fell below it.
#
# Env knobs:
#   generator (the model the AGENT uses): AGENT_E2E_BASE_URL / _MODEL / _API_KEY,
#     AGENT_E2E_INSECURE_TLS, AGENT_E2E_MAX_TOKENS / _CONTEXT_WINDOW.
#   SWEBENCH_MODE      agent (default) | validate — validate grades the GOLD patches
#                      (no agent) to prove the Docker eval environment is sound.
#   SWEBENCH_DATASET   lite (default) | verified | full | multimodal | multilingual |
#                      smoke (a curated hermetic subset, test/swebench/smoke.txt) |
#                      <raw HF dataset id>
#   SWEBENCH_SPLIT     dataset split (default "test"; e.g. "dev" for a small quick set)
#   SWEBENCH_LIMIT / SWEBENCH_INSTANCE_IDS   scope down for a fast iteration run
#   SWEBENCH_MAX_WORKERS (default 4)   parallel Docker eval workers
#   SWEBENCH_NAMESPACE (default "swebench")   pull prebuilt images; "" builds locally
#   SWEBENCH_RUN_ID (default "local")   names the report + Docker artifacts
#   SWEBENCH_CACHE_DIR (default $XDG_CACHE_HOME/agent-seddon/swebench)   repo-clone cache
#   SWEBENCH_MIN_RESOLVED (default 0)   regression floor (count of resolved instances)
#   SWEBENCH_OUTPUT_DIR   if set, predictions.jsonl + report + logs are copied here to keep
# Extra args are forwarded to `run_evaluation`.
#
# NOTE on the extra datasets: `multimodal` (JS + screenshots) hides its test-split gold, so
# LOCAL grading needs `SWEBENCH_SPLIT=dev`; the agent only sees the text of each task.
# `multilingual` (C/Go/Java/JS/PHP/Ruby/Rust/Python) may lack prebuilt images under the
# `swebench` namespace and then builds locally (slower). See docs/swebench.md.
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
    pkgs.gnugrep
  ];
  text = ''
    set -uo pipefail
  ''
  + harness.contract
  + ''

    MODE="''${SWEBENCH_MODE:-agent}"
    case "$MODE" in
      agent | validate) ;;
      *) echo "FAIL(harness): SWEBENCH_MODE must be 'agent' or 'validate' (got '$MODE')." >&2; exit 1 ;;
    esac

    # ---- generator: the model the AGENT uses to write the fix (refuse, don't skip) ----
    # Only needed in agent mode — validate grades gold patches, no model involved.
    GEN_BASE_URL="''${AGENT_E2E_BASE_URL:-http://localhost:11434/v1}"
    GEN_MODEL="''${AGENT_E2E_MODEL:-llama3.1:latest}"
    GEN_API_KEY="''${AGENT_E2E_API_KEY:-ollama}"
    if [ "$MODE" = agent ]; then
      gopt=(-sf -m 10)
      # Honor a self-signed remote generator (the agent sets insecure_tls to match).
      [ "''${AGENT_E2E_INSECURE_TLS:-0}" = 1 ] && gopt+=(-k)
      if ! curl "''${gopt[@]}" -H "Authorization: Bearer $GEN_API_KEY" "$GEN_BASE_URL/models" >/dev/null 2>&1 \
         && ! curl "''${gopt[@]}" "''${GEN_BASE_URL%/v1}/api/tags" >/dev/null 2>&1; then
        echo "FAIL(harness): no generator model at $GEN_BASE_URL" >&2
        echo "  start one (ollama serve && ollama pull $GEN_MODEL) or set AGENT_E2E_BASE_URL/_MODEL/_API_KEY." >&2
        exit 1
      fi
    fi

    # ---- Docker: SWE-bench grades each instance in a container (refuse, don't skip) ----
    if ! docker info >/dev/null 2>&1; then
      echo "FAIL(harness): Docker is not reachable — SWE-bench evaluation runs each instance" >&2
      echo "  in a container. Start the daemon (and ensure your user can reach it)." >&2
      exit 1
    fi

    # ---- resolve the dataset alias -> HuggingFace id ----
    DATASET="''${SWEBENCH_DATASET:-lite}"
    # `validate` with no explicit scope defaults to the curated hermetic smoke set, so a bare
    # `SWEBENCH_MODE=validate` is a fast, safe env check (not 300 gold patches by accident).
    if [ "$MODE" = validate ] && [ "$DATASET" = lite ] && [ -z "''${SWEBENCH_INSTANCE_IDS:-}" ]; then
      DATASET=smoke
    fi
    SMOKE=0
    case "$DATASET" in
      lite)         HF="princeton-nlp/SWE-bench_Lite" ;;
      verified)     HF="princeton-nlp/SWE-bench_Verified" ;;
      full)         HF="princeton-nlp/SWE-bench" ;;
      multimodal)   HF="SWE-bench/SWE-bench_Multimodal" ;;
      multilingual) HF="swe-bench/SWE-bench_Multilingual" ;;
      smoke)        HF="princeton-nlp/SWE-bench_Lite"; SMOKE=1 ;;
      *)            HF="''${SWEBENCH_DATASET}" ;;
    esac
    SPLIT="''${SWEBENCH_SPLIT:-test}"
    RUN_ID="''${SWEBENCH_RUN_ID:-local}"
    WORKERS="''${SWEBENCH_MAX_WORKERS:-4}"
    NS="''${SWEBENCH_NAMESPACE-swebench}"   # unset -> "swebench"; set-empty -> build locally
    MIN_RESOLVED="''${SWEBENCH_MIN_RESOLVED:-0}"
    MODEL_NAME="agent-seddon"; [ "$MODE" = validate ] && MODEL_NAME="gold"
    CACHE_DIR="''${SWEBENCH_CACHE_DIR:-''${XDG_CACHE_HOME:-$HOME/.cache}/agent-seddon/swebench}"
    mkdir -p "$CACHE_DIR"
    # Pin the HuggingFace cache into OUR cache dir so `datasets.load_dataset` never depends
    # on $HOME/.cache/huggingface being writable (it can be root-owned from a prior Docker
    # run). Respects an operator-set HF_HOME.
    export HF_HOME="''${HF_HOME:-$CACHE_DIR/hf}"
    mkdir -p "$HF_HOME"

    # Scratch for predictions + swebench's per-run logs/report (repos are cached OUTSIDE it).
    work="$(mktemp -d)"
    # shellcheck disable=SC2064
    trap "rm -rf '$work'" EXIT
    cp -r ${../test/swebench}/. "$work/"
    chmod -R u+w "$work"
    cd "$work"

    # The `smoke` alias scopes to a committed hermetic instance list (unless the caller set
    # their own SWEBENCH_INSTANCE_IDS). These instances' tests need no external network, so a
    # correct patch actually resolves — the reliable fast default for a smoke run.
    if [ "$SMOKE" = 1 ] && [ -z "''${SWEBENCH_INSTANCE_IDS:-}" ]; then
      SWEBENCH_INSTANCE_IDS="$(grep -vE '^[[:space:]]*#|^[[:space:]]*$' smoke.txt | tr '\n' ' ')"
      echo "swebench: curated hermetic smoke set — $(echo "$SWEBENCH_INSTANCE_IDS" | wc -w) instance(s)"
    fi
    export SWEBENCH_INSTANCE_IDS="''${SWEBENCH_INSTANCE_IDS:-}"

    echo "swebench: mode=$MODE  dataset=$HF  split=$SPLIT  workers=$WORKERS  run_id=$RUN_ID"

    if [ "$MODE" = validate ]; then
      # ---- validate: grade the GOLD patches (agent-independent env sanity) ----
      echo "swebench: validate — grading GOLD patches (expect every one to resolve) ..."
      PRED="gold"
    else
      # ---- Phase 1: inference — drive the agent, collect predictions ----
      echo "swebench: [1/2] inference — driving the agent over the selected instances ..."
      export SWEBENCH_HF_DATASET="$HF"
      export SWEBENCH_SPLIT="$SPLIT"
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
      PRED="$work/predictions.jsonl"
    fi

    # ---- Evaluation — Docker-grade the patches (gold or the agent's) ----
    echo "swebench: [2/2] evaluation — grading in Docker (this pulls/builds images) ..."
    eval_args=(
      --dataset_name "$HF"
      --split "$SPLIT"
      --predictions_path "$PRED"
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

    resolved=$(jq -r '(.resolved_instances // 0)' "$report" 2>/dev/null || echo 0)
    errs=$(jq -r '(.error_instances // 0)' "$report" 2>/dev/null || echo 0)
    empty=$(jq -r '(.empty_patch_instances // 0)' "$report" 2>/dev/null || echo 0)
    # Denominator = the instances WE actually attempted. swebench's total_instances /
    # submitted_instances count the whole dataset even under --instance_ids or gold
    # predictions, so we derive it ourselves: the scoped id count, else (agent) the number
    # of predictions we wrote, else (full validate) the dataset total.
    if [ -n "''${SWEBENCH_INSTANCE_IDS:-}" ]; then
      denom=$(printf '%s' "''${SWEBENCH_INSTANCE_IDS//,/ }" | wc -w | tr -dc '0-9')
    elif [ "$MODE" = agent ]; then
      denom=$(wc -l < "$work/predictions.jsonl" 2>/dev/null | tr -dc '0-9')
    else
      denom=$(jq -r '(.total_instances // 0)' "$report" 2>/dev/null || echo 0)
    fi
    denom="''${denom:-0}"
    pct=0
    [ "$denom" -gt 0 ] && pct=$(( resolved * 100 / denom ))

    echo ""
    echo "=== swebench summary ($MODE) ===  resolved=$resolved/$denom ($pct%)  empty_patch=$empty  error=$errs"
    echo "  report: $MODEL_NAME.$RUN_ID.json (in the run's scratch dir)"

    if [ -n "''${SWEBENCH_OUTPUT_DIR:-}" ]; then
      mkdir -p "$SWEBENCH_OUTPUT_DIR"
      cp "$report" "$SWEBENCH_OUTPUT_DIR/" 2>/dev/null || true
      [ -f "$work/predictions.jsonl" ] && cp "$work/predictions.jsonl" "$SWEBENCH_OUTPUT_DIR/" 2>/dev/null || true
      # Per-instance agent logs (stdout+stderr) — invaluable when an instance crashed.
      [ -d "$work/logs" ] && cp -r "$work/logs" "$SWEBENCH_OUTPUT_DIR/" 2>/dev/null || true
      echo "  kept report (+ predictions/logs) in $SWEBENCH_OUTPUT_DIR"
    fi

    if [ "$MODE" = validate ]; then
      # Every gold patch MUST resolve; if not, the eval environment (Docker/network/images)
      # is broken — a harness failure, not a model result.
      if [ "$denom" -eq 0 ] || [ "$resolved" -lt "$denom" ]; then
        echo "CONTRACT: validate — only $resolved/$denom gold patches resolved; the eval ENV is broken." >&2
        note_fail 1
      fi
    elif [ "$MIN_RESOLVED" -gt 0 ] && [ "$resolved" -lt "$MIN_RESOLVED" ]; then
      echo "CONTRACT: resolved $resolved < SWEBENCH_MIN_RESOLVED=$MIN_RESOLVED — regression." >&2
      note_fail 2
    fi

    echo ""
    contract_exit "PASS: swebench ($MODE) — resolved $resolved/$denom ($pct%)."
  '';
}
