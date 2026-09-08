#!/usr/bin/env bash
# Live review-fleet end-to-end (docs: nix run .#fleet-e2e).
#
# Proves the MULTI-REPO fleet actually works against REAL PRs: one `--serve-fleet`
# process hosts a two-row roster (two different GitHub repos), is triggered with
# `ReviewNow` for each, grounds each review against THAT row's own repo + forge
# (review-fleet multi-repo grounding, #289), and writes a redacted draft `.md` per
# PR. This is the "does it actually work" proof the hermetic in-process test
# (crates/agent-review-fleet) cannot give — it needs a real model, a real forge
# token, and the network.
#
# The generator model (Kimi by default) writes the review narrative; a stronger
# judge/verifier (GLM-5.2) is the DOCUMENTED next layer — see "Verifier" below.
#
# Nothing is ever posted to a PR: the fleet stops at `drafted` (the approve → post
# tail, inc 6c, is out of scope). The assertion surface is the draft `.md` on disk.
#
# Exit codes (same split as nix/e2e-live.nix — the two failures have different owners):
#   0 — every roster row produced a valid, redacted draft.
#   1 — HARNESS failure: a missing tool/token/endpoint, the server never came up,
#       a ReviewNow error, or NO draft was produced. Our bug (or the operator's env).
#   2 — MODEL-QUALITY failure: a draft was produced but its review narrative is
#       empty/degenerate. The harness is fine; the model underperformed.
#
# Env knobs (all optional unless marked required):
#   GITHUB_TOKEN                      (REQUIRED) — get_pr is fail-closed on an empty token.
#   AGENT_E2E_BASE_URL/_MODEL         (REQUIRED) — the generator (Kimi) OpenAI-compat endpoint.
#   AGENT_E2E_API_KEY | _API_KEY_FILE — generator key (inline or a file, e.g. the runpod kimi key).
#   AGENT_E2E_INSECURE_TLS=1          — skip TLS verify for a self-signed generator (trusted nets only).
#   AGENT_E2E_MAX_TOKENS/_CONTEXT_WINDOW — model budgets (reasoning models need more).
#   AGENT_FLEET_E2E_REPOS             — comma list of owner__name slugs (default the two real repos).
#   AGENT_FLEET_E2E_PRS               — comma list of PR numbers, 1:1 with REPOS (default 75,97).
#   AGENT_FLEET_E2E_USER              — roster owner/org (default randomizedcoder).
#   AGENT_FLEET_E2E_TIMEOUT           — seconds to wait for each draft (default 600).
#   AGENT_FLEET_E2E_PORT              — fleet gRPC TCP port (default 50186).
#   AGENT_E2E_KEEP=1                  — keep the temp workspace for inspection.
set -uo pipefail

# --- the agent binary: `agent` on PATH (nix app) or $AGENT_BIN (dev shell) ---
if [ -n "${AGENT_BIN:-}" ]; then AGENT="$(realpath "$AGENT_BIN")"; else AGENT="$(command -v agent || true)"; fi
[ -n "$AGENT" ] && [ -x "$AGENT" ] || { echo "FAIL(harness): no agent binary (put 'agent' on PATH or set AGENT_BIN)" >&2; exit 1; }
command -v grpcurl >/dev/null 2>&1 || { echo "FAIL(harness): grpcurl not on PATH" >&2; exit 1; }
command -v python3 >/dev/null 2>&1 || { echo "FAIL(harness): python3 not on PATH" >&2; exit 1; }

PORT="${AGENT_FLEET_E2E_PORT:-50186}"
# grpcurl wants flags (incl. -d) BEFORE the address, then the address, then the method.
DIAL_FLAGS=(-plaintext)
ADDR="127.0.0.1:$PORT"
DRAFT_TIMEOUT="${AGENT_FLEET_E2E_TIMEOUT:-900}"
FLEET_USER="${AGENT_FLEET_E2E_USER:-randomizedcoder}"

# --- generator (Kimi): the model the review sessions use to WRITE the narrative ---
# Defaults to the Kimi K3 dev endpoint (docs/llm-endpoints.md); RunPod proxy URLs are
# ephemeral, so override with AGENT_E2E_BASE_URL/_MODEL when the pod rotates.
GEN_BASE_URL="${AGENT_E2E_BASE_URL:-https://175ppwu9phh1r6-4000.proxy.runpod.net/v1}"
GEN_MODEL="${AGENT_E2E_MODEL:-moonshotai/Kimi-K3}"
# Key: an explicit file wins, then an inline key, then the repo-local `./kimi-api-key`
# (git-ignored) the dev setup keeps alongside the checkout.
KEY_FILE="${AGENT_E2E_API_KEY_FILE:-}"
if [ -z "$KEY_FILE" ] && [ -z "${AGENT_E2E_API_KEY:-}" ] && [ -r "./kimi-api-key" ]; then
  KEY_FILE="./kimi-api-key"
