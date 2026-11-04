# nix/checks/update-nixpkgs.nix
#
# Arg-surface smoke for `nix run .#update-nixpkgs` (the weekly bump, which the
# gate otherwise never builds): building it runs writeShellApplication's
# shellcheck, and each row runs it with an argument that exits before it touches
# git, gh or the network. Row: name, args, expected exit code, expected text.
{
  pkgs,
}:
let
  app = import ../update-nixpkgs.nix { inherit pkgs; };
in
pkgs.runCommand "update-nixpkgs-args"
  {
    nativeBuildInputs = [
      app
      pkgs.coreutils
      pkgs.gnugrep
    ];
  }
  ''
    fail() { echo "FAIL: $*" >&2; exit 1; }
    while IFS='|' read -r name args code text; do
      [ -n "$name" ] || continue
      set +e
      # shellcheck disable=SC2086 # args is a word list by design
      res=$(update-nixpkgs $args 2>&1)
      got=$?
      set -e
      [ "$got" = "$code" ] || fail "$name: exit $got, want $code ($res)"
      echo "$res" | grep -qF -- "$text" || fail "$name: output lacks '$text' ($res)"
      echo "ok: $name"
    done <<'ROWS'
    positive_help|--help|0|usage: update-nixpkgs [--dry-run]
    positive_short_help|-h|0|usage: update-nixpkgs
    negative_unknown_arg|--merge|2|unknown arg '--merge'
    adversarial_injection_arg|;rm -rf /|2|unknown arg ';rm'
    ROWS
    touch $out
  ''
