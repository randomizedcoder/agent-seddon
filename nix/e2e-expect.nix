# `nix run .#e2e-expect` — drive the REAL agent through a MULTI-TURN interactive
# REPL conversation with a REAL model (via tcl/expect) and check it can actually
# do the job. This is the interactive companion to `nix run .#e2e-live` (which is
# one-shot, single-file): it proves the iterative, tool-selecting, within-session
# paths that only a real model + a real conversation exercise.
#
# The pattern is pcp's (build/nix/lifecycle): nix boots the environment, expect
# drives it. Here expect `spawn`s the binary directly — the shared procs live in
# test/expect/lib.exp, the multi-turn driver in test/expect/repl_session.exp, and
# THIS wrapper owns the scenario table + the pass/fail judgement (mirroring
# e2e-live, where the shell does the checking).
#
# Not a `checks.*`: it needs a network socket and a running model, which the
# hermetic sandbox has neither of. The tcl/expect harness itself is CI-validated
# model-free by nix/checks/expect-smoke.nix.
#
# Exit codes match e2e-live's contract, aggregated over the scenario table:
#   0 — every scenario passed.
#   1 — a HARNESS failure (a turn timed out, the agent errored, or wrote no file).
#   2 — a MODEL-QUALITY failure only (the agent ran, but the output was wrong).
{
  pkgs,
  lib,
  versions,
  agent,
}:
pkgs.writeShellApplication {
  name = "e2e-expect";
  runtimeInputs = [
    agent
    pkgs.expect
    pkgs.gcc
    pkgs.curl
    pkgs.coreutils
    pkgs.gnugrep
  ];
  text = ''
    set -uo pipefail

    EXPECT_SRC="${../test/expect}"
    # A single turn can fan out to many tool iterations, each a model round-trip,
    # so a small local model needs headroom; a genuinely hung turn still fails.
    TURN_TIMEOUT="''${AGENT_E2E_TURN_TIMEOUT:-180}"

    BASE_URL="''${AGENT_E2E_BASE_URL:-http://localhost:11434/v1}"
    MODEL="''${AGENT_E2E_MODEL:-llama3.1:latest}"
    API_KEY="''${AGENT_E2E_API_KEY:-ollama}"
    MAX_TOKENS="''${AGENT_E2E_MAX_TOKENS:-2048}"
    CONTEXT_WINDOW="''${AGENT_E2E_CONTEXT_WINDOW:-8192}"
    INSECURE_TLS="''${AGENT_E2E_INSECURE_TLS:-0}"

    echo "e2e-expect: $MODEL at $BASE_URL (multi-turn REPL via tcl/expect)"

    curl_opts=(-sf -m 10)
    tls_line=""
    if [ "$INSECURE_TLS" = "1" ]; then
      echo "e2e-expect: TLS verification DISABLED (AGENT_E2E_INSECURE_TLS=1)"
      curl_opts+=(-k)
      tls_line="insecure_tls = true"
    fi

    # Refuse rather than skip — a skip that exits 0 reads as a pass, and the whole
    # point of this tier is that it actually talked to a model.
    auth=(-H "Authorization: Bearer $API_KEY")
    if ! curl "''${curl_opts[@]}" "''${auth[@]}" "$BASE_URL/models" >/dev/null 2>&1 \
       && ! curl "''${curl_opts[@]}" "''${BASE_URL%/v1}/api/tags" >/dev/null 2>&1; then
      echo "FAIL: no model server reachable at $BASE_URL" >&2
      echo "  start one:  ollama serve && ollama pull $MODEL" >&2
      echo "  or point elsewhere with AGENT_E2E_BASE_URL / _MODEL / _API_KEY." >&2
      exit 1
    fi

    # TERM=dumb + NO_COLOR so expect matches text, not terminal control bytes.
    export TERM=dumb
    export NO_COLOR=1

    HARNESS_FAILS=0
    MODEL_FAILS=0
    PASSES=0
    declare -a SUMMARY=()

    # write_cfg <workdir> <tools-toml-array>
    write_cfg() {
      local wd="$1" tools="$2"
      cat > "$wd/agent.toml" <<EOF
    [agent]
    provider = "openai-compat"
    context  = "sliding-window"
    policy   = "auto-approve"
    working_dir = "$wd"
    max_iterations = 15
    max_tokens = $MAX_TOKENS
    context_window = $CONTEXT_WINDOW
    reserve_output = $MAX_TOKENS
    stream = false
    temperature = 0.0
    system_prompt = "You are a coding agent. Use the provided tools to read and write files. Keep replies short."

    [provider]
    base_url = "$BASE_URL"
    model    = "$MODEL"
    api_key  = "$API_KEY"
    max_retries = 2
    $tls_line

    [memory]
    backend       = "file"
    episodic_path = "$wd/.agent/episodic.jsonl"
    semantic_dir  = "$wd/.agent/memory"

    [tools]
    enabled = [$tools]

    [search]
    auto_index = false

    [metrics]
    enabled = false
    EOF
    }

    # drive <workdir> <prompt...> — spawn the REPL and run the turns via expect.
    # Returns the expect exit code (0 = all turns completed; nonzero = harness).
    drive() {
      local wd="$1"; shift
      set +e
      expect "$EXPECT_SRC/repl_session.exp" "$wd/agent.toml" "$TURN_TIMEOUT" "$@" \
        > "$wd/session.log" 2>&1
      local rc=$?
      set -e
      return $rc
    }

    record_harness() { HARNESS_FAILS=$((HARNESS_FAILS + 1)); SUMMARY+=("FAIL(harness)  $1"); echo "FAIL(harness): $1" >&2; }
    record_model()   { MODEL_FAILS=$((MODEL_FAILS + 1));     SUMMARY+=("WARN(model)    $1"); echo "WARN(model): $1" >&2; }
    record_pass()    { PASSES=$((PASSES + 1));               SUMMARY+=("PASS           $1"); echo "PASS: $1"; }

    # ---- scenario 1: multi-turn edit + within-session memory ----------------
    scenario_hello_c_multiturn() {
      local name="hello-c-multiturn" wd
      wd="$(mktemp -d)"; trap 'rm -rf "$wd"' RETURN
      write_cfg "$wd" '"read_file","write_file","edit","ls"'
      if ! drive "$wd" \
        "Write a C program in a file called hello.c that prints Hello, World! to stdout." \
        "Now edit hello.c so it also prints a second line that says Goodbye."; then
        record_harness "$name (a turn did not complete)"; sed 's/^/  /' "$wd/session.log" | tail -n 20 >&2; return
      fi
      if [ ! -f "$wd/hello.c" ]; then record_harness "$name (no hello.c written)"; return; fi
      if ! cc "$wd/hello.c" -o "$wd/hello" 2> "$wd/cc.log"; then
        record_model "$name (hello.c does not compile)"; sed 's/^/  /' "$wd/cc.log" >&2; return
      fi
      local out; out="$("$wd/hello")"
      case "$out" in
        *Hello*) record_pass "$name (compiles, prints: $(echo "$out" | tr '\n' ' '))" ;;
        *) record_model "$name (compiled but printed '$out')" ;;
      esac
    }

    # ---- scenario 2: read -> diagnose -> edit (red -> green) -----------------
    scenario_fix_compile_error() {
      local name="fix-compile-error" wd
      wd="$(mktemp -d)"; trap 'rm -rf "$wd"' RETURN
      write_cfg "$wd" '"read_file","write_file","edit","ls"'
      # A deliberately broken C file (missing semicolon) for the agent to fix.
      cat > "$wd/broken.c" <<'BROKEN'
    #include <stdio.h>
    int main(void) {
        printf("hi\n")
        return 0;
    }
    BROKEN
      if cc "$wd/broken.c" -o /dev/null 2>/dev/null; then
        record_harness "$name (seed broken.c unexpectedly compiled)"; return
      fi
      if ! drive "$wd" \
        "The file broken.c does not compile. Read it, find the bug, and fix it so it compiles with cc. Keep its behaviour."; then
        record_harness "$name (a turn did not complete)"; sed 's/^/  /' "$wd/session.log" | tail -n 20 >&2; return
      fi
      if [ ! -f "$wd/broken.c" ]; then record_harness "$name (broken.c disappeared)"; return; fi
      if cc "$wd/broken.c" -o "$wd/fixed" 2> "$wd/cc.log"; then
        record_pass "$name (agent made broken.c compile)"
      else
        record_model "$name (still does not compile)"; sed 's/^/  /' "$wd/cc.log" >&2
      fi
    }

    # ---- scenario 3: tool selection + assert on the RESPONSE -----------------
    scenario_count_with_tools() {
      local name="count-with-tools" wd
      wd="$(mktemp -d)"; trap 'rm -rf "$wd"' RETURN
      write_cfg "$wd" '"ls","read_file","grep","find","bash"'
      : > "$wd/a.c"; : > "$wd/b.c"; : > "$wd/c.c"      # three .c files
      : > "$wd/notes.txt"; : > "$wd/readme.md"          # and some non-.c noise
      if ! drive "$wd" \
        "How many files ending in .c are in the current directory? Use your tools to check, then reply with just the number."; then
        record_harness "$name (a turn did not complete)"; sed 's/^/  /' "$wd/session.log" | tail -n 20 >&2; return
      fi
      # The answer is the model's response text (echoed into session.log). Assert
      # loosely: a standalone 3 must appear. Real models phrase freely, so a miss
      # is a model-quality result, not a harness bug.
      if grep -Eq '(^|[^0-9])3([^0-9]|$)' "$wd/session.log"; then
        record_pass "$name (answered 3)"
      else
        record_model "$name (did not answer 3)"; sed 's/^/  /' "$wd/session.log" | tail -n 15 >&2
      fi
    }

    scenario_hello_c_multiturn
    scenario_fix_compile_error
    scenario_count_with_tools

    echo
    echo "=== e2e-expect summary ==="
    for line in "''${SUMMARY[@]}"; do echo "  $line"; done
    echo "  $PASSES passed / $MODEL_FAILS model-quality / $HARNESS_FAILS harness"

    if [ "$HARNESS_FAILS" -gt 0 ]; then exit 1; fi
    if [ "$MODEL_FAILS" -gt 0 ]; then exit 2; fi
    exit 0
  '';
}
