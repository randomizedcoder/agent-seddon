# `nix run .#eval` — grade the REAL agent on a coding-task corpus with promptfoo.
#
# promptfoo (pinned in nix/versions.nix) drives the agent one-shot through
# `test/eval/agent_provider.sh` (an `exec:` provider) over the tasks in
# `test/eval/tasks.yaml`, grading each BOTH deterministically (assert_compiles.js
# compiles/runs the file the agent wrote) AND with an `llm-rubric` scored by a strong
# judge model. It generalizes the ad-hoc GLM `judge()` in test/e2e-multi/run.sh into a
# declarative, reusable, JUnit/JSON-emitting eval. See docs/eval.md.
#
# NOT a `nix flake check`: it needs a running generator model (the one the agent uses)
# AND a judge endpoint AND a network socket — none of which the hermetic sandbox has.
# Sits beside e2e-live / e2e-multi / review-eval.
#
# Exit codes (the shared 0/1/2 contract, nix/lib/contract.sh):
#   0 — every task passed (compiled/ran + judged correct).
#   1 — HARNESS failure: no generator/judge endpoint, promptfoo errored, agent crashed.
#   2 — MODEL-QUALITY failure: the agent ran but produced wrong/uncompilable code, or
#       the judge scored it a failure.
#
# Env knobs (shared with the e2e harnesses so one setup drives both):
#   generator (the model the AGENT uses): AGENT_E2E_BASE_URL / _MODEL / _API_KEY,
#     AGENT_E2E_INSECURE_TLS, AGENT_E2E_MAX_TOKENS / _CONTEXT_WINDOW.
#   judge   (the model that GRADES): AGENT_E2E_JUDGE_BASE_URL / _MODEL /
#     _API_KEY_FILE / _INSECURE_TLS.
# Extra args are forwarded to `promptfoo eval` (e.g. `-- --filter-pattern hello`).
{
  pkgs,
  lib,
  versions,
  harness,
  agent,
}:
pkgs.writeShellApplication {
  name = "eval";
  runtimeInputs = [
    agent
    versions.promptfoo
    pkgs.git # agent-git tools
    versions.ripgrep # `grep` tool fast path
    pkgs.gcc # cc — assert_compiles.js (C tasks)
    versions.go # go — Go tasks
    versions.rustToolchain # rustc — Rust tasks
    pkgs.python3 # python3 — Python tasks
    pkgs.curl
    pkgs.jq
    pkgs.coreutils
    pkgs.gnused
  ];
  text = ''
    set -uo pipefail
  ''
  + harness.contract
  + ''

    # ---- generator: the model the AGENT uses to write code (refuse, don't skip) ----
    GEN_BASE_URL="''${AGENT_E2E_BASE_URL:-http://localhost:11434/v1}"
    GEN_MODEL="''${AGENT_E2E_MODEL:-llama3.1:latest}"
    GEN_API_KEY="''${AGENT_E2E_API_KEY:-ollama}"
    gopt=(-sf -m 10)
    # Honor a self-signed generator (the agent connects with insecure_tls to match) — else the
    # preflight rejects a valid endpoint the agent itself can reach.
    [ "''${AGENT_E2E_INSECURE_TLS:-0}" = 1 ] && gopt+=(-k)
    if ! curl "''${gopt[@]}" -H "Authorization: Bearer $GEN_API_KEY" "$GEN_BASE_URL/models" >/dev/null 2>&1 \
       && ! curl "''${gopt[@]}" "''${GEN_BASE_URL%/v1}/api/tags" >/dev/null 2>&1; then
      echo "FAIL(harness): no generator model at $GEN_BASE_URL" >&2
      echo "  start one (ollama serve && ollama pull $GEN_MODEL) or set AGENT_E2E_BASE_URL/_MODEL/_API_KEY." >&2
      exit 1
    fi

    # ---- judge: grades the llm-rubric assertions (refuse, don't skip) ----
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

    echo "eval: generator $GEN_MODEL @ $GEN_BASE_URL   judge $JUDGE_MODEL @ $JUDGE_BASE_URL"

    # promptfoo's openai grader reads the endpoint + key from these env vars; TLS off for
    # a self-signed judge; hermetic + fresh (no telemetry/update/share/cache).
    export OPENAI_BASE_URL="$JUDGE_BASE_URL"
    OPENAI_API_KEY="$(cat "$JUDGE_KEY_FILE")"; export OPENAI_API_KEY
    [ "$JUDGE_INSECURE" = 1 ] && export NODE_TLS_REJECT_UNAUTHORIZED=0
    export PROMPTFOO_DISABLE_TELEMETRY=1 PROMPTFOO_DISABLE_UPDATE=1 PROMPTFOO_DISABLE_SHARING=1 PROMPTFOO_DISABLE_CACHE=1

    # Copy the eval kit to a writable scratch dir so promptfoo resolves the relative
    # provider/tests/assert paths, and substitute the judge model into the config.
    work="$(mktemp -d)"
    # shellcheck disable=SC2064
    trap "rm -rf '$work'" EXIT
    cp -r ${../test/eval}/. "$work/"
    chmod -R u+w "$work"
    sed -i "s|__JUDGE_MODEL__|$JUDGE_MODEL|g" "$work/quality.yaml"

    cd "$work"
    echo "eval: running promptfoo over test/eval/tasks.yaml ..."
    set +e
    promptfoo eval -c quality.yaml \
      -o "$work/results.json" -o "$work/results.junit.xml" "$@"
    pf_rc=$?
    set -e

    # Map promptfoo's outcome onto the 0/1/2 contract from the results JSON (its own
    # exit code conflates "some tests failed" with "tooling broke").
    if [ ! -s "$work/results.json" ]; then
      echo "FAIL(harness): promptfoo produced no results (exit $pf_rc)." >&2
      note_fail 1
    else
      succ=$(jq -r '(.results?.stats?.successes // .stats?.successes // 0)' "$work/results.json" 2>/dev/null || echo 0)
      fail=$(jq -r '(.results?.stats?.failures  // .stats?.failures  // 0)' "$work/results.json" 2>/dev/null || echo 0)
      errs=$(jq -r '(.results?.stats?.errors    // .stats?.errors    // 0)' "$work/results.json" 2>/dev/null || echo 0)
      echo ""
      echo "=== eval summary ===  pass=$succ  fail=$fail  error=$errs"
      echo "  JSON: results.json   JUnit: results.junit.xml (in the run's scratch dir)"
      if [ "$errs" -gt 0 ]; then
        echo "CONTRACT: $errs task(s) ERRORED (agent crash / provider or grader error) — harness." >&2
        note_fail 1
      elif [ "$fail" -gt 0 ]; then
        echo "CONTRACT: $fail task(s) FAILED an assertion (bad/uncompilable code or judge FAIL)." >&2
        note_fail 2
      fi
    fi

    echo ""
    contract_exit "PASS: eval — every task compiled/ran and was judged correct."
  '';
}
