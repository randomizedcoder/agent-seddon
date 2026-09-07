# nix/checks/review-nearby.nix
#
# Nearby-similar coverage for the code-review flow (review-fleet increment 5, C12).
# Builds a temp git repo whose head introduces a new declaration, enables the
# read-only nearby collector, and asserts `agent --review base..head` runs the
# collector and folds a `Nearby similar code:` section into the grounded context.
#
# This proves the *wiring* deterministically: config toggle → builder → collector
# executes over the injected SearchBackend → fragment assembled → section rendered.
# The collector is fail-soft, so it emits its section (with a run summary) whether
# the background-built search index is warm, empty, or unqueryable — which is what
# keeps this hermetic. The correlation logic itself (symbol extraction, in-change
# filtering, path confinement, and turning a real hit into a finding) is covered
# deterministically by the fake-backend unit tests in
# `crates/agent-review/src/nearby.rs` (run by the default `test` check).
{
  pkgs,
  agent,
}:
import ../lib/mk-review-check.nix { inherit pkgs agent; } {
  name = "nearby";
  # Isolate the collector under test: only nearby on.
  reviewConfig = ''
    analyze = false
    signatures = false
    callgraph = false
    style = false
    summaries = false
    cochange = false
    churn = false
    nearby = true
  '';
  setup = ''
    export XDG_CACHE_HOME="$(mktemp -d)"

    # A pre-existing file (unchanged at head) — the kind of out-of-change location
    # nearby correlates a new symbol against when the index is warm.
    cat > caller.go <<'GO'
    package app

    func run() { _ = "references WidgetProcessor elsewhere" }
    GO
    git add -A -f && git commit -q -m base
    base="$(git rev-parse HEAD)"

    # Head introduces a new declaration ⇒ nearby has a symbol to correlate.
    cat > widget.go <<'GO'
    package app

    // WidgetProcessor is newly introduced by this change.
    func WidgetProcessor() int { return 42 }
    GO
    git add -A -f && git commit -q -m head
    head="$(git rev-parse HEAD)"

    ctx="$(agent --config "$cfg" --review "$base..$head")"
    echo "----- generated review context (nearby-similar) -----"
    echo "$ctx"
    echo "-----------------------------------------------------"
  '';
  asserts = ''
    echo "$ctx" | grep -q "Grounded review facts"   || fail "no grounded facts block"
    echo "$ctx" | grep -q "Nearby similar code:"    || fail "no nearby-similar section"
    echo "$ctx" | grep -q "nearby-similar"          || fail "the nearby collector did not run"
  '';
  okMsg = "OK: nearby-similar collector wired into the grounded review context";
}
