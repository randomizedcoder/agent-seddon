# nix/checks/default.nix
#
# Aggregates every `nix flake check` target for agent-seddon.
#
# All of these run by default. crane reuses the shared `cargoArtifacts`
# dependency build, so clippy/test/audit only recompile first-party crates.
#
{
  pkgs,
  lib,
  craneLib,
  commonArgs,
  cargoArtifacts,
  advisory-db,
  versions,
  constantsRs,
}:

{
  clippy = import ./clippy.nix { inherit craneLib commonArgs cargoArtifacts; };
  rustfmt = import ./rustfmt.nix { inherit craneLib commonArgs; };
  test = import ./test.nix {
    inherit
      pkgs
      craneLib
      commonArgs
      cargoArtifacts
      ;
  };
  cargo-audit = import ./cargo-audit.nix { inherit craneLib commonArgs advisory-db; };
  # Deterministic perf gate (iai-callgrind under valgrind, absolute Ir ceilings)
  # + heap leak/allocation-budget gate (dhat). See docs/components/benchmarking.md.
  bench = import ./bench.nix {
    inherit
      craneLib
      commonArgs
      cargoArtifacts
      versions
      ;
  };
  leak = import ./leak.nix { inherit craneLib commonArgs cargoArtifacts; };
  nix-fmt = import ./nix-fmt.nix { inherit pkgs versions; };
  # `constants.rs` must match what `nix/constants.nix` renders (see gen-constants).
  constants-sync = import ./constants-sync.nix {
    inherit pkgs constantsRs;
    src = commonArgs.src;
  };
}
