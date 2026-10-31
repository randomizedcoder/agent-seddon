# nix/checks/review-go-race-bench.nix
#
# Go race+bench coverage for the code-review flow (review-fleet increment 5, C12).
# Builds a temp git repo whose head adds a stdlib-only Go module with a benchmark
# (deterministic `ns/op`) and a data-race test, then asserts `agent --review
# base..head` runs `go test -race` + `go test -bench` on the changed package under
# the sandbox (network off) and folds the results into the grounded context's
# `Go race & benchmarks:` section.
#
# Offline + deterministic: the module imports only the stdlib (`testing`, `sync`),
# so `go test` needs no module download (GOPROXY=off) and runs in the hermetic
# `nix flake check` sandbox. Both results are asserted, not just that the runs were
# attempted: each run is recorded whatever its outcome, so grepping for the command
# name could never fail (the #665 review). The race detector works on happens-before,
# not timing, so four sibling goroutines writing `x` with no synchronisation is
# reported on every run, even on one CPU. A `-race` run that could not build (no C
# compiler for cgo) is recorded as `failed`, so `ok` with a finding proves it ran.
{
  pkgs,
  agent,
}:
import ../lib/mk-review-check.nix { inherit pkgs agent; } {
  name = "go-race-bench";
  # Isolate the collector under test: only go_checks on.
  reviewConfig = ''
    analyze = false
    signatures = false
    callgraph = false
    style = false
    summaries = false
    cochange = false
    churn = false
    go_checks = true
    go_checks_timeout_secs = 240
  '';
  setup = ''
    export GOCACHE="$(mktemp -d)"
    export GOPATH="$(mktemp -d)"
    export XDG_CACHE_HOME="$(mktemp -d)"
    export GOPROXY=off
    export GOFLAGS=-mod=mod
    export GOSUMDB=off
    export CGO_ENABLED=1

    git commit -q --allow-empty -m base
    base="$(git rev-parse HEAD)"

    cat > go.mod <<'GOMOD'
    module example.com/review

    go 1.21
    GOMOD

    cat > add.go <<'GO'
    package review

    // Sum adds the first n integers.
    func Sum(n int) int {
    	s := 0
    	for i := 0; i < n; i++ {
    		s += i
    	}
    	return s
    }
    GO

    cat > add_test.go <<'GO'
    package review

    import (
    	"sync"
    	"testing"
    )

    func BenchmarkSum(b *testing.B) {
    	for i := 0; i < b.N; i++ {
    		_ = Sum(1000)
    	}
    }

    // A deliberate data race for the -race run to exercise.
    func TestDataRace(t *testing.T) {
    	x := 0
    	var wg sync.WaitGroup
    	for i := 0; i < 4; i++ {
    		wg.Add(1)
    		go func() {
    			defer wg.Done()
    			for j := 0; j < 50000; j++ {
    				x++
    			}
    		}()
    	}
    	wg.Wait()
    	_ = x
    }
    GO

    git add -A -f && git commit -q -m head
    head="$(git rev-parse HEAD)"

    ctx="$(agent --config "$cfg" --review "$base..$head")"
    echo "----- generated review context (go race+bench) -----"
    echo "$ctx"
    echo "----------------------------------------------------"
  '';
  asserts = ''
    echo "$ctx" | grep -q "Grounded review facts"     || fail "no grounded facts block"
    echo "$ctx" | grep -q "Go race & benchmarks:"     || fail "no go-checks section"
    echo "$ctx" | grep -qE "go test -race: ok \([1-9][0-9]* finding"  || fail "the -race run did not build and report the data race"
    echo "$ctx" | grep -qE "go test -bench: ok \([1-9][0-9]* finding" || fail "the -bench run did not build and report a benchmark"
    echo "$ctx" | grep -q "ns/op"                     || fail "the benchmark result was not surfaced"
    echo "$ctx" | grep -q "BenchmarkSum"              || fail "the benchmark name was not surfaced"
  '';
  okMsg = "OK: go test -race found the data race and -bench reported ns/op in the grounded review context";
}
