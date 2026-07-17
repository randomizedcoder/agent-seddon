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
}:

{
  clippy = import ./clippy.nix { inherit craneLib commonArgs cargoArtifacts; };
  rustfmt = import ./rustfmt.nix { inherit craneLib commonArgs; };
  test = import ./test.nix { inherit craneLib commonArgs cargoArtifacts; };
  cargo-audit = import ./cargo-audit.nix { inherit craneLib commonArgs advisory-db; };
  nix-fmt = import ./nix-fmt.nix { inherit pkgs versions; };
}
