# nix/checks/review-runtime-path.nix
#
# The wrapped `agent` must run its review collectors from its OWN `PATH`, as a
# systemd service does (`services.agentSeddon`: no `bash`, no dev tools). The other
# review checks run inside a stdenv builder, whose `PATH` always has `bash` and the
# helpers on it, so they passed while the l2 fleet reported every call graph as
# `agent-go-ast not found on PATH`: the Sandbox runs `bash -c`, and `bash` was not
# on the wrapper's `PATH` (exit 127).
#
# Same history as review-callgraph, but the review runs under `env -i`, so only the
# wrapper's `--prefix PATH` is there. `go-ast` is deliberately NOT an input.
{
  pkgs,
  agent,
}:
import ../lib/mk-review-check.nix { inherit pkgs agent; } {
  name = "runtime-path";
  reviewConfig = ''
    analyze = false
    signatures = false
    callgraph = true
  '';
  setup = ''
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
    sed -i 's/return 1/return 2/' target.go
    git add -A -f && git commit -q -m head
    head="$(git rev-parse HEAD)"

    # No inherited PATH: the wrapper's own runtime PATH is all the agent gets.
    ctx="$(${pkgs.coreutils}/bin/env -i HOME="$HOME" ${agent}/bin/agent \
      --config "$cfg" --review "$base..$head" 2>&1)"
    echo "----- review context under env -i -----"
    echo "$ctx"
    echo "---------------------------------------"
  '';
  asserts = ''
    echo "$ctx" | grep -q "not found on PATH" \
      && fail "a collector could not find its tool on the wrapped agent's PATH"
    echo "$ctx" | grep -qE "Target .*called by .*Caller" \
      || fail "call graph did not run from the wrapped agent's own PATH"
  '';
  okMsg = "OK: the wrapped agent runs its review helpers from its own PATH";
}
