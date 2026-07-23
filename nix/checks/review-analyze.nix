# nix/checks/review-analyze.nix
#
# Static-analysis coverage for the code-review flow (increment 5). Builds a tiny,
# self-contained, stdlib-only Go module with a deliberate lint hit, commits it into
# a temp git repo (base = empty tree, head = the module), and asserts
# `agent --review base..head` runs golangci-lint on the changed package and folds
# the finding into the grounded context's `Analysis (static):` section.
#
# Offline + deterministic: the module imports only the stdlib, so golangci-lint's
# typecheck needs no module download (GOPROXY=off, caches in temp) — it runs in the
# hermetic `nix flake check` sandbox. clippy is exercised live (dev shell + eval);
# reproducing it here would need the whole vendored Rust build tree.
{
  pkgs,
  versions,
  agent,
}:
pkgs.runCommand "agent-review-analyze"
  {
    nativeBuildInputs = [
      agent
      pkgs.git
      pkgs.coreutils
      versions.go
      versions.golangci-lint
    ];
  }
  ''
    export HOME="$(mktemp -d)"
    # golangci-lint drives the go toolchain + its own cache; point every cache at a
    # writable temp dir and disable the network so the run is fully offline.
    export GOCACHE="$(mktemp -d)"
    export GOPATH="$(mktemp -d)"
    export GOLANGCI_LINT_CACHE="$(mktemp -d)"
    export XDG_CACHE_HOME="$(mktemp -d)"
    export GOPROXY=off
    export GOFLAGS=-mod=mod
    export GOSUMDB=off

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
    analyze = true
    analyze_timeout_secs = 120
    [pool]
    members = []
    TOML

    wd="$(mktemp -d)"
    cd "$wd"
    git init -q -b main
    git config user.email t@e
    git config user.name t
    # Base: an empty commit, so the whole module lands in the diff (⇒ scoped).
    git commit -q --allow-empty -m base
    base="$(git rev-parse HEAD)"

    # A stdlib-only module with an ineffectual assignment (`x := 1` overwritten
    # before use) — reliably flagged by golangci-lint's default `ineffassign`.
    cat > go.mod <<'GOMOD'
    module example.com/review

    go 1.21
    GOMOD
    cat > compute.go <<'GO'
    package review

    // Compute has an ineffectual assignment ineffassign flags.
    func Compute() int {
    	x := 1
    	x = 2
    	return x
    }
    GO
    git add -A -f && git commit -q -m head
    head="$(git rev-parse HEAD)"

    ctx="$(agent --config "$cfg" --review "$base..$head")"
    echo "----- generated review context (Go static analysis) -----"
    echo "$ctx"
    echo "----------------------------------------------------------"

    fail() { echo "FAIL: $1" >&2; exit 1; }
    echo "$ctx" | grep -q "Grounded review facts"  || fail "no grounded facts block"
    echo "$ctx" | grep -q "Analysis (static):"     || fail "no static-analysis section"
    echo "$ctx" | grep -q "golangci-lint: ok"      || fail "golangci-lint did not run cleanly"
    echo "$ctx" | grep -q "ineffassign"            || fail "the deliberate lint hit was not surfaced"
    echo "$ctx" | grep -q "compute.go"             || fail "finding not tied to the changed file"

    echo "OK: golangci-lint finding folded into the grounded review context" > "$out"
  ''
