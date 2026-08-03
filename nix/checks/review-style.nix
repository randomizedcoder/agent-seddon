# nix/checks/review-style.nix
#
# Code-style fingerprint coverage for the code-review flow (component 07).
# Reconstructs a small Go repo with a deliberate, consistent house style
# (tab-indented, PascalCase exported functions, conventional-commit messages) and
# asserts `agent --review base..head` folds a `Code style` fingerprint into the
# grounded context with the right verdicts.
#
# Fully in-process (counting over blobs + the commit log) — no toolchain, no
# network — so it runs in the hermetic sandbox with only `agent` + `git`.
{
  pkgs,
  agent,
}:
import ../lib/mk-review-check.nix { inherit pkgs agent; } {
  name = "style";
  reviewConfig = ''
    analyze = false
    signatures = false
    callgraph = false
    style = true
  '';
  setup = ''
    # Tab-indented, PascalCase exported functions, a doc comment. printf keeps the
    # leading tabs literal (a heredoc through nixfmt would mangle them).
    write_src() {
      printf 'package app\n\n// Handle sums twice.\nfunc Handle() int {\n\treturn Compute() + Compute()\n}\n\nfunc Compute() int {\n\treturn %s\n}\n' "$1" > app.go
    }

    write_src 1
    git add -A -f && git commit -q -m "feat: initial handler"
    base="$(git rev-parse HEAD)"
    write_src 2
    git add -A -f && git commit -q -m "fix: adjust compute result"
    head="$(git rev-parse HEAD)"

    ctx="$(agent --config "$cfg" --review "$base..$head")"
    echo "----- generated review context (code style) -----"
    echo "$ctx"
    echo "-------------------------------------------------"
  '';
  asserts = ''
    echo "$ctx" | grep -q "Grounded review facts"        || fail "no grounded facts block"
    echo "$ctx" | grep -q "Code style"                   || fail "no code-style section"
    echo "$ctx" | grep -q "indent tabs"                  || fail "tab indentation not detected"
    echo "$ctx" | grep -qE "fn pascal"                   || fail "PascalCase functions not detected"
    echo "$ctx" | grep -q "conventional"                 || fail "conventional-commit ratio missing"
    echo "$ctx" | grep -q "change conforms to repo style: yes" \
      || fail "change should conform to the repo's own style"
  '';
  okMsg = "OK: code-style fingerprint folded into the grounded review context";
}
