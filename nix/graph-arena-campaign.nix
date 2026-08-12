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
  ];
  text = ''
    export ARENA_COGNITION_DIR="${../config/cognition}"
    # Pure-Go / pure-C builds match the hermetic check (no ambient cc/cgo).
    export CGO_ENABLED=0
    exec python3 "${../test/graph-arena}/campaign.py" "$@"
  '';
}
