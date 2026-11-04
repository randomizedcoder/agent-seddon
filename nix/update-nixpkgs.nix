# nix/update-nixpkgs.nix
#
# `nix run .#update-nixpkgs` — keep the pinned nixpkgs current, so the review
# toolbox (go, golangci-lint, gosec, shellcheck, …) can analyze repos that target
# newer toolchains. nixpkgs keeps older versions; an old pin only lacks newer ones.
#
# Run from a checkout of this repo. It:
#   1. skips when an update PR from this job is already open, or the disk (this
#      checkout's and /nix/store's) is under the floor;
#   2. makes a throwaway worktree of origin/main on `chore/nixpkgs-<date>`;
#   3. runs `nix flake update nixpkgs advisory-db` there (stops if nothing moved),
#      then the deterministic offline regen (`gen-openapi`) the gate checks — a
#      regen that fails (the flake no longer evaluates) is reported like a red gate;
#   4. runs the gate (`nix flake check --keep-going`) and notes the failing checks;
#      the full log is kept under $XDG_STATE_HOME/update-nixpkgs/, and its error
#      lines go in the PR body;
#   5. commits, pushes and opens a PR with the result. It never merges.
#
#   nix run .#update-nixpkgs                 # the weekly job
#   nix run .#update-nixpkgs -- --dry-run    # everything up to (not incl.) push + PR
#
# A bump that moves Flutter can still leave the portal goldens red: regenerating
# them is a judgement call (a real visual change, or font-rendering noise), so the
# PR names that check and a human decides.
#
# Env: MIN_FREE_PCT (default 10), MAX_JOBS (8), CORES (4).
{
  pkgs,
}:
let
  summary = import ./update-nixpkgs-summary.nix { inherit pkgs; };
in
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
    pkgs.nix
    pkgs.openssh
    summary
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
    # A timer may run with no git identity; the commit must not die after the gate.
    if [ -z "$(git config user.email || true)" ] || [ -z "$(git config user.name || true)" ]; then
      export GIT_AUTHOR_NAME=update-nixpkgs GIT_AUTHOR_EMAIL=update-nixpkgs@localhost
      export GIT_COMMITTER_NAME=update-nixpkgs GIT_COMMITTER_EMAIL=update-nixpkgs@localhost
    fi

    # Only this job's own PRs count: anyone can open a PR with a matching title.
    open=$(gh pr list --state open --author "@me" --json headRefName \
      --jq '[.[] | select(.headRefName | startswith("chore/nixpkgs-"))] | length')
    if [ "$open" != 0 ]; then
      echo "update-nixpkgs: an update PR is already open; skipping"
      exit 0
    fi

    floor=''${MIN_FREE_PCT:-10}
    for fs in "$top" /nix/store; do
      free=$((100 - $(df --output=pcent "$fs" | tail -1 | tr -dc 0-9)))
      if [ "$free" -lt "$floor" ]; then
        echo "update-nixpkgs: only $free% free on $fs (floor $floor%); skipping"
        exit 0
      fi
    done

    git fetch -q origin main
    branch="chore/nixpkgs-$(date -u +%Y%m%d)"
    logdir="''${XDG_STATE_HOME:-$HOME/.local/state}/update-nixpkgs"
    mkdir -p "$logdir"
    log="$logdir/$(date -u +%Y%m%dT%H%M%SZ).log"
    work=$(mktemp -d)
    cleanup() {
      git -C "$top" worktree remove --force "$work" 2>/dev/null || true
      rmdir "$work" 2>/dev/null || true
      git -C "$top" branch -D "$branch" 2>/dev/null || true
    }
    trap cleanup EXIT
    # A same-day rerun reuses the name (this job is the branch's only writer, so the
    # push below forces: a stale remote copy from a failed run is replaced). A run
    # that was killed (no EXIT trap) can leave a worktree holding the branch: drop
    # it first, or `worktree add -b` dies.
    git worktree prune
    git worktree list --porcelain |
      awk -v b="branch refs/heads/$branch" '/^worktree /{w=substr($0,10)} $0==b{print w}' |
      while IFS= read -r stale; do git worktree remove --force "$stale" || true; done
    git branch -D "$branch" 2>/dev/null || true
    git worktree add -q -b "$branch" "$work" origin/main
    cd "$work"

    rev() { jq -r '.nodes.nixpkgs.locked | "\(.rev[0:8]) (\(.lastModified | todate | .[0:10]))"' flake.lock; }
    old=$(rev)
    if ! nix flake update nixpkgs advisory-db >"$log" 2>&1; then
      echo "update-nixpkgs: nix flake update failed (log: $log)" >&2
      exit 1
    fi
    if git diff --quiet flake.lock; then
      echo "update-nixpkgs: nixpkgs and advisory-db are already current ($old)"
      exit 0
    fi
    new=$(rev)
    echo "update-nixpkgs: nixpkgs $old -> $new; regenerating and running the gate (log: $log)"

    # The newer protoc plugins can change the generated OpenAPI; the gate checks it.
    # A pin that drops an attr this flake uses fails here, at eval: that is the
    # failure most worth a PR, so it is reported like a red gate, not lost.
    stage=""
    if ! nix run .#gen-openapi >>"$log" 2>&1; then
      stage="The OpenAPI regen (nix run .#gen-openapi) failed, so the gate did not run; the flake may no longer evaluate at this pin."
    elif ! nix flake check --max-jobs "''${MAX_JOBS:-8}" --cores "''${CORES:-4}" --keep-going >>"$log" 2>&1; then
      stage="The gate (nix flake check --keep-going) failed."
    fi
    if [ -z "$stage" ]; then
      result="The gate (nix flake check --keep-going) passed."
    else
      result=$(update-nixpkgs-summary "$stage" "$log")
    fi
    echo "$result"

    if [ "$old" = "$new" ]; then
      title="chore(deps): advisory-db (nixpkgs $new)"
    else
      title="chore(deps): nixpkgs $old -> $new"
    fi
    git add flake.lock crates/agent-proto/openapi/agent.swagger.json
    git commit -q -m "$title"
    if [ "$dry" = 1 ]; then
      echo "update-nixpkgs: --dry-run; not pushing or opening a PR"
      exit 0
    fi
    git push -q --force -u origin "$branch"
    body=$(printf 'Weekly nixpkgs bump (nix run .#update-nixpkgs).\n\n%s\n' "$result")
    # Two same-day runs (the timer and a manual one) can both pass the open-PR check;
    # the later push then owns the branch, so its result replaces the PR's text —
    # but only on this job's own PR: anyone else's is left alone, and the create
    # error stands.
    if ! gh pr create --base main --head "$branch" --title "$title" --body "$body"; then
      mine=$(gh pr list --state open --author "@me" --head "$branch" --json number --jq length)
      if [ "$mine" != 1 ]; then
        echo "update-nixpkgs: gh pr create failed, and $branch has no open PR of ours" >&2
        exit 1
      fi
      gh pr edit "$branch" --title "$title" --body "$body"
    fi
    # The PR holds the branch now; drop the local copy (cleanup), so a rerun works.
  '';
}
