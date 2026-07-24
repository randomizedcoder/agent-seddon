# nix/checks/review-salience.nix
#
# Salience / blast-radius coverage for the code-review flow (Homer design input).
# Reconstructs a Go repo where `Core` is called by three other functions (so it is
# load-bearing / high-centrality) and is authored entirely by one person (bus factor
# 1). The reviewed change touches `Core`, and we assert `agent --review base..head`
# folds the call-graph centrality + churn ownership into a `Salience` verdict of
# `CriticalSilo` (load-bearing AND single-owner) — the highest-risk class.
#
# Exercises the post-fan-out synthesis (call graph × churn). Offline + hermetic:
# the prebuilt `agent-go-ast` helper + `git` on PATH; no Go toolchain, no network.
{
  pkgs,
  agent,
  go-ast,
}:
pkgs.runCommand "agent-review-salience"
  {
    nativeBuildInputs = [
      agent
      go-ast
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
    signatures = false
    style = false
    summaries = false
    cochange = false
    callgraph = true
    churn = true
    [pool]
    members = []
    TOML

    wd="$(mktemp -d)"
    cd "$wd"
    git init -q -b main

    commit_as() { author="$1"; shift
      GIT_AUTHOR_NAME="$author" GIT_AUTHOR_EMAIL="$author@e" \
      GIT_COMMITTER_NAME="$author" GIT_COMMITTER_EMAIL="$author@e" \
      git commit -q -m "$*"; }

    # Core is called by three other functions → load-bearing / high centrality.
    cat > core.go <<'GO'
    package app

    func Core() int { return 0 }
    GO
    cat > callers.go <<'GO'
    package app

    func A() int { return Core() }
    func B() int { return Core() }
    func C() int { return Core() }
    GO
    git add -A -f && commit_as alice seed

    # A few more alice-only commits to core.go → single-owner history (bus factor 1).
    for i in 1 2 3; do
      printf 'package app\n\nfunc Core() int { return %d }\n' "$i" > core.go
      git add -A -f && commit_as alice "core $i"
    done

    base="$(git rev-parse HEAD)"
    # The reviewed change touches Core (the load-bearing, single-owner function).
    printf 'package app\n\nfunc Core() int { return 42 }\n' > core.go
    git add -A -f && commit_as eve "the change under review"
    head="$(git rev-parse HEAD)"

    ctx="$(agent --config "$cfg" --review "$base..$head")"
    echo "----- generated review context (salience) -----"
    echo "$ctx"
    echo "-----------------------------------------------"

    fail() { echo "FAIL: $1" >&2; exit 1; }
    echo "$ctx" | grep -q "Grounded review facts" || fail "no grounded facts block"
    echo "$ctx" | grep -q "Salience"              || fail "no salience section"
    echo "$ctx" | grep -qE "core.go .*CriticalSilo" \
      || fail "load-bearing single-owner change not classified CriticalSilo"

    echo "OK: call-graph centrality × churn ownership → CriticalSilo salience verdict" > "$out"
  ''
