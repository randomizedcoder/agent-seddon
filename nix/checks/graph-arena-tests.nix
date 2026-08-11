# nix/checks/graph-arena-tests.nix
#
# The graph-arena harness is tested like product code (R13 of
# docs/design/cognition-graph/06-graph-arena.md): the pure core's four-class
# unittest tables (manifest/step/scoring/arm-config, hermetic fakes) plus the
# check-the-checks fixture matrix — every objective requirement must ACCEPT its
# pass fixture and REJECT its fail fixture, so an always-green check fails the
# build. Runs `go` offline (GOPROXY=off, stdlib-only modules); no model, no
# network, no agent binary.
{
  pkgs,
  versions,
}:
pkgs.runCommand "graph-arena-tests"
  {
    nativeBuildInputs = [
      pkgs.python3
      versions.go
      pkgs.git
      pkgs.gnugrep
      pkgs.coreutils
    ];
  }
  ''
    export HOME="$(mktemp -d)"
    cp -r ${../../test/graph-arena} arena
    chmod -R u+w arena
    cd arena
    echo "graph-arena-tests: core tables ..."
    python3 -m unittest test_arena_core -v
    echo "graph-arena-tests: check-the-checks fixture matrix ..."
    python3 -m unittest test_fixtures -v
    touch "$out"
  ''
