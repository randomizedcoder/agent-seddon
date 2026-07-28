#!/usr/bin/env bash
# Concurrent multi-session e2e (docs: nix run .#e2e-multi).
#
# Launches N agent sessions AT THE SAME TIME. Each is asked to write a small program
# — hello-world or FizzBuzz, round-robin across C, Go, and Rust — using the agent's
# tools. The harness then compiles and runs each result, and asks a strong external
# judge model (GLM-5.2 by default) to evaluate correctness as the FINAL check.
#
# This exercises the multi-session machinery under real concurrent load (10 distinct
# sessions, each its own working dir + identity), the three language toolchains, and
# an independent model-graded quality gate.
#
# Exit codes (same contract as nix/e2e-live.nix):
#   0 — every session passed (compiled + judged correct).
#   1 — a HARNESS failure (agent crashed/timed out, a toolchain or endpoint was missing).
#   2 — MODEL-QUALITY failures only (the agent ran, but produced wrong/uncompilable code,
#       or the judge rejected it).
set -uo pipefail

# --- the agent binary: `agent` on PATH (nix app) or $AGENT_BIN (dev shell) ---
if [ -n "${AGENT_BIN:-}" ]; then AGENT="$(realpath "$AGENT_BIN")"; else AGENT="$(command -v agent || true)"; fi
[ -n "$AGENT" ] && [ -x "$AGENT" ] || { echo "FAIL(harness): no agent binary (put 'agent' on PATH or set AGENT_BIN)" >&2; exit 1; }

N="${AGENT_E2E_SESSIONS:-10}"
PER_TIMEOUT="${AGENT_E2E_TIMEOUT:-360}"

# --- generator: the model the agent uses to WRITE the code ---
GEN_BASE_URL="${AGENT_E2E_BASE_URL:-http://localhost:11434/v1}"
# Default llama3.1: among common local models it is the one that reliably emits
# STRUCTURED tool calls through ollama's OpenAI-compat endpoint. Strong coders like
# qwen2.5-coder emit the call as plain text (no file gets written); swap one in via
# AGENT_E2E_MODEL once its ollama tool template is fixed.
GEN_MODEL="${AGENT_E2E_MODEL:-llama3.1:latest}"
GEN_API_KEY="${AGENT_E2E_API_KEY:-ollama}"

# --- judge: the model that EVALUATES the produced code (the final check) ---
# Defaults to the GLM-5.2 dev endpoint; fully overridable. If it is unreachable or the
# key is missing, the harness warns and falls back to a deterministic output match.
JUDGE_ENABLED="${AGENT_E2E_JUDGE:-1}"
JUDGE_BASE_URL="${AGENT_E2E_JUDGE_BASE_URL:-https://213.173.96.56:8000/v1}"
JUDGE_MODEL="${AGENT_E2E_JUDGE_MODEL:-/model}"
JUDGE_KEY_FILE="${AGENT_E2E_JUDGE_API_KEY_FILE:-$HOME/Downloads/runpod/glm/glm-api-key}"
JUDGE_INSECURE="${AGENT_E2E_JUDGE_INSECURE_TLS:-1}"

# --- preflight: the generator endpoint MUST be reachable (refuse, don't skip) ---
gopt=(-sf -m 10)
if ! curl "${gopt[@]}" -H "Authorization: Bearer $GEN_API_KEY" "$GEN_BASE_URL/models" >/dev/null 2>&1 \
   && ! curl "${gopt[@]}" "${GEN_BASE_URL%/v1}/api/tags" >/dev/null 2>&1; then
  echo "FAIL(harness): no generator model server at $GEN_BASE_URL" >&2
  echo "  start one:  ollama serve && ollama pull $GEN_MODEL" >&2
  exit 1
fi

# --- preflight: judge (soft — degrade to deterministic checking if unavailable) ---
if [ "$JUDGE_ENABLED" = 1 ]; then
  jopt=(-sf -m 20); [ "$JUDGE_INSECURE" = 1 ] && jopt+=(-k)
  if [ ! -r "$JUDGE_KEY_FILE" ]; then
    echo "WARN: judge key $JUDGE_KEY_FILE unreadable — judging DISABLED (deterministic checks only)" >&2
    JUDGE_ENABLED=0
  elif ! curl "${jopt[@]}" "$JUDGE_BASE_URL/models" -H "Authorization: Bearer $(cat "$JUDGE_KEY_FILE")" >/dev/null 2>&1; then
    echo "WARN: judge endpoint $JUDGE_BASE_URL unreachable — judging DISABLED (deterministic checks only)" >&2
    JUDGE_ENABLED=0
  fi
fi

ROOT="$(mktemp -d)"; [ "${AGENT_E2E_KEEP:-0}" = 1 ] || trap 'rm -rf "$ROOT"' EXIT
[ "${AGENT_E2E_KEEP:-0}" = 1 ] && echo "e2e-multi: keeping artifacts in $ROOT"

