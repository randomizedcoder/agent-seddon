# `nix run .#redteam` — adversarially probe the REAL agent with promptfoo.
#
# Drives the agent (via test/eval/agent_provider.sh) through an untrusted-model red-team
# with its DEFENSES ACTIVE — the harness sets AGENT_EVAL_GUARD=deny so the dangerous-
# command / sensitive-path / secret guard hard-denies malicious tool calls without an
# interactive prompt. Two modes:
#   default   — the hand-written, fully-local suite (test/eval/redteam.yaml).
#   --plugins — promptfoo's `coding-agent:*` plugin generators (redteam.plugins.yaml);
#               NOTE generation may call promptfoo's remote service (see that file).
# See docs/eval.md.
#
# NOT a `nix flake check`: needs the agent's generator model, a judge endpoint, and a
# socket. Sits beside eval / e2e-multi.
#
# Exit codes (shared 0/1/2 contract, nix/lib/contract.sh):
#   0 — the agent's defenses HELD on every adversarial case.
#   1 — HARNESS failure: no generator/judge endpoint, promptfoo errored, agent crashed.
#   2 — a BREACH was demonstrated (an adversarial case scored a compliance failure) —
#       surfaced, not hidden.
#
# Env: generator (AGENT_E2E_BASE_URL/_MODEL/_API_KEY) + judge
# (AGENT_E2E_JUDGE_BASE_URL/_MODEL/_API_KEY_FILE/_INSECURE_TLS), as for `nix run .#eval`.
{
  pkgs,
  lib,
  versions,
  harness,
  agent,
}:
pkgs.writeShellApplication {
  name = "redteam";
  runtimeInputs = [
    agent
    versions.promptfoo
    pkgs.git
    versions.ripgrep
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

    MODE=local
    args=()
    for a in "$@"; do
      case "$a" in
        --plugins) MODE=plugins ;;
        *) args+=("$a") ;;
      esac
    done

    # ---- generator: the model the AGENT uses (refuse, don't skip) ----
    GEN_BASE_URL="''${AGENT_E2E_BASE_URL:-http://localhost:11434/v1}"
    GEN_MODEL="''${AGENT_E2E_MODEL:-llama3.1:latest}"
    GEN_API_KEY="''${AGENT_E2E_API_KEY:-ollama}"
    gopt=(-sf -m 10)
    # Honor a self-signed generator (the agent connects with insecure_tls to match).
    [ "''${AGENT_E2E_INSECURE_TLS:-0}" = 1 ] && gopt+=(-k)
    if ! curl "''${gopt[@]}" -H "Authorization: Bearer $GEN_API_KEY" "$GEN_BASE_URL/models" >/dev/null 2>&1 \
       && ! curl "''${gopt[@]}" "''${GEN_BASE_URL%/v1}/api/tags" >/dev/null 2>&1; then
      echo "FAIL(harness): no generator model at $GEN_BASE_URL (set AGENT_E2E_BASE_URL/_MODEL/_API_KEY)." >&2
      exit 1
    fi

    # ---- judge: grades the llm-rubric assertions (refuse, don't skip) ----
    JUDGE_BASE_URL="''${AGENT_E2E_JUDGE_BASE_URL:-https://213.173.96.56:8000/v1}"
    JUDGE_MODEL="''${AGENT_E2E_JUDGE_MODEL:-/model}"
    JUDGE_KEY_FILE="''${AGENT_E2E_JUDGE_API_KEY_FILE:-$HOME/Downloads/runpod/glm/glm-api-key}"
    JUDGE_INSECURE="''${AGENT_E2E_JUDGE_INSECURE_TLS:-1}"
    if [ ! -r "$JUDGE_KEY_FILE" ]; then
      echo "FAIL(harness): judge key $JUDGE_KEY_FILE unreadable (set AGENT_E2E_JUDGE_API_KEY_FILE)." >&2
      exit 1
    fi
    jopt=(-sf -m 20); [ "$JUDGE_INSECURE" = 1 ] && jopt+=(-k)
    if ! curl "''${jopt[@]}" "$JUDGE_BASE_URL/models" -H "Authorization: Bearer $(cat "$JUDGE_KEY_FILE")" >/dev/null 2>&1; then
      echo "FAIL(harness): judge endpoint $JUDGE_BASE_URL unreachable (set AGENT_E2E_JUDGE_BASE_URL)." >&2
      exit 1
    fi

    echo "redteam: mode=$MODE  generator $GEN_MODEL @ $GEN_BASE_URL  judge $JUDGE_MODEL @ $JUDGE_BASE_URL"

    export OPENAI_BASE_URL="$JUDGE_BASE_URL"
    OPENAI_API_KEY="$(cat "$JUDGE_KEY_FILE")"; export OPENAI_API_KEY
    [ "$JUDGE_INSECURE" = 1 ] && export NODE_TLS_REJECT_UNAUTHORIZED=0
    export PROMPTFOO_DISABLE_TELEMETRY=1 PROMPTFOO_DISABLE_UPDATE=1 PROMPTFOO_DISABLE_SHARING=1 PROMPTFOO_DISABLE_CACHE=1
    # Run the agent WITH its defenses active so malicious tool calls are hard-denied.
    export AGENT_EVAL_POLICY=auto-approve AGENT_EVAL_GUARD=deny

    work="$(mktemp -d)"
    # shellcheck disable=SC2064
    trap "rm -rf '$work'" EXIT
    cp -r ${../test/eval}/. "$work/"
    chmod -R u+w "$work"
    sed -i "s|__JUDGE_MODEL__|$JUDGE_MODEL|g" "$work/redteam.yaml"
    cd "$work"

    set +e
    if [ "$MODE" = plugins ]; then
      echo "redteam: generating + running promptfoo coding-agent plugin cases ..."
      echo "  NOTE: plugin GENERATION may call promptfoo's remote service (see redteam.plugins.yaml)."
      promptfoo redteam run -c redteam.plugins.yaml \
        -o "$work/results.json" "''${args[@]}"
    else
      echo "redteam: running the local adversarial suite (test/eval/redteam.yaml) ..."
      promptfoo eval -c redteam.yaml \
        -o "$work/results.json" -o "$work/results.junit.xml" "''${args[@]}"
    fi
    pf_rc=$?
    set -e

    if [ ! -s "$work/results.json" ]; then
      echo "FAIL(harness): promptfoo produced no results (exit $pf_rc)." >&2
      note_fail 1
    else
      held=$(jq -r '(.results?.stats?.successes // .stats?.successes // 0)' "$work/results.json" 2>/dev/null || echo 0)
      breach=$(jq -r '(.results?.stats?.failures // .stats?.failures // 0)' "$work/results.json" 2>/dev/null || echo 0)
      errs=$(jq -r '(.results?.stats?.errors // .stats?.errors // 0)' "$work/results.json" 2>/dev/null || echo 0)
      echo ""
      echo "=== redteam summary ===  defended=$held  BREACHED=$breach  error=$errs"
      if [ "$errs" -gt 0 ]; then
        echo "CONTRACT: $errs case(s) ERRORED (agent crash / provider or grader error) — harness." >&2
        note_fail 1
      elif [ "$breach" -gt 0 ]; then
        echo "CONTRACT: $breach adversarial case(s) succeeded against the agent — SECURITY FINDING." >&2
        note_fail 2
      fi
    fi

    echo ""
    contract_exit "PASS: redteam — the agent's defenses held on every adversarial case."
  '';
}
