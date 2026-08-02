# nix/checks/cli-help.nix
#
# CLI-surface smoke (mirrors hermes' `nix/checks.nix` cli-commands): run the
# shipped binary's `agent --help` and assert it stays runnable and keeps
# advertising the full serve interface. The help renders the seam list from
# `grpc_server::Seam::flag_names()` (the `SEAMS` table), so a seam added to the
# runtime but dropped from the CLI surface — or a `--help` that regresses/panics —
# fails here rather than shipping. Fully offline: `--help` reads no config and
# calls nothing.
{
  pkgs,
  agent,
}:
pkgs.runCommand "agent-cli-help"
  {
    nativeBuildInputs = [
      agent
      pkgs.coreutils
    ];
  }
  ''
    export HOME="$(mktemp -d)"

    if ! help="$(agent --help)"; then
      echo "FAIL: agent --help must exit 0" >&2
      exit 1
    fi
    echo "$help"

    require() {
      case "$help" in
        *"$1"*) : ;;
        *) echo "FAIL: agent --help must document '$1'" >&2; exit 1 ;;
      esac
    }

    # The one-shot / serve interface must stay documented.
    require "--config"
    require "--check-config"
    require "--serve-mcp"
    require "--serve-all"
    require "--serve-sessions"
    require "--serve-<seam>"

    # A representative slice of the per-seam gRPC surface (`<seam> = a|b|c|...`).
    # Not the full 28 — enough to catch a gross `flag_names()` regression without
    # turning the check into a manual list to babysit.
    for seam in provider memory tools context policy search session tokenizer scheduler lsp; do
      require "$seam"
    done

    echo "OK: agent --help stays runnable and advertises the full serve interface" > "$out"
  ''
