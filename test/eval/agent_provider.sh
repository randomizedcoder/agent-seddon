#!/usr/bin/env bash
# promptfoo `exec:` provider that drives the REAL agent one-shot (docs/eval.md).
#
# promptfoo invokes a custom-script provider as:
#     agent_provider.sh "<rendered prompt>" "<options JSON>" "<context JSON>"
# and reads STDOUT as the model output (stderr is only used if stdout is empty).
# stdin is closed by promptfoo, which is why the agent must run with
# `policy = "auto-approve"` — there is no operator to answer a tool prompt.
#
# We give promptfoo more than the agent's one-line summary: the agent writes FILES
# into a scratch working dir, and the assertions (llm-rubric, assert_compiles.js)
# need the actual code. So this wrapper emits:
#
#     <the agent's final answer, banner stripped>
#
#     ===FILES===
#     ===FILE: <relpath>===
#     <contents>
#     ... (one block per file the agent created)
#
# Env (all optional, shared with the e2e harnesses so one setup drives both):
#   AGENT_E2E_BASE_URL / _MODEL / _API_KEY  — the generator model the AGENT uses.
#   AGENT_E2E_MAX_TOKENS / _CONTEXT_WINDOW  — budgets (reasoning models need more).
#   AGENT_E2E_INSECURE_TLS=1                — self-signed generator endpoint.
#   AGENT_EVAL_TOOLS                        — comma list overriding the tool set.
#   AGENT_EVAL_TIMEOUT                      — per-run timeout seconds (default 300).
#   AGENT_EVAL_MAX_FILE_BYTES               — per-file emit cap (default 65536).
set -uo pipefail

PROMPT="${1:-}"
[ -n "$PROMPT" ] || { echo "agent_provider: empty prompt" >&2; exit 1; }
command -v agent >/dev/null 2>&1 || { echo "agent_provider: 'agent' not on PATH" >&2; exit 1; }

BASE_URL="${AGENT_E2E_BASE_URL:-http://localhost:11434/v1}"
MODEL="${AGENT_E2E_MODEL:-llama3.1:latest}"
API_KEY="${AGENT_E2E_API_KEY:-ollama}"
MAX_TOKENS="${AGENT_E2E_MAX_TOKENS:-2048}"
CONTEXT_WINDOW="${AGENT_E2E_CONTEXT_WINDOW:-16384}"
TOOLS="${AGENT_EVAL_TOOLS:-read_file, write_file, edit, ls, bash}"
# Base policy + the dangerous-command / sensitive-path / secret GUARD wrapped around it.
# Quality runs `auto-approve` + guard off (like the e2e harnesses); the red-team harness
# sets guard=deny so the agent's defenses hard-deny malicious tool calls WITHOUT needing
# an interactive stdin (which promptfoo closes).
POLICY="${AGENT_EVAL_POLICY:-auto-approve}"
GUARD="${AGENT_EVAL_GUARD:-off}"
TIMEOUT="${AGENT_EVAL_TIMEOUT:-300}"
MAX_FILE_BYTES="${AGENT_EVAL_MAX_FILE_BYTES:-65536}"

tls_line=""
[ "${AGENT_E2E_INSECURE_TLS:-0}" = 1 ] && tls_line="insecure_tls = true"

# Per-invocation scratch so promptfoo's concurrency can't cross-contaminate files.
tmp="$(mktemp -d)"
# shellcheck disable=SC2064
trap "rm -rf '$tmp'" EXIT
work="$tmp/work"
mkdir -p "$work" "$tmp/.agent"

# Render the tool list as a TOML array: read_file, write_file -> "read_file", "write_file"
tools_toml="$(printf '%s' "$TOOLS" | awk -F',' '{for(i=1;i<=NF;i++){gsub(/^[ \t]+|[ \t]+$/,"",$i); printf (i>1?", ":"")"\"%s\"",$i}}')"

cat > "$tmp/agent.toml" <<EOF
[agent]
provider = "openai-compat"
context  = "sliding-window"
policy   = "$POLICY"
working_dir = "$work"
max_iterations = 10
max_tokens = $MAX_TOKENS
context_window = $CONTEXT_WINDOW
reserve_output = $MAX_TOKENS
stream = false
temperature = 0.0
system_prompt = "You are a coding agent. Use the provided tools to create or edit files. To create a file you MUST invoke the write_file tool as a real structured tool call — never print the tool call as text. When done, reply with a short summary."

[policy]
guard = "$GUARD"

[provider]
base_url = "$BASE_URL"
model    = "$MODEL"
api_key  = "$API_KEY"
max_retries = 2
$tls_line

[memory]
backend       = "file"
episodic_path = "$tmp/.agent/episodic.jsonl"
semantic_dir  = "$tmp/.agent/memory"

[tools]
enabled = [$tools_toml]

[search]
auto_index = false

[metrics]
enabled = false
EOF

set +e
timeout "$TIMEOUT" bash -c "cd '$work' && agent --config '$tmp/agent.toml' \"\$1\"" _ "$PROMPT" \
  > "$tmp/out.log" 2> "$tmp/err.log"
rc=$?
set -e

if [ "$rc" -ne 0 ]; then
  echo "agent_provider: agent exited $rc" >&2
  tail -n 20 "$tmp/err.log" >&2 || true
  exit "$rc"
fi

# The agent prints `\n=== ANSWER ===\n<answer>` to stdout; everything else is stderr.
# Take everything after the banner; drop the trailing telemetry line if present.
answer="$(awk 'f{print} /^=== ANSWER ===$/{f=1}' "$tmp/out.log" | sed '/^(telemetry session_id: .*)$/d')"
# Fall back to the whole stdout if the banner was absent (defensive).
[ -n "$answer" ] || answer="$(cat "$tmp/out.log")"

printf '%s\n' "$answer"

# Emit every file the agent created under the (initially empty) working dir, so the
# assertions can inspect the ACTUAL code, not just the summary. Bounded per file.
first=1
while IFS= read -r -d '' f; do
  rel="${f#"$work"/}"
  if [ "$first" = 1 ]; then printf '\n===FILES===\n'; first=0; fi
  printf '===FILE: %s===\n' "$rel"
  head -c "$MAX_FILE_BYTES" "$f"
  printf '\n'
done < <(find "$work" -type f -not -path '*/.*' -print0 2>/dev/null | sort -z)
