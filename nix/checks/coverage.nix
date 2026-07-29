# nix/checks/coverage.nix
#
# Test-coverage check (source-based, via `cargo-llvm-cov`). Runs the workspace
# tests **instrumented** and emits `lcov.info` as the derivation output.
#
# Non-gating on the *number*: there is deliberately no `--fail-under-*`, so this
# only asserts that the instrumented build + default-feature tests pass and that
# a report can be produced — it keeps the coverage path alive in `nix flake check`
# without pinning a threshold to an unknown baseline. The human-facing report
# (HTML + summary) is the on-demand `nix run .#coverage` app.
#
# Feature set mirrors the `test` check: **default features** (NOT `--all-features`)
# — `dhat-heap` installs a `#[global_allocator]` that conflicts with coverage
# instrumentation, and the non-default backends aren't the runnable set. Generated
# code is excluded (proto stubs live in OUT_DIR and are auto-excluded; the committed
# @generated constants.rs, benches, and dev-only agent-testkit are regex-ignored).
#
# `cargoArtifacts = null`: the shared dep cache is built WITHOUT coverage
# instrumentation (different RUSTFLAGS) and `cargo llvm-cov` uses its own target
# dir, so reusing it buys nothing — deps come from crane's auto-vendored source.
#
{
  pkgs,
  craneLib,
  commonArgs,
}:

craneLib.cargoLlvmCov (
  commonArgs
  // {
    cargoArtifacts = null;
    # The suite shells out to these exactly as the `test` check supplies them:
    # `git` for agent-git's fixture repos, `rg` for the grep tool's fast path.
    nativeBuildInputs = (commonArgs.nativeBuildInputs or [ ]) ++ [
      pkgs.git
      pkgs.ripgrep
    ];
    cargoLlvmCovExtraArgs =
      "--workspace "
      + "--ignore-filename-regex '(/constants\\.rs$|/benches/|/agent-testkit/)' "
      + "--lcov --output-path $out";
  }
)
