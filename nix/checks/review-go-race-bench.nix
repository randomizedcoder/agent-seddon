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
# `nix flake check` sandbox. The bench finding is deterministic and asserted; the
# race *detection* is best-effort (not asserted — the detector is probabilistic),
# but the `-race` run itself is asserted to have executed.
{
  pkgs,
  versions,
  agent,
}:
import ../lib/mk-review-check.nix { inherit pkgs agent; } {
  name = "go-race-bench";
  extraInputs = [
    versions.go
    pkgs.gcc # `go test -race` needs cgo + a C compiler
  ];
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
    echo "$ctx" | grep -q "go test -race"             || fail "the -race run was not recorded"
    echo "$ctx" | grep -q "go test -bench"            || fail "the -bench run was not recorded"
    echo "$ctx" | grep -q "ns/op"                     || fail "the benchmark result was not surfaced"
    echo "$ctx" | grep -q "BenchmarkSum"              || fail "the benchmark name was not surfaced"
  '';
  okMsg = "OK: go test -race + -bench results folded into the grounded review context";
}
