# nix/update-nixpkgs.nix
#
# `nix run .#update-nixpkgs` — keep the pinned nixpkgs current, so the review
# toolbox (go, golangci-lint, gosec, shellcheck, …) can analyze repos that target
# newer toolchains. nixpkgs keeps older versions; an old pin only lacks newer ones.
#
# Run from a checkout of this repo. It:
#   1. skips when an update PR is already open, or the disk is under the floor;
#   2. makes a throwaway worktree of origin/main on `chore/nixpkgs-<date>`;
#   3. runs `nix flake update nixpkgs` there (stops if nothing moved);
#   4. runs the gate (`nix flake check --keep-going`) and notes the failing checks;
#   5. commits, pushes and opens a PR with the result. It never merges.
#
#   nix run .#update-nixpkgs                 # the weekly job
#   nix run .#update-nixpkgs -- --dry-run    # everything up to (not incl.) push + PR
#
# Env: MIN_FREE_PCT (default 10), MAX_JOBS (8), CORES (4).
{
  pkgs,
}:
pkgs.writeShellApplication {
  name = "update-nixpkgs";
  runtimeInputs = [
    pkgs.coreutils
    pkgs.gawk
    pkgs.gnugrep
    pkgs.gnused
    pkgs.git
    pkgs.gh
    pkgs.jq
    pkgs.openssh
  ];
  text = ''
    dry=0
    for a in "$@"; do
      case "$a" in
        --dry-run) dry=1 ;;
        -h | --help)
          echo "usage: update-nixpkgs [--dry-run]"
          exit 0
          ;;
        *)
          echo "update-nixpkgs: unknown arg '$a'" >&2
          exit 2
          ;;
      esac
    done

    top=$(git rev-parse --show-toplevel)
    cd "$top"
    : "''${GIT_SSH_COMMAND:=ssh -o ControlMaster=no -o ControlPath=none}"
    export GIT_SSH_COMMAND

    open=$(gh pr list --state open --search 'in:title "chore(deps): nixpkgs"' --json number --jq 'length')
    if [ "$open" != 0 ]; then
      echo "update-nixpkgs: an update PR is already open; skipping"
      exit 0
    fi

    free=$((100 - $(df --output=pcent "$top" | tail -1 | tr -dc 0-9)))
    if [ "$free" -lt "''${MIN_FREE_PCT:-10}" ]; then
      echo "update-nixpkgs: only $free% free (floor ''${MIN_FREE_PCT:-10}%); skipping"
      exit 0
    fi

    git fetch -q origin main
    branch="chore/nixpkgs-$(date -u +%Y%m%d)"
    work=$(mktemp -d)
    cleanup() {
      git -C "$top" worktree remove --force "$work" 2>/dev/null || true
      git -C "$top" branch -D "$branch" 2>/dev/null || true
    }
    trap cleanup EXIT
    git worktree add -q -b "$branch" "$work" origin/main
    cd "$work"

    rev() { jq -r '.nodes.nixpkgs.locked | "\(.rev[0:8]) (\(.lastModified | todate | .[0:10]))"' flake.lock; }
    old=$(rev)
    nix flake update nixpkgs
    if git diff --quiet flake.lock; then
      echo "update-nixpkgs: nixpkgs is already current ($old)"
      exit 0
    fi
    new=$(rev)
    echo "update-nixpkgs: nixpkgs $old -> $new; running the gate"

    log="$work/.gate.log"
    if nix flake check --max-jobs "''${MAX_JOBS:-8}" --cores "''${CORES:-4}" --keep-going >"$log" 2>&1; then
      result="The gate (nix flake check --keep-going) passed."
    else
      failed=$(grep -oE "Cannot build '/nix/store/[a-z0-9]{32}-[^']+\.drv'" "$log" |
        sed -E "s|.*/[a-z0-9]{32}-||; s|\.drv'$||" | sort -u | sed 's/^/- /')
      result=$(printf 'The gate failed. Failing derivations:\n\n%s\n\nRebuild a flaky check alone before treating it as a blocker.' "''${failed:-"- (none named; see the gate log)"}")
    fi
    rm -f "$log"
    echo "$result"

    title="chore(deps): nixpkgs $old -> $new"
    git commit -q -m "$title" flake.lock
    if [ "$dry" = 1 ]; then
      echo "update-nixpkgs: --dry-run; not pushing or opening a PR"
      exit 0
    fi
    git push -q -u origin "$branch"
    gh pr create --base main --head "$branch" --title "$title" --body "$(printf 'Weekly nixpkgs bump (nix run .#update-nixpkgs).\n\n%s\n' "$result")"
    trap - EXIT
    cd "$top"
    git worktree remove --force "$work"
  '';
}
