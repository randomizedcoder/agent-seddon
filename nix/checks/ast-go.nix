# nix/checks/ast-go.nix
#
# Type-aware Go code-graph coverage for the AstBackend `go` engine (docs/components/
# ast.md). Runs the flake-built `agent-go-graph` helper over a tiny fixture Go module
# with an interface, two implicit implementers (value + pointer receiver), and a call
# chain, then asserts the helper resolves what syntax alone cannot:
#
#   - the implicit interface-satisfaction relation (`implements`: both implementers),
#   - a precise call edge (`main` → `run`).
#
# Fully offline + hermetic: a stdlib-only fixture means `go` resolves types locally
# with no module downloads (GOPROXY=off, GOFLAGS=-mod=mod). Needs `go-graph` + the
# pinned `go` toolchain + `jq` on PATH.
{
  pkgs,
  versions,
  go-graph,
}:
pkgs.runCommand "ast-go-check"
  {
    nativeBuildInputs = [
      go-graph
      versions.go
      pkgs.jq
    ];
  }
  ''
    set -euo pipefail
    work="$(mktemp -d)"
    cd "$work"

    # Hermetic Go env: everything under the sandbox tmp, no network.
    export HOME="$work"
    export GOPATH="$work/gopath"
    export GOCACHE="$work/gocache"
    export GOFLAGS=-mod=mod
    export GOPROXY=off
    export GO111MODULE=on

    mkdir -p greeter cmd
    cat > go.mod <<'GOMOD'
    module example.com/fx

    go 1.24
    GOMOD

    cat > greeter/greeter.go <<'GO'
    package greeter

    import "fmt"

    type Greeter interface {
    	Greet(name string) string
    }

    type Polite struct{ Prefix string }

    func (p Polite) Greet(name string) string { return p.Prefix + " " + name }

    type Loud struct{}

    func (l *Loud) Greet(name string) string { return fmt.Sprintf("%s!!!", name) }
    GO

    cat > cmd/main.go <<'GO'
    package main

    import "example.com/fx/greeter"

    func run(g greeter.Greeter) string { return g.Greet("world") }

    func main() {
    	p := greeter.Polite{Prefix: "Hi"}
    	println(run(p))
    }
    GO

    graph="$(agent-go-graph --root .)"
    echo "----- agent-go-graph output -----"
    echo "$graph" | jq .
    echo "---------------------------------"

    fail() { echo "FAIL: $1" >&2; exit 1; }

    # The Greeter interface's symbol id.
    iface_id="$(echo "$graph" | jq -r '.symbols[] | select(.name=="Greeter" and .kind=="interface") | .id')"
    [ -n "$iface_id" ] || fail "Greeter interface symbol missing"

    # Both Polite and Loud must be recorded as implementers (implicit satisfaction).
    impl_count="$(echo "$graph" | jq --argjson i "$iface_id" '[.implements[] | select(.interface_id==$i)] | length')"
    [ "$impl_count" = "2" ] || fail "expected 2 implementers of Greeter, got $impl_count"

    # A precise call edge main → run must exist.
    main_id="$(echo "$graph" | jq -r '.symbols[] | select(.name=="main") | .id')"
    run_id="$(echo "$graph" | jq -r '.symbols[] | select(.name=="run") | .id')"
    has_edge="$(echo "$graph" | jq --argjson c "$main_id" --argjson e "$run_id" 'any(.edges[]; .caller_id==$c and .callee_id==$e)')"
    [ "$has_edge" = "true" ] || fail "call edge main → run missing"

    echo "OK: implicit interface satisfaction (2 implementers) + precise call edge resolved"
    touch $out
  ''
