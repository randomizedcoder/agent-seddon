# nix/checks/update-nixpkgs.nix
#
# Arg-surface smoke for `nix run .#update-nixpkgs` (the weekly bump, which the
# gate otherwise never builds): building it runs writeShellApplication's
# shellcheck, and each row runs it with an argument that exits before it touches
# git, gh or the network. Row: name, args, expected exit code, expected text.
# Then `update-nixpkgs-summary` (the red-PR body) against fixture logs, one per
# phrasing nix uses for a failed derivation. Row: name, log lines (`\n`-escaped),
# a text the body must hold, and one it must not.
{
  pkgs,
}:
let
  app = import ../update-nixpkgs.nix { inherit pkgs; };
  summary = import ../update-nixpkgs-summary.nix { inherit pkgs; };
  h = "0123456789abcdfghijklmnpqrsvwxyz"; # 32 store-hash chars
in
pkgs.runCommand "update-nixpkgs-args"
  {
    nativeBuildInputs = [
      app
      summary
      pkgs.coreutils
      pkgs.gnugrep
    ];
  }
  ''
    fail() { echo "FAIL: $*" >&2; exit 1; }
    while IFS='|' read -r name args code text; do
      [ -n "$name" ] || continue
      set +e
      read -r -a argv <<<"$args"
      res=$(update-nixpkgs "''${argv[@]}" 2>&1)
      got=$?
      set -e
      [ "$got" = "$code" ] || fail "$name: exit $got, want $code ($res)"
      echo "$res" | grep -qF -- "$text" || fail "$name: output lacks '$text' ($res)"
      echo "ok: $name"
    done <<'ROWS'
    positive_help|--help|0|usage: update-nixpkgs [--dry-run]
    positive_short_help|-h|0|usage: update-nixpkgs
    negative_unknown_arg|--merge|2|unknown arg '--merge'
    corner_help_wins_over_dry_run|--dry-run --help|0|usage: update-nixpkgs
    negative_first_bad_arg_wins|--merge --help|2|unknown arg '--merge'
    adversarial_injection_arg|;rm -rf /|2|unknown arg ';rm'
    ROWS

    while IFS='|' read -r name lines want deny; do
      [ -n "$name" ] || continue
      printf '%b\n' "$lines" >log
      res=$(update-nixpkgs-summary "The gate failed." log)
      echo "$res" | grep -qF -- "$want" || fail "$name: body lacks '$want' ($res)"
      if [ -n "$deny" ] && echo "$res" | grep -qF -- "$deny"; then
        fail "$name: body holds '$deny' ($res)"
      fi
      echo "ok: $name"
    done <<'ROWS'
    positive_cannot_build|error: Cannot build '/nix/store/${h}-agent-cli-help.drv'.|- agent-cli-help|(none named)
    positive_builder_for|error: builder for '/nix/store/${h}-portal-visual.drv' failed with exit code 1|- portal-visual|(none named)
    positive_dependency_phrasing|error: 1 dependencies of derivation '/nix/store/${h}-agent.drv' failed to build|- agent|(none named)
    corner_error_lines_kept|error: attribute 'go_latest' missing\n  at flake.nix:3|error: attribute 'go_latest' missing|  at flake.nix
    negative_no_match|warning: Git tree is dirty\nall good|- (none named)|warning:
    adversarial_hash_too_short|error: Cannot build '/nix/store/abc-evil.drv'.|- (none named)|- evil
    ROWS

    # A derivation named in two phrasings is listed once.
    printf '%s\n' "error: Cannot build '/nix/store/${h}-leak.drv'." \
      "error: builder for '/nix/store/${h}-leak.drv' failed" >log
    n=$(update-nixpkgs-summary "The gate failed." log | grep -c '^- leak$' || true)
    [ "$n" = 1 ] || fail "corner_same_drv_twice_listed_once: listed $n times"
    echo "ok: corner_same_drv_twice_listed_once"

    # Only the first 40 error lines go in the body.
    for i in $(seq 1 60); do echo "error: e$i"; done >log
    res=$(update-nixpkgs-summary "The gate failed." log)
    echo "$res" | grep -qx 'error: e40' || fail "boundary_error_lines_capped_at_40: e40 missing"
    if echo "$res" | grep -qx 'error: e41'; then fail "boundary_error_lines_capped_at_40: e41 kept"; fi
    echo "ok: boundary_error_lines_capped_at_40"

    set +e
    update-nixpkgs-summary only-one-arg >/dev/null 2>&1
    got=$?
    set -e
    [ "$got" = 2 ] || fail "negative_summary_usage: exit $got, want 2"
    echo "ok: negative_summary_usage"
    touch $out
  ''
