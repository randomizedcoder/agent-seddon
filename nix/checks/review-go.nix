# nix/checks/review-go.nix
#
# Reproducible Go coverage for the code-review flow. Reconstructs a flake-pinned
# xtcp2 change (base + head trees, hash-locked) into a temp git repo and asserts
# `agent --review base..head` produces correct Go facts. Offline + deterministic
# (the trees are store paths; the collector never calls a model), so it runs in
# the hermetic `nix flake check` sandbox — unlike the real-repo Rust sweep, which
# needs the stripped `.git` and lives in `nix run .#review-eval`.
{
  pkgs,
  agent,
  reviewGoCorpus,
}:
let
  c = reviewGoCorpus."s3-secret-file"; # #56 feat/s3-secret-file (2 .go files)
in
import ../lib/mk-review-check.nix { inherit pkgs agent; } {
  name = "go";
  reviewConfig = ''
    # This check is scoped to repo/change/git-state facts; the analyzer + signature
    # collectors have their own dedicated checks (review-analyze, review-signatures).
    analyze = false
    signatures = false
  '';
  setup = ''
    cp -r ${c.base}/. .
    chmod -R u+w .
    git add -A -f && git commit -q -m base
    base="$(git rev-parse HEAD)"
    git remote add origin https://github.com/randomizedcoder/xtcp2.git
    git rm -rqf . >/dev/null
    cp -r ${c.head}/. .
    chmod -R u+w .
    git add -A -f && git commit -q -m head
    head="$(git rev-parse HEAD)"

    ctx="$(agent --config "$cfg" --review "$base..$head")"
    echo "----- generated Go review context -----"
    echo "$ctx"
    echo "----------------------------------------"
  '';
  asserts = ''
    echo "$ctx" | grep -q "Grounded review facts" || fail "no grounded facts block"
    echo "$ctx" | grep -q "Repo: go ·"            || fail "not detected as a Go project"
    echo "$ctx" | grep -q "· clone ·"             || fail "remote relationship not clone"
    echo "$ctx" | grep -q "cmd/xtcp2/xtcp2.go"    || fail "missing the changed .go file"
    echo "$ctx" | grep -q "^Diffs:"               || fail "diff hunks not rendered"
  '';
  okMsg = "OK: pinned Go change reconstructed; review facts correct";
}
