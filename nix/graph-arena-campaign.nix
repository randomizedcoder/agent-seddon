# `nix run .#graph-arena-campaign` — the full statistical ladder, one command.
#
# Runs every campaign rung (07-arena-campaign.md: lockbox S, logtriage M,
# csv-slice M, relay L — all arms, R=5/5/5/3) sequentially through the arena
# driver, one resumable output root. Re-running the SAME command is the
# recovery pass (completed runs skip, DNF casualties retry). ~12-13 pod-hours
# for the full ladder — a deliberate standalone; eval-all does NOT run it.
#
# Per the repo's language policy this wrapper is a pure exec shim — the ladder,
# resume semantics, and the combined report live in test/graph-arena/campaign.py
# (pure parts tested in the hermetic `graph-arena-tests` check).
#
# Env: the graph-arena set (AGENT_E2E_*, ARENA_LOCAL_*, ARENA_CLICKHOUSE[_NATIVE])
#      + ARENA_OUTPUT_DIR (REQUIRED — the campaign refuses a throwaway root).
# Args: [--only lockbox,relay]
{
  pkgs,
  lib,
  versions,
  agent,
}:
pkgs.writeShellApplication {
  name = "graph-arena-campaign";
  runtimeInputs = [
    agent
    versions.go
    pkgs.gcc # csv-slice (C) — agent in-run builds + scoring + ASAN
    pkgs.git
    pkgs.python3
    pkgs.gnugrep
    pkgs.coreutils
    pkgs.nix # self-plant a durable gcroot (see below)
  ];
  text = ''
    export ARENA_COGNITION_DIR="${../config/cognition}"
    # Pure-Go / pure-C builds match the hermetic check (no ambient cc/cgo).
    export CGO_ENABLED=0

    # Durable gcroot (F3, root cause of the #256 recovery): `nix run` plants no
    # persistent root, so a ~12h campaign whose seed sources are read lazily can
    # have them garbage-collected mid-run (a parallel `nix` GC collected the
    # `-graph-arena` store path and the driver then failed "objective seed dir
    # missing"). Best-effort self-root both source store paths under the output
    # dir so GC cannot collect them while the campaign runs. Never fail the run
    # because rooting failed (read-only store, no daemon perms, absent nix).
    plant_gcroot() {
      nix-store --add-root "$2" --indirect --realise "$1" >/dev/null 2>&1 \
        || echo "graph-arena-campaign: warning: could not gcroot $1 (continuing)" >&2
    }
    if [ -n "''${ARENA_OUTPUT_DIR:-}" ]; then
      mkdir -p "$ARENA_OUTPUT_DIR" 2>/dev/null || true
      plant_gcroot "${../test/graph-arena}" "$ARENA_OUTPUT_DIR/.gcroot-graph-arena"
      plant_gcroot "${../config/cognition}" "$ARENA_OUTPUT_DIR/.gcroot-cognition"
    fi

    exec python3 "${../test/graph-arena}/campaign.py" "$@"
  '';
}
