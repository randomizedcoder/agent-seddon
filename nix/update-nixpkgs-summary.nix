# nix/update-nixpkgs-summary.nix
#
# `update-nixpkgs-summary <stage> <log>` — the red-PR body text for
# `nix run .#update-nixpkgs`: <stage> (what failed), then the failed derivations
# and the `error:` lines nix wrote to <log>. Its own app so
# `nix/checks/update-nixpkgs.nix` can pin the extraction against fixture logs:
# nix's phrasings drift across versions, and a silent mismatch would turn every
# red PR into "(none named)".
{
  pkgs,
}:
pkgs.writeShellApplication {
  name = "update-nixpkgs-summary";
  runtimeInputs = [
    pkgs.coreutils
    pkgs.gnugrep
    pkgs.gnused
  ];
  text = ''
    if [ "$#" -ne 2 ]; then
      echo "usage: update-nixpkgs-summary <stage> <log>" >&2
      exit 2
    fi
    stage=$1
    log=$2
    # nix names a failed derivation in several phrasings; take any of them.
    failed=$(grep -oE "(Cannot build|builder for|derivation) '/nix/store/[a-z0-9]{32}-[^']+\.drv'" "$log" |
      sed -E "s|.*/[a-z0-9]{32}-||; s|\.drv'$||" | sort -u | sed 's/^/- /' || true)
    errors=$(grep -E '^error:' "$log" | cut -c1-300 | head -n 40 || true)
    printf '%s Failing derivations:\n\n%s\n\nError lines (the full log is on the host that ran the job, %s):\n\n~~~\n%s\n~~~\n\nRebuild a flaky check alone before treating it as a blocker.\n' \
      "$stage" "''${failed:-"- (none named)"}" "$log" "''${errors:-(none)}"
  '';
}
