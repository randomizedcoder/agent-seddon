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
    # The hermetic check sandbox has no host PATH, unlike `nix develop`, so
    # provide the CLIs the tests shell out to:
    #   - `git`: `agent-git`'s integration tests (identity via `GIT_AUTHOR_*`/
    #     `GIT_COMMITTER_*` env, so the binary alone is enough);
    #   - `rg` (ripgrep): the `grep` tool's fast path — including it here exercises
    #     the `rg` branch (the in-process fallback covers its absence).
    nativeBuildInputs = (commonArgs.nativeBuildInputs or [ ]) ++ [
      pkgs.git
      pkgs.ripgrep
    ];
  }
)