fi
if [ -n "$KEY_FILE" ]; then
  [ -r "$KEY_FILE" ] || { echo "FAIL(harness): generator key file unreadable: $KEY_FILE" >&2; exit 1; }
  GEN_KEY="$(cat "$KEY_FILE")"
else
  GEN_KEY="${AGENT_E2E_API_KEY:-}"
fi
GEN_INSECURE="${AGENT_E2E_INSECURE_TLS:-0}"
MAX_TOKENS="${AGENT_E2E_MAX_TOKENS:-4096}"
CONTEXT_WINDOW="${AGENT_E2E_CONTEXT_WINDOW:-32768}"
# A grounded review session explores the worktree with tools before concluding, so it
# needs headroom above the default loop cap or it hits max_iterations with no final answer.
MAX_ITERS="${AGENT_FLEET_E2E_MAX_ITERS:-40}"

# --- preflight (REFUSE, never skip — a skip that exits 0 reads as a pass) ---
[ -n "${GITHUB_TOKEN:-}" ] || { echo "FAIL(harness): GITHUB_TOKEN is required (forge get_pr is fail-closed on an empty token, even for public repos)" >&2; exit 1; }
if [ -z "$GEN_BASE_URL" ] || [ -z "$GEN_MODEL" ]; then
  echo "FAIL(harness): set AGENT_E2E_BASE_URL and AGENT_E2E_MODEL (the Kimi generator endpoint)" >&2
  echo "  e.g. AGENT_E2E_BASE_URL=https://<kimi-host>/v1 AGENT_E2E_MODEL=<model> \\" >&2
  echo "       AGENT_E2E_API_KEY_FILE=~/Downloads/runpod/glm/kimi-api-key GITHUB_TOKEN=... nix run .#fleet-e2e" >&2
  exit 1
fi
gopt=(-sf -m 10); [ "$GEN_INSECURE" = 1 ] && gopt+=(-k)
if ! curl "${gopt[@]}" -H "Authorization: Bearer $GEN_KEY" "$GEN_BASE_URL/models" >/dev/null 2>&1; then
  echo "FAIL(harness): generator endpoint unreachable at $GEN_BASE_URL" >&2
  echo "  a self-signed endpoint also needs AGENT_E2E_INSECURE_TLS=1." >&2
  exit 1
fi
echo "fleet-e2e: generator $GEN_MODEL at $GEN_BASE_URL"

# --- roster: one row per (repo, pr), owner = $FLEET_USER, token_ref = env:GITHUB_TOKEN ---
# Defaults review two external repos AND agent-seddon itself (drinking our own champagne —
# the fleet reviews this very repo's own PR). refs/pull/<n>/head persists after a PR merges,
# so a merged self-PR default stays valid; override with AGENT_FLEET_E2E_REPOS/_PRS.
IFS=',' read -r -a REPOS <<< "${AGENT_FLEET_E2E_REPOS:-randomizedcoder__rtl-fun,randomizedcoder__uds-rdma-proxy,randomizedcoder__agent-seddon}"
IFS=',' read -r -a PRS   <<< "${AGENT_FLEET_E2E_PRS:-75,97,290}"
[ "${#REPOS[@]}" -eq "${#PRS[@]}" ] || { echo "FAIL(harness): AGENT_FLEET_E2E_REPOS and _PRS must have the same length" >&2; exit 1; }
[ "${#REPOS[@]}" -ge 1 ] || { echo "FAIL(harness): need at least one repo/pr" >&2; exit 1; }

work="$(mktemp -d)"
if [ "${AGENT_E2E_KEEP:-0}" = 1 ]; then echo "fleet-e2e: keeping artifacts in $work"; else
  # shellcheck disable=SC2064
  trap "rm -rf '$work'" EXIT
fi
FLEET_ROOT="$work/fleet"
mkdir -p "$FLEET_ROOT"

# Roster id = the repo's name half (owner__name → name), so ReviewNow addresses it simply.
python3 - "$work/roster.json" "$FLEET_USER" "${REPOS[*]}" <<'PY'
import json, sys
out_path, user, repos = sys.argv[1], sys.argv[2], sys.argv[3].split()
rows = []
for repo in repos:
    rid = repo.split("__", 1)[1] if "__" in repo else repo
    rows.append({
        "id": rid, "user": user, "repo": repo,
        "backend": "github", "base_url": "", "token_ref": "env:GITHUB_TOKEN",
        "skill": "", "slack_trigger_channel": "", "slack_progress_channel": "",
        "poll_secs": 0, "enabled": True, "created_at": 0, "updated_at": 0,
    })
