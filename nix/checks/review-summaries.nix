# nix/checks/review-summaries.nix
#
# Fail-soft coverage for the cheap-LLM summaries collector (component 08) in the
# pool-absent case — which is the default in `nix flake check` and most CI. With
# `summaries = true` but an empty `[pool]`, the collector must **skip cleanly**: no
# `Summaries` section, and every hard fact still present. (The happy path — a pool
# producing summaries — is proven offline by the in-process `FakePool` integration
# test `crates/agent-review/tests/summaries_e2e.rs`, since the real binary has no
# offline model to dial.)
{
  pkgs,
  agent,
}:
import ../lib/mk-review-check.nix { inherit pkgs agent; } {
  name = "summaries";
  reviewConfig = ''
    analyze = false
    signatures = false
    callgraph = false
    style = false
    summaries = true
  '';
  setup = ''
    printf 'package app\n\nfunc Handle() int {\n\treturn 1\n}\n' > app.go
    git add -A -f && git commit -q -m base
    base="$(git rev-parse HEAD)"
    printf 'package app\n\nfunc Handle() int {\n\treturn 2\n}\n' > app.go
    git add -A -f && git commit -q -m head
    head="$(git rev-parse HEAD)"

    ctx="$(agent --config "$cfg" --review "$base..$head")"
    echo "----- generated review context (summaries, no pool) -----"
    echo "$ctx"
    echo "----------------------------------------------------------"
  '';
  asserts = ''
    echo "$ctx" | grep -q "Grounded review facts" || fail "no grounded facts block"
    echo "$ctx" | grep -q "app.go"                || fail "hard facts (change set) missing"
    # The soft section must be ABSENT when no pool is configured (fail-soft skip).
    echo "$ctx" | grep -q "Summaries (soft" && fail "summaries rendered despite empty pool" || true
  '';
  okMsg = "OK: summaries collector skipped fail-soft with no pool; hard facts intact";
}
