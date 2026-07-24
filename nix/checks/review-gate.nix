# nix/checks/review-gate.nix
#
# Risk synthesis + CI gate coverage for the code-review flow (Homer design input).
# Builds a Go repo where the reviewed change stacks three independent risk reasons on
# one file: it is load-bearing + single-owner (CriticalSilo, +0.35), it drops a usual
# co-change partner (+0.20), and it alters a public signature (+0.15) → ~0.70, over
# the run's 0.5 gate threshold (set with margin, off the float boundary). Asserts
# `agent --review base..head --gate`
# prints a `Risk (… GATE FAIL …)` section AND exits non-zero, while the same review
# without `--gate` exits zero.
#
# Uses the prebuilt `agent-go-ast` helper; offline + hermetic (no linter needed — the
# score is reached from call-graph + churn + co-change + signatures alone).
{
  pkgs,
  agent,
  go-ast,
}:
pkgs.runCommand "agent-review-gate"
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
    style = false
    summaries = false
    callgraph = true
    churn = true
    cochange = true
    signatures = true
    # Threshold below the stacked score (~0.70) so the gate fires with margin, not
    # on a floating-point boundary.
    gate_threshold = 0.5
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

    # Core is called by three functions (load-bearing) and co-changes with partner.go.
    printf 'package app\n\nfunc Core() int { return 0 }\n' > core.go
    cat > callers.go <<'GO'
    package app

    func A() int { return Core() }
    func B() int { return Core() }
    func C() int { return Core() }
    GO
    printf 'package app\n\nfunc P() int { return 0 }\n' > partner.go
    git add -A -f && commit_as alice seed

    # Three commits touching core.go AND partner.go together, all alice → co-change
    # coupling + single-owner history (bus factor 1).
    for i in 1 2 3; do
      printf 'package app\n\nfunc Core() int { return %d }\n' "$i" > core.go
      printf 'package app\n\nfunc P() int { return %d }\n' "$i" > partner.go
      git add -A -f && commit_as alice "pair $i"
    done

    base="$(git rev-parse HEAD)"
    # The reviewed change: alter Core's SIGNATURE (api change), touch ONLY core.go
    # (partner.go left behind → missing co-change partner).
    printf 'package app\n\nfunc Core(x int) int { return x }\n' > core.go
    git add -A -f && commit_as eve "change Core signature"
    head="$(git rev-parse HEAD)"

    # 1) Without --gate: prints the Risk section and exits zero.
    ctx="$(agent --config "$cfg" --review "$base..$head")"
    echo "----- review context (risk) -----"
    echo "$ctx"
    echo "---------------------------------"
    fail() { echo "FAIL: $1" >&2; exit 1; }
    echo "$ctx" | grep -q "Grounded review facts" || fail "no grounded facts block"
    echo "$ctx" | grep -qE "Risk .*GATE FAIL"      || fail "risk section did not reach the gate threshold"
    echo "$ctx" | grep -qE "core.go — (high|medium)" || fail "core.go not scored"

    # 2) With --gate: must exit NON-ZERO and print the gate-failure message.
    if agent --config "$cfg" --review "$base..$head" --gate > gate.out 2>&1; then
      echo "----- unexpected gate.out -----"; cat gate.out
      fail "--gate should have exited non-zero but returned 0"
    fi
    grep -q "review gate FAILED" gate.out || { cat gate.out; fail "no gate-failure message on stderr"; }

    echo "OK: stacked risk reasons cross the threshold; --gate exits non-zero" > "$out"
  ''