with open(out_path, "w") as f:
    json.dump(rows, f, indent=2)
PY

tls_line=""; [ "$GEN_INSECURE" = 1 ] && tls_line="insecure_tls = true"
cat > "$work/agent.toml" <<EOF
[agent]
provider    = "openai-compat"
context     = "sliding-window"
policy      = "auto-approve"
working_dir = "$work"
max_iterations = $MAX_ITERS
max_tokens     = $MAX_TOKENS
context_window = $CONTEXT_WINDOW
reserve_output = $MAX_TOKENS
stream = false
temperature = 0.0
system_prompt = "You are a code-review agent. Assess the grounded review brief (the diff and mechanized findings) and summarize concrete findings. Do not post or push anything."

[provider]
base_url    = "$GEN_BASE_URL"
model       = "$GEN_MODEL"
api_key     = "$GEN_KEY"
max_retries = 2
$tls_line

[memory]
backend       = "file"
episodic_path = "$work/.agent/episodic.jsonl"
semantic_dir  = "$work/.agent/memory"

[tools]
enabled = ["read_file", "ls"]

[search]
auto_index = false

[review]
backend = "local"

[git]
backend = "cli"

[review_fleet]
store = "file"
file  = "$work/roster.json"
root  = "$FLEET_ROOT"

# Verifier (GLM-5.2 verifies Kimi output) is the documented next layer: the llm
# verifier reuses the loop provider Verify-role-stamped, so routing GLM into the
# Verify role needs the task-router plus the route judge_from_env bridge. Left off
# in this first cut so the end-to-end draft path is what is proven here.

[telemetry]
enabled = false

[metrics]
enabled = false
EOF

# --- start the fleet server; wait for gRPC health (refuse, don't race) ---
srv_log="$work/serve-fleet.log"
( cd "$work" && GITHUB_TOKEN="$GITHUB_TOKEN" "$AGENT" --config "$work/agent.toml" --serve-fleet --listen "127.0.0.1:$PORT" ) >"$srv_log" 2>&1 &
srv_pid=$!
stop_server() { [ -n "${srv_pid:-}" ] && kill "$srv_pid" 2>/dev/null || true; }
trap 'stop_server; [ "${AGENT_E2E_KEEP:-0}" = 1 ] || rm -rf "'"$work"'"' EXIT

ready=0
for _ in $(seq 1 60); do
  if ! kill -0 "$srv_pid" 2>/dev/null; then
    echo "FAIL(harness): --serve-fleet exited during startup" >&2; tail -n 40 "$srv_log" >&2; exit 1
  fi
  if grpcurl "${DIAL_FLAGS[@]}" "$ADDR" grpc.health.v1.Health/Check >/dev/null 2>&1; then ready=1; break; fi
  sleep 0.5
done
[ "$ready" = 1 ] || { echo "FAIL(harness): fleet server never became healthy on 127.0.0.1:$PORT" >&2; tail -n 40 "$srv_log" >&2; exit 1; }
echo "fleet-e2e: --serve-fleet healthy on 127.0.0.1:$PORT"

# --- trigger a review for each row (ReviewNow over the wire) ---
for i in "${!REPOS[@]}"; do
  repo="${REPOS[$i]}"; pr="${PRS[$i]}"
  rid="${repo##*__}"
  reply="$(grpcurl "${DIAL_FLAGS[@]}" -d "{\"session_id\":\"$rid\",\"pr_number\":$pr}" \
    "$ADDR" agent.v1.ReviewFleetService/ReviewNow 2>"$work/reviewnow.$rid.err")" || {
      echo "FAIL(harness): ReviewNow($rid, #$pr) errored" >&2; cat "$work/reviewnow.$rid.err" >&2; exit 1; }
  accepted="$(printf '%s' "$reply" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("accepted", False))')"
  echo "fleet-e2e: ReviewNow($rid, #$pr) → accepted=$accepted"
done

# --- wait for a draft .md per row; assert non-empty, well-formed, redacted ---
draft_for() { # repo pr -> path or empty
  local repo="$1" pr="$2"
  find "$FLEET_ROOT" -type f -path "*/reviews/pr-${pr}-r*.md" 2>/dev/null \
    | while read -r p; do case "$p" in *"$(printf '%s' "$repo" | tr -c 'A-Za-z0-9_.' '-')"*) echo "$p";; esac; done | head -n1
}

