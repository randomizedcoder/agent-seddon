# nix/checks/test.nix
#
# `cargo test` across the workspace. Includes the agent-tools path-traversal
# unit tests. Hermetic: no network, so provider/ClickHouse integration tests
# (which need live services) are not run here.
#
{
  pkgs,
  craneLib,
  commonArgs,
  cargoArtifacts,
}:

craneLib.cargoTest (
  commonArgs
  // {
    inherit cargoArtifacts;
    # `agent-git`'s integration tests shell out to the `git` CLI (they pass
    # `GIT_AUTHOR_*`/`GIT_COMMITTER_*` identity via env, so the binary alone is
    # enough). The hermetic check sandbox has no host PATH, unlike `nix develop`,
    # so provide `git` here to match what those tests document they need.
    nativeBuildInputs = (commonArgs.nativeBuildInputs or [ ]) ++ [ pkgs.git ];
  }
)