LANGS=(c go rust); TASKS=(hello fizzbuzz)
COMBOS=(); for t in "${TASKS[@]}"; do for l in "${LANGS[@]}"; do COMBOS+=("$l:$t"); done; done
NCOMBO=${#COMBOS[@]}

ext()  { case "$1" in c) echo c;; go) echo go;; rust) echo rs;; esac; }
label(){ case "$1" in c) echo C;; go) echo Go;; rust) echo Rust;; esac; }

expected_of() {
  case "$1" in
    hello) printf 'Hello, World!';;
    fizzbuzz) printf '1\n2\nFizz\n4\nBuzz\nFizz\n7\n8\nFizz\nBuzz\n11\nFizz\n13\n14\nFizzBuzz';;
  esac
}
goal_of() {
  local lang="$1" task="$2" fname="$3"
  case "$task" in
    hello) echo "Using your tools, create a file named $fname in the current directory containing a complete, compilable $(label "$lang") program. When run it must print exactly the text Hello, World! and nothing else — no leading or trailing spaces — followed by a single newline. Create only that one file; do not run it.";;
    fizzbuzz) echo "Using your tools, create a file named $fname in the current directory containing a complete, compilable $(label "$lang") program. When run it must print the integers 1 through 15, one per line with no extra spaces, except: for multiples of 3 print Fizz, for multiples of 5 print Buzz, and for multiples of both 3 and 5 print FizzBuzz. Create only that one file; do not run it.";;
  esac
}

# --- phase 1: launch N sessions concurrently ---
run_session() {
  local i="$1" combo="${COMBOS[$((i % NCOMBO))]}"
  local lang="${combo%%:*}" task="${combo##*:}"
  local d="$ROOT/s$i"; mkdir -p "$d/.agent"
  echo "$lang" > "$d/lang"; echo "$task" > "$d/task"
  local fname="solution.$(ext "$lang")"; echo "$fname" > "$d/fname"
  cat > "$d/agent.toml" <<EOF
[agent]
provider = "openai-compat"
context  = "sliding-window"
policy   = "auto-approve"
working_dir = "$d"
max_iterations = 8
max_tokens = 2048
context_window = 16384
reserve_output = 2048
stream = false
temperature = 0.0
system_prompt = "You are a coding agent. To create a file you MUST invoke the write_file tool as a real structured tool call — never print the tool call as text in your reply. Write correct, complete, compilable programs. When done, reply with a one-line summary."

[provider]
base_url = "$GEN_BASE_URL"
model    = "$GEN_MODEL"
api_key  = "$GEN_API_KEY"
max_retries = 2

[memory]
backend       = "file"
episodic_path = "$d/.agent/episodic.jsonl"
semantic_dir  = "$d/.agent/memory"

[tools]
enabled = ["read_file", "write_file", "edit", "ls"]

[search]
auto_index = false

[metrics]
enabled = false
EOF
  local goal; goal=$(goal_of "$lang" "$task" "$fname")
  timeout "$PER_TIMEOUT" bash -c "cd '$d' && '$AGENT' --config '$d/agent.toml' \"\$1\"" _ "$goal" \
    > "$d/agent.out" 2> "$d/agent.err"
  echo "$?" > "$d/rc"
}

echo "e2e-multi: launching $N concurrent sessions"
echo "  generator: $GEN_MODEL at $GEN_BASE_URL"
echo "  judge:     $JUDGE_MODEL at $JUDGE_BASE_URL (enabled=$JUDGE_ENABLED)"
start=$(date +%s)
for i in $(seq 0 $((N-1))); do run_session "$i" & done
wait
elapsed=$(( $(date +%s) - start ))
echo "e2e-multi: all $N sessions finished in ${elapsed}s"
echo

# --- phase 2: compile + run + judge each ---
harness_fail=0; model_fail=0; passed=0
printf '%-4s %-5s %-9s %-8s %-8s %-6s %s\n' "SESS" "LANG" "TASK" "AGENT" "COMPILE" "OUTPUT" "GLM"
printf '%s\n' "----------------------------------------------------------------------"

