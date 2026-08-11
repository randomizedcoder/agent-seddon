# `nix run .#graph-arena` — the cognition-graph A/B/n value sweep.
#
# The same purpose-built, multi-requirement objective runs under the graph-less
# baseline and the shipped cognition documents; each run is scored per
# requirement and the harness prints a k/n comparison table plus JSONL
# artifacts. Design: docs/design/cognition-graph/06-graph-arena.md (harness
# increment 1: baseline+simple arms, mechanical scoring).
#
# Not a hermetic check: needs the generator (Kimi) and — for graph arms — the
# judge/critic (GLM) endpoints. The hermetic companion is the
# `graph-arena-tests` flake check (the harness's own test suite).
#
# Per the repo's language policy this wrapper is a pure exec shim — preflights,
# contract exits, and all orchestration live in test/graph-arena/driver.py.
#
# Env: AGENT_E2E_{BASE_URL,MODEL,API_KEY} (generator, required),
#      AGENT_E2E_JUDGE_{BASE_URL,MODEL,API_KEY_FILE,INSECURE_TLS} (graph arms),
#      ARENA_OUTPUT_DIR, AGENT_BIN.
# Args: --objective lockbox --tier S --arms baseline,simple --reps 2
{
  pkgs,
  lib,
  versions,
  agent,
}:
pkgs.writeShellApplication {
  name = "graph-arena";
  runtimeInputs = [
    agent
    versions.go
    pkgs.git
    pkgs.python3
    pkgs.gnugrep
    pkgs.coreutils
  ];
  text = ''
    export ARENA_COGNITION_DIR="${../config/cognition}"
    # Pure-Go builds: the shim carries no C compiler, and objectives importing
    # `net` (relay) must build for the agent's own in-run verification exactly
    # as they do for scoring (driver.py sets the same for its step executor).
    export CGO_ENABLED=0
    exec python3 "${../test/graph-arena}/driver.py" "$@"
  '';
}
