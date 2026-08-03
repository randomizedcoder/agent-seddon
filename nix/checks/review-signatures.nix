# nix/checks/review-signatures.nix
#
# Signature-diff coverage for the code-review flow (increment 6). Reconstructs a
# two-commit history where one Go function's signature changes (a param is added)
# and one new function appears, then asserts `agent --review base..head` folds the
# change into the grounded context's `API signature changes` section.
#
# Fully in-process: the collector reads the base/head blobs and regex-scans them —
# no language toolchain, no network. So this runs in the hermetic `nix flake check`
# sandbox with just `agent` + `git`.
{
  pkgs,
  agent,
}:
import ../lib/mk-review-check.nix { inherit pkgs agent; } {
  name = "signatures";
  reviewConfig = ''
    # Isolate the signature collector: the analyzer needs linters not on PATH here.
    analyze = false
    signatures = true
  '';
  setup = ''
    # Base: two stdlib-free functions.
    cat > math.go <<'GO'
    package math

    func Add(a int) int {
    	return a
    }

    func Keep() bool {
    	return true
    }
    GO
    git add -A -f && git commit -q -m base
    base="$(git rev-parse HEAD)"

    # Head: Add gains a parameter (signature MODIFIED); Sub is new (ADDED); Keep is
    # untouched (must NOT appear).
    cat > math.go <<'GO'
    package math

    func Add(a, b int) int {
    	return a + b
    }

    func Sub(a, b int) int {
    	return a - b
    }

    func Keep() bool {
    	return true
    }
    GO
    git add -A -f && git commit -q -m head
    head="$(git rev-parse HEAD)"

    ctx="$(agent --config "$cfg" --review "$base..$head")"
    echo "----- generated review context (signature diff) -----"
    echo "$ctx"
    echo "------------------------------------------------------"
  '';
  asserts = ''
    echo "$ctx" | grep -q "Grounded review facts"          || fail "no grounded facts block"
    echo "$ctx" | grep -q "API signature changes"          || fail "no signature-diff section"
    echo "$ctx" | grep -qE "~ Add .*func Add\(a int\) int.*func Add\(a, b int\) int" \
      || fail "modified signature (before → after) not rendered"
    echo "$ctx" | grep -qE "\+ Sub .*func Sub\(a, b int\) int" || fail "added function not surfaced"
    # (Unchanged functions like Keep are absent from the signature section — covered
    # by the `corner_identical_files_yield_no_changes` unit test; not asserted here
    # since `Keep` also legitimately appears in the rendered Diffs section.)
  '';
  okMsg = "OK: changed function signatures folded into the grounded review context";
}
