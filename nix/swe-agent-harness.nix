# `nix run .#swe-agent` — a COMPARISON BASELINE, not an eval of our agent.
#
# SWE-agent is a DIFFERENT agent scaffold, so it can't "run our agent". Instead this runs
# SWE-agent with the SAME model (AGENT_E2E_*) on the SAME SWE-bench Lite instances, then grades
# its patches with the EXACT swebench Docker harness `nix run .#swebench` uses — so the
# resolved% is directly comparable to our agent's. See docs/swe-agent.md.
#
# Two phases in one scratch dir:
#   1. INFERENCE — `sweagent run-batch --instances.type swe_bench --instances.subset lite`
#      with `--agent.model.*` pointed at our generator; merges patches to preds.json.
#   2. EVALUATION — `python -m swebench.harness.run_evaluation --predictions_path preds.json`
#      (the same grader as swebench), scoped to the same instance ids.
#
# NOT a `nix flake check`: needs Docker (both SWE-agent's SWE-ReX sandbox AND the swebench
# grader) + a model + network + large disk. Sits beside swebench / eval / inspect.
#
# Exit codes (the shared 0/1/2 contract) — BENCHMARK semantics:
#   0 — ran to completion and produced a report (resolved% is the measurement) — or the score
#       met `SWE_AGENT_MIN_RESOLVED`.
#   1 — HARNESS failure: no generator model, Docker down, sweagent/swebench errored, no preds.
#   2 — REGRESSION gate: `SWE_AGENT_MIN_RESOLVED > 0` and resolved fell below it.
#
# Env knobs:
#   generator (the model SWE-agent uses — the SAME one our agent uses, for a fair comparison):
#     AGENT_E2E_BASE_URL / _MODEL / _API_KEY, AGENT_E2E_INSECURE_TLS.
#   SWE_AGENT_INSTANCE_IDS   space/comma ids to run (default: the curated hermetic smoke set,
#                            test/swebench/smoke.txt — the same fast set swebench's `smoke` uses).
#   SWEBENCH_SPLIT (default test) / SWEBENCH_NAMESPACE (default swebench; ""=build local) /
#     SWEBENCH_MAX_WORKERS (default 4) / SWEBENCH_RUN_ID (default swe-agent-baseline).
#   SWE_AGENT_CALL_LIMIT     per-instance model call cap (default 0 = unlimited).
#   SWE_AGENT_CONFIG         SWE-agent run config (default: the shipped simple DefaultAgentConfig
#                            `config/default.yaml`; without a --config, run-batch defaults to a
#                            RetryAgentConfig the `--agent.model.*` overrides don't fit).
#   SWE_AGENT_MIN_RESOLVED   regression floor (count). SWEBENCH_OUTPUT_DIR keeps preds+report.
# Extra args are forwarded to `sweagent run-batch`.
{
  pkgs,
  lib,
  versions,
  harness,
}:
let
  # One python with BOTH SWE-agent (inference) and swebench (grading) importable.
  pythonEnv = pkgs.python3.withPackages (_ps: [
    versions.swe-agent
    versions.swebench
  ]);
