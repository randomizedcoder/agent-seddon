# nix/checks/review-churn.nix
#
# Churn/ownership coverage for the code-review flow (Homer design input). Builds a
# history where `owned.go` is authored almost entirely by one person (bus factor 1)
# and asserts `agent --review base..head` folds it into the grounded context's
# `Churn & ownership` section, foregrounding the single-owner file — WITHOUT leaking
# any author identity into the rendered text.
#
# Fully in-process: the collector mines `git log --numstat` (+ author) and computes
# bus factor itself — no toolchain, no network. Runs in the hermetic sandbox with
# just `agent` + `git`.
{
  pkgs,
  agent,
}:
pkgs.runCommand "agent-review-churn"
  {
    nativeBuildInputs = [
      agent
      pkgs.git
      pkgs.coreutils
    ];
  }
  ''
    export HOME="$(mktemp -d)"
    cfg="$HOME/agent.toml"
    cat > "$cfg" <<'TOML'
    [agent]
    provider = "openai-compat"
    policy   = "auto-approve"
    [provider]
    base_url = "http://127.0.0.1:1/v1"
    model    = "none"
    api_key  = "none"
    [memory]
    backend = "file"
    [search]
    auto_index = false
    [review]
    backend = "local"
    analyze = false
    callgraph = false
    signatures = false
    style = false
    summaries = false
    cochange = false
    churn = true
    [pool]
    members = []
    TOML

    wd="$(mktemp -d)"
    cd "$wd"
    git init -q -b main

    commit_as() {
      # $1 = author, rest = message
      author="$1"; shift
      GIT_AUTHOR_NAME="$author" GIT_AUTHOR_EMAIL="$author@e" \
      GIT_COMMITTER_NAME="$author" GIT_COMMITTER_EMAIL="$author@e" \
      git commit -q -m "$*"
    }

    printf 'package p\n\nfunc O() int { return 0 }\n' > owned.go
    git add -A -f && commit_as alice seed

    # Five alice commits to owned.go, one bob commit → alice owns >80% → bus factor 1.
    for i in 1 2 3 4; do
      printf 'package p\n\nfunc O() int { return %d }\n' "$i" > owned.go
      git add -A -f && commit_as alice "owned $i"
    done
    printf 'package p\n\nfunc O() int { return 9 }\n' > owned.go
    git add -A -f && commit_as bob "owned bob"

    base="$(git rev-parse HEAD)"
    # The reviewed change: touch owned.go.
    printf 'package p\n\nfunc O() int { return 42 }\n' > owned.go
    git add -A -f && commit_as eve "the change under review"
    head="$(git rev-parse HEAD)"

    ctx="$(agent --config "$cfg" --review "$base..$head")"
    echo "----- generated review context (churn) -----"
    echo "$ctx"
    echo "--------------------------------------------"

    fail() { echo "FAIL: $1" >&2; exit 1; }
    echo "$ctx" | grep -q "Grounded review facts" || fail "no grounded facts block"
    echo "$ctx" | grep -q "Churn & ownership"      || fail "no churn section"
    echo "$ctx" | grep -qE "owned.go .*single-owner" \
      || fail "single-owner file not foregrounded"
    # No author identity may leak into the rendered context.
    echo "$ctx" | grep -qi "alice" && fail "author identity leaked into context"

    echo "OK: bus-factor / single-owner folded into the review context (no author leak)" > "$out"
  ''