rc=0
deadline=$((SECONDS + DRAFT_TIMEOUT))
declare -A FOUND
for i in "${!REPOS[@]}"; do FOUND[$i]=""; done
while :; do
  all=1
  for i in "${!REPOS[@]}"; do
    [ -n "${FOUND[$i]}" ] && continue
    p="$(draft_for "${REPOS[$i]}" "${PRS[$i]}")"
    if [ -n "$p" ] && [ -s "$p" ]; then FOUND[$i]="$p"; echo "fleet-e2e: draft for ${REPOS[$i]} #${PRS[$i]} → $p"; else all=0; fi
  done
  [ "$all" = 1 ] && break
  # Fast-fail: a review that reaches max_iterations / errors is terminal (one-shot ReviewNow,
  # no retry). Once every not-yet-drafted row has a failure logged, stop now instead of
  # polling to the deadline — a review that FAILED is a MODEL-QUALITY (2) outcome.
  fails="$(grep -c "review run failed (no draft)" "$srv_log" 2>/dev/null)"
  found_n=0; for i in "${!REPOS[@]}"; do [ -n "${FOUND[$i]}" ] && found_n=$((found_n + 1)); done
  if [ $((found_n + fails)) -ge "${#REPOS[@]}" ]; then
    echo "FAIL: $fails review(s) produced no draft (the model did not conclude)" >&2
    tail -n 30 "$srv_log" >&2
    echo "  raise AGENT_FLEET_E2E_MAX_ITERS / _TIMEOUT, or use a stronger generator." >&2
    if grep -q "review trigger failed" "$srv_log"; then rc=1; else rc=2; fi
    break
  fi
  if ! kill -0 "$srv_pid" 2>/dev/null; then echo "FAIL(harness): server died before all drafts landed" >&2; tail -n 40 "$srv_log" >&2; rc=1; break; fi
  if [ "$SECONDS" -ge "$deadline" ]; then
    echo "FAIL: timed out after ${DRAFT_TIMEOUT}s waiting for drafts" >&2
    tail -n 40 "$srv_log" >&2
    # Distinguish owners: if the review SESSION ran but the model never produced a final
    # answer (or the draft step failed), that is a MODEL-QUALITY (2) outcome, not a harness
    # bug (1). "review trigger failed" (fetch/worktree/infra) stays harness.
    if grep -qE "reached max_iterations|review run failed \(no draft\)" "$srv_log" \
       && ! grep -q "review trigger failed" "$srv_log"; then
      echo "  the review session RAN but the model did not conclude in time (MODEL-QUALITY)." >&2
      echo "  raise AGENT_FLEET_E2E_MAX_ITERS / _TIMEOUT, or use a stronger generator." >&2
      rc=2
    else
      rc=1
    fi
    break
  fi
  sleep 3
done

if [ "$rc" -eq 1 ]; then exit 1; fi
if [ "$rc" -eq 2 ]; then exit 2; fi

# Content + redaction checks. A present-but-degenerate draft is a MODEL-QUALITY (2)
# failure, not a harness one.
for i in "${!REPOS[@]}"; do
  p="${FOUND[$i]}"
  verdict="$(GITHUB_TOKEN="$GITHUB_TOKEN" python3 - "$p" <<'PY'
import os, re, sys
path = sys.argv[1]
text = open(path, encoding="utf-8", errors="replace").read()
tok = os.environ.get("GITHUB_TOKEN", "")
# Redaction is a HARD invariant (harness/security bug if it leaks).
if tok and tok in text:
    print("LEAK"); sys.exit(0)
# Structure the renderer always emits.
if "# Review draft" not in text or "## Review" not in text:
    print("MALFORMED"); sys.exit(0)
# The model's narrative under "## Review" must be non-trivial.
body = text.split("## Review", 1)[1]
body = re.split(r"\n## ", body, 1)[0].strip()
print("EMPTY" if len(body) < 40 else "OK")
PY
)"
  case "$verdict" in
    OK)        echo "fleet-e2e: ${REPOS[$i]} #${PRS[$i]} draft OK (redacted, well-formed)";;
    LEAK)      echo "FAIL(harness): GITHUB_TOKEN value leaked into draft $p" >&2; exit 1;;
    MALFORMED) echo "FAIL(harness): draft $p missing expected sections" >&2; exit 1;;
    EMPTY)     echo "WARN(model): draft $p has an empty/degenerate review narrative" >&2; rc=2;;
    *)         echo "FAIL(harness): unexpected draft check verdict '$verdict' for $p" >&2; exit 1;;
  esac
done

if [ "$rc" = 2 ]; then
  echo "WARN(model): drafts were produced but at least one narrative is weak — try a stronger generator." >&2
  exit 2
fi
echo "PASS: ${#REPOS[@]} roster rows each produced a valid, redacted review draft (nothing posted)."
