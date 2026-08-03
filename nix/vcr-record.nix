# nix/vcr-record.nix
#
# `nix run .#vcr-record` — refresh the provider cassettes replayed by the hermetic
# `vcr_matrix` test (crates/agent-cli/tests/vcr_matrix.rs) from a REAL endpoint.
#
# The matrix test replays committed cassettes
# (crates/agent-cli/tests/fixtures/cassettes/{openai_compat,anthropic}.json) so the
# openai-compat AND Anthropic wire paths are driven through the binary offline. Those
# cassettes are just the provider's own response bodies for the golden "write hello.c"
# tool turn; this app re-captures the volatile turn-1 body (the `tool_use` /
# `tool_calls` response) live, so a real wire change is picked up on refresh.
#
# NOT a check: it needs a real model endpoint + key, like `e2e-live`. It writes ONLY
# response bodies to the cassette — never the request, the API key, or any header — so
# no secret is persisted. Review the cassette diff before committing.
#
# Usage:
#   AGENT_VCR_PROVIDER=openai-compat \
#   AGENT_VCR_BASE_URL=http://localhost:11434/v1 AGENT_VCR_MODEL=llama3.1:latest \
#   AGENT_VCR_API_KEY=ollama  nix run .#vcr-record
#
#   AGENT_VCR_PROVIDER=anthropic \
#   AGENT_VCR_BASE_URL=https://api.anthropic.com/v1 AGENT_VCR_MODEL=claude-haiku-4-5 \
#   AGENT_VCR_API_KEY=sk-ant-...  nix run .#vcr-record
#
# Exit codes:
#   0 — captured a turn-1 tool-call body and rewrote the cassette.
#   1 — HARNESS failure: no endpoint reachable, or bad arguments.
#   2 — the model returned no tool call (can't seed a tool-loop cassette) — try a
#       stronger model. Not our bug, but must be visible rather than silently passing.
{
  pkgs,
}:
pkgs.writeShellApplication {
  name = "vcr-record";
  runtimeInputs = [
    pkgs.curl
    pkgs.jq
    pkgs.coreutils
  ];
  text = ''
    set -uo pipefail

    PROVIDER="''${AGENT_VCR_PROVIDER:-openai-compat}"
    BASE_URL="''${AGENT_VCR_BASE_URL:-http://localhost:11434/v1}"
    MODEL="''${AGENT_VCR_MODEL:-llama3.1:latest}"
    API_KEY="''${AGENT_VCR_API_KEY:-ollama}"
    DEST="''${AGENT_VCR_DEST:-crates/agent-cli/tests/fixtures/cassettes}"

    GOAL="write a hello world program in C called hello.c that prints Hello, World!"
    # The write_file tool schema the agent advertises (kept minimal — only what the
    # model needs to emit a valid call).
    write_file_params='{"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}},"required":["path","content"]}'

    tmp="$(mktemp -d)"
    # shellcheck disable=SC2064
    trap "rm -rf '$tmp'" EXIT

    case "$PROVIDER" in
      openai-compat)
        cass="$DEST/openai_compat.json"
        req="$tmp/req.json"
        jq -n --arg model "$MODEL" --arg goal "$GOAL" --argjson params "$write_file_params" '
          {model:$model, stream:false, temperature:0,
           messages:[{role:"user",content:$goal}],
           tools:[{type:"function",function:{name:"write_file",description:"Write a UTF-8 text file.",parameters:$params}}]}' > "$req"
        echo "vcr-record: POST $BASE_URL/chat/completions ($MODEL) ..."
        if ! curl -sf -m 60 -H "Authorization: Bearer $API_KEY" -H "Content-Type: application/json" \
             -d @"$req" "$BASE_URL/chat/completions" -o "$tmp/turn0.json"; then
          echo "FAIL(harness): no openai-compat endpoint reachable at $BASE_URL" >&2
          exit 1
        fi
        if [ "$(jq '.choices[0].message.tool_calls | length' "$tmp/turn0.json" 2>/dev/null || echo 0)" -lt 1 ]; then
          echo "WARN(model): $MODEL returned no tool_calls; cannot seed a tool-loop cassette." >&2
          jq -r '.choices[0].message.content // "(no content)"' "$tmp/turn0.json" >&2 || true
          exit 2
        fi
        turn1='{"id":"chatcmpl-vcr","object":"chat.completion","choices":[{"index":0,"message":{"role":"assistant","content":"Wrote hello.c."},"finish_reason":"stop"}],"usage":{"prompt_tokens":30,"completion_tokens":5,"total_tokens":35}}'
        jq -s --argjson turn1 "$turn1" '[.[0], $turn1]' "$tmp/turn0.json" > "$cass"
        ;;
      anthropic)
        cass="$DEST/anthropic.json"
        req="$tmp/req.json"
        jq -n --arg model "$MODEL" --arg goal "$GOAL" --argjson params "$write_file_params" '
          {model:$model, max_tokens:1024, temperature:0,
           messages:[{role:"user",content:$goal}],
           tools:[{name:"write_file",description:"Write a UTF-8 text file.",input_schema:$params}]}' > "$req"
        echo "vcr-record: POST $BASE_URL/messages ($MODEL) ..."
        if ! curl -sf -m 60 -H "x-api-key: $API_KEY" -H "anthropic-version: 2023-06-01" \
             -H "Content-Type: application/json" \
             -d @"$req" "$BASE_URL/messages" -o "$tmp/turn0.json"; then
          echo "FAIL(harness): no Anthropic endpoint reachable at $BASE_URL" >&2
          exit 1
        fi
        if [ "$(jq '[.content[]? | select(.type=="tool_use")] | length' "$tmp/turn0.json" 2>/dev/null || echo 0)" -lt 1 ]; then
          echo "WARN(model): $MODEL returned no tool_use block; cannot seed a tool-loop cassette." >&2
          exit 2
        fi
        turn1='{"id":"msg_vcr2","type":"message","role":"assistant","content":[{"type":"text","text":"Wrote hello.c."}],"stop_reason":"end_turn","usage":{"input_tokens":25,"output_tokens":5}}'
        jq -s --argjson turn1 "$turn1" '[.[0], $turn1]' "$tmp/turn0.json" > "$cass"
        ;;
      *)
        echo "FAIL(harness): AGENT_VCR_PROVIDER must be openai-compat or anthropic, got '$PROVIDER'" >&2
        exit 1
        ;;
    esac

    echo "vcr-record: rewrote $cass — review the diff before committing."
  '';
}
