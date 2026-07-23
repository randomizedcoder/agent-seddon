# nix/checks/review-callgraph.nix
#
# Call-graph (blast-radius) coverage for the code-review flow (component 06).
# Reconstructs a two-commit Go history where `Caller` calls `Target` and `Target`
# changes between base and head, then asserts `agent --review base..head` runs the
# flake-built `agent-go-ast` helper and folds the graph into the grounded context's
# `Call graph` section — surfacing `Caller` as a caller of the changed `Target`.
#
# Fully offline + hermetic: the helper is a prebuilt stdlib-only Go binary (no Go
# toolchain or network needed at run time), so this needs only `agent` + `go-ast` +
# `git` on PATH.
{
  pkgs,
  agent,
  go-ast,
}:
pkgs.runCommand "agent-review-callgraph"
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
    # Isolate the call-graph collector.
    analyze = false
    signatures = false
    callgraph = true
    [pool]
    members = []
    TOML

    wd="$(mktemp -d)"
    cd "$wd"
    git init -q -b main
    git config user.email t@e
    git config user.name t

    # A caller in one file, the callee in another.
    cat > caller.go <<'GO'
    package app

    func Caller() int {
    	return Target()
    }
    GO
    cat > target.go <<'GO'
    package app

    func Target() int {
    	return 1
    }
    GO
    git add -A -f && git commit -q -m base
    base="$(git rev-parse HEAD)"

    # Head: Target changes (so the diff marks it a changed fn / blast-radius root).
    cat > target.go <<'GO'
    package app

    func Target() int {
    	return 2
    }
    GO
    git add -A -f && git commit -q -m head
    head="$(git rev-parse HEAD)"

    ctx="$(agent --config "$cfg" --review "$base..$head")"
    echo "----- generated review context (call graph) -----"
    echo "$ctx"
    echo "-------------------------------------------------"

    fail() { echo "FAIL: $1" >&2; exit 1; }
    echo "$ctx" | grep -q "Grounded review facts"      || fail "no grounded facts block"
    echo "$ctx" | grep -q "Call graph"                 || fail "no call-graph section"
    echo "$ctx" | grep -qE "Target .*called by .*Caller" \
      || fail "blast radius did not surface Caller → Target"

    echo "OK: call graph (blast radius) folded into the grounded review context" > "$out"
  ''
