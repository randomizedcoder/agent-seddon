# nix/checks/review-shellcheck.nix
#
# Shellcheck coverage for the code-review flow (review-fleet increment 5, C12).
# Builds a temp git repo whose head adds a shell script with an unquoted variable
# (the classic SC2086) plus a script carrying an inline `# shellcheck disable=…`
# directive (which the no-ignores rule flags as its own finding), then asserts
# `agent --review base..head` runs shellcheck on the changed scripts under the
# sandbox and folds the findings into the grounded context's
# `Shell scripts (shellcheck):` section.
#
# Offline + deterministic: shellcheck is a static analyzer (no network, no
# toolchain download), so it runs in the hermetic `nix flake check` sandbox.
{
  pkgs,
  agent,
}:
import ../lib/mk-review-check.nix { inherit pkgs agent; } {
  name = "shellcheck";
  extraInputs = [ pkgs.shellcheck ];
  # Isolate the collector under test: only shellcheck on, the rest off so the
  # section is unambiguous and the run is fast.
  reviewConfig = ''
    analyze = false
    signatures = false
    callgraph = false
    style = false
    summaries = false
    cochange = false
    churn = false
    shellcheck = true
  '';
  setup = ''
    git commit -q --allow-empty -m base
    base="$(git rev-parse HEAD)"

    # An unquoted expansion — shellcheck reliably flags SC2086.
    cat > deploy.sh <<'SH'
    #!/usr/bin/env bash
    target=$1
    echo $target
    SH

    # An inline disable directive — the no-ignores rule flags the directive itself.
    cat > run.bash <<'SH'
    #!/usr/bin/env bash
    # shellcheck disable=SC2086
    x=$1
    echo $x
    SH

    git add -A -f && git commit -q -m head
    head="$(git rev-parse HEAD)"

    ctx="$(agent --config "$cfg" --review "$base..$head")"
    echo "----- generated review context (shellcheck) -----"
    echo "$ctx"
    echo "-------------------------------------------------"
  '';
  asserts = ''
    echo "$ctx" | grep -q "Grounded review facts"        || fail "no grounded facts block"
    echo "$ctx" | grep -q "Shell scripts (shellcheck):"  || fail "no shellcheck section"
    echo "$ctx" | grep -q "SC2086"                        || fail "the unquoted-var finding was not surfaced"
    echo "$ctx" | grep -q "deploy.sh"                     || fail "finding not tied to the changed script"
    echo "$ctx" | grep -q "no-ignores"                    || fail "inline disable directive not flagged (no-ignores rule)"
  '';
  okMsg = "OK: shellcheck findings + no-ignores rule folded into the grounded review context";
}
