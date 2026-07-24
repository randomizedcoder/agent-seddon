# nix/checks/review-cochange.nix
#
# Co-change coverage for the code-review flow (Homer design input). Builds a git
# history where `handler.go` and `schema.go` habitually change together, then a
# final commit touches ONLY `handler.go` — and asserts `agent --review base..head`
# folds the coupling into the grounded context's `Historical co-change` section
# with `schema.go` foregrounded as an absent ("NOT in this diff") partner.
#
# Fully in-process: the collector mines `git log --numstat` and computes the
# coupling itself — no language toolchain, no network. So this runs in the
# hermetic `nix flake check` sandbox with just `agent` + `git`.
{
  pkgs,
  agent,
}:
pkgs.runCommand "agent-review-cochange"
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
    # Isolate the co-change collector: analyzer/callgraph need tools not on PATH here.
    analyze = false
    callgraph = false
    signatures = false
    style = false
    summaries = false
    cochange = true
    [pool]
    members = []
    TOML

    wd="$(mktemp -d)"
    cd "$wd"
    git init -q -b main
    git config user.email t@e
    git config user.name t

    # Seed all three files.
    printf 'package p\n\nfunc H() int { return 0 }\n' > handler.go
    printf 'package p\n\ntype S struct{ V int }\n' > schema.go
    printf 'package p\n\nfunc U() {}\n' > util.go
    git add -A -f && git commit -q -m seed

    # Four commits touching handler.go AND schema.go together (strong coupling).
    for i in 1 2 3 4; do
      printf 'package p\n\nfunc H() int { return %d }\n' "$i" > handler.go
      printf 'package p\n\ntype S struct{ V int; N%d int }\n' "$i" > schema.go
      git add -A -f && git commit -q -m "pair $i"
    done

    # Two independent util.go commits (noise that must NOT couple to handler.go).
    for i in 1 2; do
      printf 'package p\n\nfunc U() { _ = %d }\n' "$i" > util.go
      git add -A -f && git commit -q -m "util $i"
    done

    base="$(git rev-parse HEAD)"
    # The reviewed change: touch ONLY handler.go (schema.go is left behind).
    printf 'package p\n\nfunc H() int { return 42 }\n' > handler.go
    git add -A -f && git commit -q -m "the change under review"
    head="$(git rev-parse HEAD)"

    ctx="$(agent --config "$cfg" --review "$base..$head")"
    echo "----- generated review context (co-change) -----"
    echo "$ctx"
    echo "------------------------------------------------"

    fail() { echo "FAIL: $1" >&2; exit 1; }
    echo "$ctx" | grep -q "Grounded review facts"     || fail "no grounded facts block"
    echo "$ctx" | grep -q "Historical co-change"      || fail "no co-change section"
    echo "$ctx" | grep -q "handler.go usually changes with" \
      || fail "changed file's coupling not rendered"
    echo "$ctx" | grep -qE "schema.go .*NOT in this diff" \
      || fail "absent partner (schema.go) not foregrounded"
    # util.go moved independently → it must NOT appear as a handler.go partner.
    echo "$ctx" | grep -q "util.go" && fail "independent util.go leaked as a partner"

    echo "OK: historical co-change + absent partner folded into the review context" > "$out"
  ''