judge() { # task_desc source ran prog_out  -> prints one VERDICT line
  [ "$JUDGE_ENABLED" = 1 ] || { echo "VERDICT: SKIP (judge off)"; return; }
  local sys="You are a strict code reviewer. Given a programming TASK, a candidate SOURCE file, and the program's actual runtime OUTPUT, decide if the candidate CORRECTLY and COMPLETELY solves the task. Judge the output exactly. Reply with ONE line only, starting with 'VERDICT: PASS' or 'VERDICT: FAIL', followed by a brief reason."
  local body; body=$(jq -n --arg m "$JUDGE_MODEL" --arg s "$sys" \
    --arg u "TASK:
$1

SOURCE:
$2

COMPILED_AND_RAN: $3
PROGRAM_OUTPUT:
$4" '{model:$m,temperature:0,max_tokens:8000,messages:[{role:"system",content:$s},{role:"user",content:$u}]}')
  local opts=(-sf -m 180); [ "$JUDGE_INSECURE" = 1 ] && opts+=(-k)
  local resp; resp=$(curl "${opts[@]}" "$JUDGE_BASE_URL/chat/completions" \
    -H "Authorization: Bearer $(cat "$JUDGE_KEY_FILE")" -H 'Content-Type: application/json' -d "$body" 2>/dev/null)
  local content; content=$(printf '%s' "$resp" | jq -r '.choices[0].message.content // ""' 2>/dev/null)
  if [ -n "$content" ]; then
    printf '%s\n' "$content" | grep -iE 'VERDICT' | head -1 \
      || printf '%s\n' "$content" | grep -vE '^[[:space:]]*$' | head -1
  else
    echo "VERDICT: ERROR (no judge response)"
  fi
}

for i in $(seq 0 $((N-1))); do
  d="$ROOT/s$i"; lang=$(cat "$d/lang"); task=$(cat "$d/task"); fname=$(cat "$d/fname")
  rc=$(cat "$d/rc" 2>/dev/null || echo 99)
  agent_st="ok"; comp_st="-"; out_st="-"; glm="-"; src=""; prog_out=""; ran="no"

  # rc != 0 (crash/timeout) is a harness/infra failure; rc == 0 with no file is a MODEL
  # failure — it answered in prose or emitted the tool call as text instead of calling it.
  if [ "$rc" != 0 ]; then agent_st="EXIT$rc"; harness_fail=$((harness_fail+1));
  elif [ ! -f "$d/$fname" ]; then agent_st="nofile"; model_fail=$((model_fail+1));
  else
    src=$(cat "$d/$fname")
    case "$lang" in
      c)    ( cd "$d" && cc "$fname" -o prog ) 2>"$d/cc.err" && comp_st="ok" || comp_st="FAIL";;
      rust) ( cd "$d" && rustc "$fname" -o prog ) 2>"$d/cc.err" && comp_st="ok" || comp_st="FAIL";;
      go)   ( cd "$d" && printf 'module sol\ngo 1.20\n' > go.mod && GOCACHE="$d/.gocache" go build -o prog "$fname" ) 2>"$d/cc.err" && comp_st="ok" || comp_st="FAIL";;
    esac
    if [ "$comp_st" = ok ]; then
      prog_out=$(cd "$d" && timeout 15 ./prog 2>/dev/null); ran="yes"
      exp=$(expected_of "$task")
      # informational pre-signal only (GLM is authoritative): exact, allowing one
      # trailing newline. NOT lenient about stray spaces — those are real bugs.
      if [ "$prog_out" = "$exp" ] || [ "$prog_out" = "$exp"$'\n' ]; then out_st="match"; else out_st="DIFF"; fi
    fi
  fi

  if [ -n "$src" ]; then
    verdict=$(judge "$(goal_of "$lang" "$task" "$fname")" "$src" "$ran" "$prog_out")
    echo "$verdict" > "$d/verdict"
    if printf '%s' "$verdict" | grep -qi 'PASS'; then glm="PASS";
    elif printf '%s' "$verdict" | grep -qi 'FAIL'; then glm="FAIL";
    elif printf '%s' "$verdict" | grep -qi 'SKIP'; then glm="off";
    else glm="ERR"; fi
  fi

  # GLM-5.2 is the authoritative final check: pass = produced + compiled + GLM PASS
  # (or, when the judge is off, the deterministic output match).
  if [ "$agent_st" = ok ]; then
    if [ "$comp_st" = ok ] && { { [ "$JUDGE_ENABLED" = 1 ] && [ "$glm" = PASS ]; } \
       || { [ "$JUDGE_ENABLED" != 1 ] && [ "$out_st" = match ]; }; }; then
      passed=$((passed+1))
    else
      model_fail=$((model_fail+1))
    fi
  fi

  printf '%-4s %-5s %-9s %-8s %-8s %-6s %s\n' "s$i" "$lang" "$task" "$agent_st" "$comp_st" "$out_st" "$glm"
done

echo
echo "=== e2e-multi summary ==="
echo "  sessions: $N   wall-clock: ${elapsed}s   generator: $GEN_MODEL   judge: $([ "$JUDGE_ENABLED" = 1 ] && echo "$JUDGE_MODEL" || echo off)"
echo "  passed (compiled + judged correct):            $passed / $N"
echo "  harness failures (agent crash/timeout):        $harness_fail"
echo "  model-quality failures (bad code / judge FAIL): $model_fail"
if [ "$harness_fail" -gt 0 ]; then echo "RESULT: HARNESS FAILURE"; exit 1;
elif [ "$passed" -eq "$N" ]; then echo "RESULT: ALL PASS"; exit 0;
else echo "RESULT: model-quality issues only (harness OK)"; exit 2; fi
