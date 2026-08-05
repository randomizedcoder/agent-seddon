# `nix run .#openai-evals` — grade the REAL agent with OpenAI Evals.
#
# A custom COMPLETION FUNCTION (test/openai-evals/agent_completion_fn.py) routes each prompt
# through `agent --config <toml> "<prompt>"` one-shot and returns the agent's `=== ANSWER ===`
# banner, so `oaieval` grades the WHOLE agent, not a raw model. The default eval is our own
# hermetic, deterministically-graded set (`agent-smoke`, an `Includes` grader — no judge);
# `OPENAI_EVALS_EVAL` selects any registry eval. See docs/openai-evals.md.
#
# NOT a `nix flake check`: it needs a running generator model (the one the agent uses) and a
# network socket. Sits beside e2e-live / eval / swebench / inspect.
#
# Exit codes (the shared 0/1/2 contract, nix/lib/contract.sh) — BENCHMARK semantics:
#   0 — ran to completion and produced a report (the accuracy is the measurement) — or the
#       score met `OPENAI_EVALS_MIN_SCORE`.
#   1 — HARNESS failure: no generator model, oaieval errored, or no report.
#   2 — REGRESSION gate: `OPENAI_EVALS_MIN_SCORE > 0` (a percent) and the score fell below it.
#
# Env knobs:
#   generator (the model the AGENT uses): AGENT_E2E_BASE_URL / _MODEL / _API_KEY,
#     AGENT_E2E_INSECURE_TLS, AGENT_E2E_MAX_TOKENS / _CONTEXT_WINDOW.
#   OPENAI_EVALS_EVAL        eval to run (default "agent-smoke" = our hermetic set).
#   OPENAI_EVALS_MAX_SAMPLES cap samples for a fast iteration run.
#   OPENAI_EVALS_MIN_SCORE   regression floor as a PERCENT 0..100 (default 0 = report only).
#   OPENAI_EVALS_AGENT_TIMEOUT  per-sample agent wall-clock seconds (default 300; read by the fn).
#   OPENAI_EVALS_OUTPUT_DIR  if set, the record jsonl is copied here to keep.
#   EVALS_THREADS            concurrent samples (default 4; keep modest for a local model).
# Extra args are forwarded to `oaieval`.
{
  pkgs,
  lib,
  versions,
  harness,
  agent,
}:
let
  # The python that has the `evals`/`oaieval` framework importable.
  pythonEnv = pkgs.python3.withPackages (_ps: [ versions.openai-evals ]);
in
pkgs.writeShellApplication {
  name = "openai-evals";
  runtimeInputs = [
    agent
    pythonEnv
    pkgs.git # agent-git tools
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

    # `import evals` eagerly constructs an OpenAI client, so a (dummy) key must be present;
    # our completion fn routes to the agent, so it is never actually used.
    export OPENAI_API_KEY="''${OPENAI_API_KEY:-sk-dummy-agent-seddon}"
    export EVALS_THREADS="''${EVALS_THREADS:-4}"

    # Copy the eval kit to a writable scratch dir; put its root on PYTHONPATH so
    # `oaieval` can import the `agent_completion_fn` module the registry references.
    work="$(mktemp -d)"
    # shellcheck disable=SC2064
    trap "rm -rf '$work'" EXIT
    cp -r ${../test/openai-evals}/. "$work/"
    chmod -R u+w "$work"
    cd "$work"
    export PYTHONPATH="$work''${PYTHONPATH:+:$PYTHONPATH}"

    EVAL="''${OPENAI_EVALS_EVAL:-agent-smoke}"
    rec="$work/record.jsonl"
    oaieval_args=(
      agent-seddon
      "$EVAL"
      --registry_path "$work/registry"
      --record_path "$rec"
    )
    [ -n "''${OPENAI_EVALS_MAX_SAMPLES:-}" ] && oaieval_args+=(--max_samples "$OPENAI_EVALS_MAX_SAMPLES")

    echo "openai-evals: generator $GEN_MODEL @ $GEN_BASE_URL   eval=$EVAL"
    echo "openai-evals: running oaieval (driving the agent per sample) ..."
    set +e
    oaieval "''${oaieval_args[@]}" "$@"
    oe_rc=$?
    set -e

    if [ ! -s "$rec" ]; then
      echo "FAIL(harness): oaieval produced no record (exit $oe_rc)." >&2
      note_fail 1
      contract_exit "PASS: openai-evals"
    fi

    # The record jsonl ends with a `{"final_report": {"accuracy": ...}}` line.
    acc=$(jq -rs '[.[] | select(.final_report) | .final_report.accuracy] | last // 0' "$rec" 2>/dev/null || echo 0)
    pct=$(jq -rn --argjson a "''${acc:-0}" '($a * 100) | floor' 2>/dev/null || echo 0)
    pct="''${pct:-0}"
    MIN="''${OPENAI_EVALS_MIN_SCORE:-0}"

    echo ""
    echo "=== openai-evals summary ===  eval=$EVAL  accuracy=$acc  score=$pct%"
    echo "  record: record.jsonl (in the run's scratch dir)"

    if [ -n "''${OPENAI_EVALS_OUTPUT_DIR:-}" ]; then
      mkdir -p "$OPENAI_EVALS_OUTPUT_DIR"
      cp "$rec" "$OPENAI_EVALS_OUTPUT_DIR/" 2>/dev/null || true
      echo "  kept record in $OPENAI_EVALS_OUTPUT_DIR"
    fi

    # No final_report at all => oaieval didn't complete the eval => harness failure.
    if ! jq -e -s 'any(.[]; .final_report)' "$rec" >/dev/null 2>&1; then
      echo "CONTRACT: oaieval wrote no final_report — the eval did not complete." >&2
      note_fail 1
    elif [ "$MIN" -gt 0 ] && [ "$pct" -lt "$MIN" ]; then
      echo "CONTRACT: score $pct% < OPENAI_EVALS_MIN_SCORE=$MIN% — regression." >&2
      note_fail 2
    fi

    echo ""
    contract_exit "PASS: openai-evals — eval=$EVAL score $pct%."
  '';
}
