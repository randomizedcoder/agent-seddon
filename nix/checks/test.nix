# nix/checks/test.nix
#
# `cargo test -p <crate>` for one workspace crate, on that crate's source slice
# (nix/workspace-slice.nix): the check re-runs only when the crate or one of its
# workspace dependencies changes. `nix/checks/default.nix` makes one `test-<crate>`
# per member, and `test` aggregates them. Hermetic: no network, so
# provider/ClickHouse integration tests (which need live services) are not run here.
#
{
  pkgs,
  craneLib,
  commonArgs,
  workspaceSlice,
  renderedK3s,
  crate,
  # The crate's features in a whole-workspace build (nix/test-features.json), so
  # its feature-gated tests run as they did under one workspace-wide `cargo test`.
  features,
}:

craneLib.cargoTest (
  commonArgs
  // workspaceSlice.sliceArgs [ crate ]
  // {
    pname = "test-${crate}";
    cargoTestExtraArgs =
      "-p ${crate}"
      + pkgs.lib.optionalString (features != [ ]) " --features ${pkgs.lib.concatStringsSep "," features}";
    # `agent-k8s-render` reads the committed rendered/ tree, which is not in the crane
    # source (nix/checks/default.nix `renderedK3s`).
    AGENT_RENDERED_K3S = renderedK3s;
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