in
pkgs.writeShellApplication {
  name = "swe-agent";
  runtimeInputs = [
    pythonEnv
    pkgs.docker # SWE-ReX sandbox (inference) + swebench grading (evaluation)
    pkgs.git
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

    # ---- generator: the model SWE-agent uses (the SAME as our agent — refuse, don't skip) ----
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

    # ---- Docker: SWE-ReX sandbox + swebench grading both containerize (refuse, don't skip) ----
    if ! docker info >/dev/null 2>&1; then
      echo "FAIL(harness): Docker is not reachable — SWE-agent runs each instance in a" >&2
      echo "  SWE-ReX sandbox and swebench grades in a container. Start the daemon." >&2
      exit 1
    fi

    SPLIT="''${SWEBENCH_SPLIT:-test}"
    RUN_ID="''${SWEBENCH_RUN_ID:-swe-agent-baseline}"
    WORKERS="''${SWEBENCH_MAX_WORKERS:-4}"
    NS="''${SWEBENCH_NAMESPACE-swebench}"
    MIN_RESOLVED="''${SWE_AGENT_MIN_RESOLVED:-0}"
    CALL_LIMIT="''${SWE_AGENT_CALL_LIMIT:-0}"
    CACHE_DIR="''${SWEBENCH_CACHE_DIR:-''${XDG_CACHE_HOME:-$HOME/.cache}/agent-seddon/swebench}"
    mkdir -p "$CACHE_DIR"
    export HF_HOME="''${HF_HOME:-$CACHE_DIR/hf}"
    mkdir -p "$HF_HOME"

    # Default scope = the curated hermetic smoke ids (the SAME set swebench's `smoke` uses),
    # so a SWE-agent-vs-our-agent comparison is fast and fair. Overridable.
    IDS="''${SWE_AGENT_INSTANCE_IDS:-}"
    if [ -z "$IDS" ]; then
      IDS="$(grep -vE '^[[:space:]]*#|^[[:space:]]*$' ${../test/swebench/smoke.txt} | tr '\n' ' ')"
    fi
    IDS="$(printf '%s' "''${IDS//,/ }" | tr -s ' ')"
    # shellcheck disable=SC2206
    id_arr=($IDS)
    denom="''${#id_arr[@]}"
    if [ "$denom" -eq 0 ]; then
      echo "FAIL(harness): no instance ids to run." >&2
      exit 1
    fi
    # SWE-agent selects instances with a regex over instance_id; anchor an alternation.
    FILTER="^($(printf '%s' "$IDS" | tr ' ' '|'))$"

    work="$(mktemp -d)"
    # shellcheck disable=SC2064
    trap "rm -rf '$work'" EXIT
    cd "$work"

    # litellm routes an OpenAI-compatible endpoint via the `openai/<model>` provider prefix.
    LLM_NAME="openai/$GEN_MODEL"
    # Honor a self-signed generator (best-effort; litellm reads SSL_VERIFY).
    [ "''${AGENT_E2E_INSECURE_TLS:-0}" = 1 ] && export SSL_VERIFY="False"

    # SWE-ReX zips the shipped tool bundles to upload into the sandbox, and Python's zipfile
    # rejects the 1970 mtimes nix-store files carry ("ZIP does not support timestamps before
    # 1980"). Stage a WRITABLE copy of the shipped config+tools with fresh mtimes and point the
    # SWE-agent path env at it. `cp` gives current mtimes; `touch` is belt-and-suspenders.
    cfg_root="$work/swe-agent-cfg"
    cp -rL ${versions.swe-agent}/share/swe-agent "$cfg_root"
    chmod -R u+w "$cfg_root"
    find "$cfg_root" -exec touch {} +
    export SWE_AGENT_CONFIG_ROOT="$cfg_root"
    export SWE_AGENT_TOOLS_DIR="$cfg_root/tools"

    # Without a `--config`, `run-batch` defaults to a RetryAgentConfig (which wants
    # `agent_configs`/`retry_loop`), so the top-level `--agent.model.*` overrides don't fit its
    # schema (pydantic extra_forbidden). Point it at a shipped SIMPLE DefaultAgentConfig — its
    # `agent.model` is what our `--agent.model.*` flags override. Operator-overridable.
    RUN_CONFIG="''${SWE_AGENT_CONFIG:-$cfg_root/config/default.yaml}"

    echo "swe-agent[baseline]: model $LLM_NAME @ $GEN_BASE_URL   instances=$denom  split=$SPLIT"
    echo "swe-agent[baseline]: config $RUN_CONFIG"
    echo "swe-agent[baseline]: [1/2] inference — SWE-agent scaffold over the instances ..."
    set +e
    sweagent run-batch \
      --config "$RUN_CONFIG" \
      --instances.type swe_bench \
      --instances.subset lite \
      --instances.split "$SPLIT" \
      --instances.filter "$FILTER" \
      --agent.model.name "$LLM_NAME" \
      --agent.model.api_base "$GEN_BASE_URL" \
      --agent.model.api_key "$GEN_API_KEY" \
      --agent.model.per_instance_cost_limit 0 \
      --agent.model.total_cost_limit 0 \
      --agent.model.per_instance_call_limit "$CALL_LIMIT" \
      --num_workers "$WORKERS" \
      --output_dir "$work/traj" "$@"
    sa_rc=$?
    set -e

    preds="$work/traj/preds.json"
    if [ ! -s "$preds" ]; then
      echo "FAIL(harness): SWE-agent produced no predictions (exit $sa_rc)." >&2
      note_fail 1
      contract_exit "PASS: swe-agent"
    fi

    echo "swe-agent[baseline]: [2/2] evaluation — grading with the swebench Docker harness ..."
    eval_args=(
      --dataset_name "princeton-nlp/SWE-bench_Lite"
      --split "$SPLIT"
      --predictions_path "$preds"
      --run_id "$RUN_ID"
      --max_workers "$WORKERS"
      --instance_ids "''${id_arr[@]}"
    )
    [ -n "''${NS+x}" ] && eval_args+=(--namespace "$NS")
    set +e
    python -m swebench.harness.run_evaluation "''${eval_args[@]}"
    eval_rc=$?
    set -e

    # swebench writes `<model_name_or_path>.<run_id>.json`; the model name is SWE-agent's, so
    # glob rather than guess it.
    # shellcheck disable=SC2012
    report="$(ls -t "$work"/*."$RUN_ID".json 2>/dev/null | head -1)"
    if [ -z "$report" ] || [ ! -s "$report" ]; then
      echo "FAIL(harness): swebench produced no report (exit $eval_rc)." >&2
      note_fail 1
      contract_exit "PASS: swe-agent"
    fi

    resolved=$(jq -r '(.resolved_instances // 0)' "$report" 2>/dev/null || echo 0)
    errs=$(jq -r '(.error_instances // 0)' "$report" 2>/dev/null || echo 0)
    empty=$(jq -r '(.empty_patch_instances // 0)' "$report" 2>/dev/null || echo 0)
    pct=0
    [ "$denom" -gt 0 ] && pct=$(( resolved * 100 / denom ))

    echo ""
    echo "=== SWE-agent BASELINE summary ===  resolved=$resolved/$denom ($pct%)  empty_patch=$empty  error=$errs"
    echo "  (compare against \`nix run .#swebench\` on the same instances — that is OUR agent)"

    if [ -n "''${SWEBENCH_OUTPUT_DIR:-}" ]; then
      mkdir -p "$SWEBENCH_OUTPUT_DIR"
      cp "$report" "$SWEBENCH_OUTPUT_DIR/" 2>/dev/null || true
      cp "$preds" "$SWEBENCH_OUTPUT_DIR/swe-agent-preds.json" 2>/dev/null || true
      echo "  kept report + preds in $SWEBENCH_OUTPUT_DIR"
    fi

    if [ "$MIN_RESOLVED" -gt 0 ] && [ "$resolved" -lt "$MIN_RESOLVED" ]; then
      echo "CONTRACT: resolved $resolved < SWE_AGENT_MIN_RESOLVED=$MIN_RESOLVED — regression." >&2
      note_fail 2
    fi

    echo ""
    contract_exit "PASS: swe-agent baseline — resolved $resolved/$denom ($pct%)."
  '';
}
