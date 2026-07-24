# nix/checks/review-recording.nix
#
# Recording coverage for the code-review flow (component 09). Runs `agent --review`
# with the file memory backend and asserts the run's `ReviewRecord` lands in
# `episodic.jsonl` — the durable fallback that stands in for ClickHouse when
# telemetry is off (exactly how the verifier's recording is tested). Proves the
# whole append path: `Agent::record_review` → `MemoryEvent{kind:"review"}` →
# `FileEpisodic` → serialized row with hashes/counts, no raw content.
#
# Offline + hermetic — only `agent` + `git` + `jq`.
{
  pkgs,
  agent,
}:
pkgs.runCommand "agent-review-recording"
  {
    nativeBuildInputs = [
      agent
      pkgs.git
      pkgs.jq
      pkgs.coreutils
    ];
  }
  ''
    export HOME="$(mktemp -d)"
    epi="$HOME/.agent/episodic.jsonl"
    cfg="$HOME/agent.toml"
    cat > "$cfg" <<TOML
    [agent]
    provider = "openai-compat"
    policy   = "auto-approve"
    [provider]
    base_url = "http://127.0.0.1:1/v1"
    model    = "none"
    api_key  = "none"
    [memory]
    backend = "file"
    episodic_path = "$epi"
    [search]
    auto_index = false
    [review]
    backend = "local"
    # Collectors off — the record is built from the base facts regardless; keep fast.
    analyze = false
    signatures = false
    callgraph = false
    style = false
    summaries = false
    [pool]
    members = []
    TOML

    wd="$(mktemp -d)"
    cd "$wd"
    git init -q -b main
    git config user.email t@e
    git config user.name t
    printf 'package app\nfunc A() int { return 1 }\n' > a.go
    git add -A -f && git commit -q -m base
    base="$(git rev-parse HEAD)"
    printf 'package app\nfunc A() int { return 2 }\nfunc B() {}\n' > a.go
    git add -A -f && git commit -q -m head
    head="$(git rev-parse HEAD)"

    agent --config "$cfg" --review "$base..$head" >/dev/null

    fail() { echo "FAIL: $1" >&2; exit 1; }
    [ -f "$epi" ] || fail "no episodic.jsonl written"
    rec="$(jq -c 'select(.kind == "review") | .review' "$epi" | head -1)"
    echo "----- recorded ReviewRecord -----"
    echo "$rec"
    echo "---------------------------------"
    [ -n "$rec" ] || fail "no review record in episodic.jsonl"

    echo "$rec" | jq -e '.mode_via == "explicit"'     >/dev/null || fail "mode_via != explicit"
    echo "$rec" | jq -e '.project == "go"'            >/dev/null || fail "project != go"
    echo "$rec" | jq -e '.changed_files == 1'         >/dev/null || fail "changed_files != 1"
    echo "$rec" | jq -e '.repo_hash | length > 0'     >/dev/null || fail "repo_hash empty"
    echo "$rec" | jq -e '(.collectors | length) >= 1' >/dev/null || fail "no per-collector statuses"
    # Security: the record must carry no raw source / URL — only hashes/counts/revs.
    echo "$rec" | jq -e 'has("repo_url") | not'       >/dev/null || fail "record leaked a URL field"

    echo "OK: review run recorded to episodic.jsonl (hashes/counts only)" > "$out"
  ''
