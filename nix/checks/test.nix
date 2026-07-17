# nix/checks/test.nix
#
# `cargo test` across the workspace. Includes the agent-tools path-traversal
# unit tests. Hermetic: no network, so provider/ClickHouse integration tests
# (which need live services) are not run here.
#
{
  craneLib,
  commonArgs,
  cargoArtifacts,
}:

craneLib.cargoTest (commonArgs // { inherit cargoArtifacts; })
