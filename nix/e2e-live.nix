# `nix run .#e2e-live` — drive the REAL agent against a REAL model and check it
# can do a small coding task end to end.
#
# This is the only tier that answers "can it actually do the job". It is
# deliberately NOT a `checks.*`: it needs a network socket and a running model,
# and `nix/checks/test.nix` is hermetic by design. The hermetic tier
# (`crates/agent-cli/tests/cli_e2e.rs`) proves the binary, the wire format and
# the tool path with a scripted server; this proves a real model can drive them.
#
# Exit codes are split deliberately, because the two failures have different
# owners and conflating them makes the test useless:
#
#   0 — the agent ran the task AND the produced program compiles and runs.
#   1 — HARNESS failure: the agent errored, called no tool, or wrote no file.
#       That is our bug.
#   2 — MODEL-QUALITY failure: the agent worked, but the model's output does not
#       compile. Small local models do this regularly (an 8B model was observed
#       emitting `<stdio.h:` and doubly-escaped newlines). Not our bug, but
#       it must be visible rather than dressed up as a pass.
{
  pkgs,
  lib,
  versions,
  agent,
}:
pkgs.writeShellApplication {
  name = "e2e-live";
  runtimeInputs = [
    agent
    pkgs.curl
    pkgs.gcc
    pkgs.coreutils
  ];
  text = ''
    set -uo pipefail

    BASE_URL="''${AGENT_E2E_BASE_URL:-http://localhost:11434/v1}"
    MODEL="''${AGENT_E2E_MODEL:-llama3.1:latest}"
    API_KEY="''${AGENT_E2E_API_KEY:-ollama}"

    echo "e2e-live: $MODEL at $BASE_URL"

    # Refuse rather than skip. A skip that exits 0 reads as a pass in a log, and
    # the whole point of this tier is that it actually talked to a model.
    probe="''${BASE_URL%/v1}/api/tags"
    if ! curl -sf -m 5 "$probe" >/dev/null 2>&1 \
       && ! curl -sf -m 5 "$BASE_URL/models" >/dev/null 2>&1; then
      echo "FAIL: no model server reachable at $BASE_URL" >&2
      echo "  start one:  ollama serve && ollama pull $MODEL" >&2
      echo "  or point elsewhere: AGENT_E2E_BASE_URL=... AGENT_E2E_MODEL=... AGENT_E2E_API_KEY=..." >&2
      exit 1
    fi

    work="$(mktemp -d)"
    # shellcheck disable=SC2064
    trap "rm -rf '$work'" EXIT

    cat > "$work/agent.toml" <<EOF
    [agent]
    provider = "openai-compat"
    context  = "sliding-window"
    policy   = "auto-approve"
    working_dir = "$work"
    max_iterations = 10
    max_tokens = 2048
    context_window = 8192
    reserve_output = 2048
    stream = false
    # Deterministic as the model allows: this is a test, and sampling noise in a
    # small model shows up directly as flaky C.
    temperature = 0.0
    system_prompt = "You are a coding agent. Use the provided tools to create files. When done, reply with a short summary."

    [provider]
    base_url = "$BASE_URL"
    model    = "$MODEL"
    api_key  = "$API_KEY"
    max_retries = 2

    [memory]
    backend       = "file"
    episodic_path = "$work/.agent/episodic.jsonl"
    semantic_dir  = "$work/.agent/memory"

    [tools]
    enabled = ["read_file", "write_file", "edit", "ls"]

    [search]
    auto_index = false

    [metrics]
    enabled = false
    EOF

    echo "e2e-live: asking the agent to write hello.c ..."
    set +e
    ( cd "$work" && agent --config "$work/agent.toml" \
        "write a hello world program in C called hello.c that prints Hello, World!" \
    ) > "$work/stdout.log" 2> "$work/stderr.log"
    agent_rc=$?
    set -e

    dump_diagnostics() {
      echo "--- agent stderr ---" >&2
      tail -n 40 "$work/stderr.log" >&2 || true
      echo "--- episodic log (what the agent actually did) ---" >&2
      tail -n 20 "$work/.agent/episodic.jsonl" 2>/dev/null >&2 || echo "(none written)" >&2
    }

    if [ "$agent_rc" -ne 0 ]; then
      echo "FAIL(harness): the agent exited $agent_rc" >&2
      dump_diagnostics
      exit 1
    fi

    if [ ! -f "$work/hello.c" ]; then
      echo "FAIL(harness): the agent exited 0 but wrote no hello.c" >&2
      echo "  Usually means the model answered in prose instead of calling a tool." >&2
      dump_diagnostics
      exit 1
    fi

    echo "e2e-live: hello.c written (''$(wc -c < "$work/hello.c") bytes)"
    echo "--- hello.c ---"
    cat "$work/hello.c"
    echo "---------------"

    if ! cc "$work/hello.c" -o "$work/hello" 2> "$work/cc.log"; then
      echo "WARN(model): the agent worked, but the generated C does not compile." >&2
      sed 's/^/  /' "$work/cc.log" >&2
      echo "  The harness is fine — $MODEL produced invalid C. Try a stronger model." >&2
      exit 2
    fi

    out="$("$work/hello")"
    echo "e2e-live: program output: $out"
    case "$out" in
      *Hello*) ;;
      *)
        echo "WARN(model): compiled, but printed '$out' rather than a hello greeting." >&2
        exit 2
        ;;
    esac

    echo "PASS: the agent wrote a C program that compiles and prints: $out"
  '';
}
