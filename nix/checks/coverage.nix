# nix/checks/coverage.nix
#
# Test-coverage check (source-based, via `cargo-llvm-cov`). Runs the workspace
# tests **instrumented** and emits `lcov.info` as the derivation output.
#
# Gated with a **ratchet floor**: `--fail-under-lines` fails the check if line
# coverage drops below the floor, so a regression (e.g. a big test deletion) is
# caught like a lint. The floor is set CONSERVATIVELY below the observed baseline
# (~85% at the time of writing) to leave headroom for noise; raise it as coverage
# improves — never lower it to make a red check pass. The human-facing report
# (HTML + per-file summary) is the on-demand `nix run .#coverage` app.
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
      # Ratchet floor: fail if line coverage regresses below this. Conservative vs
      # the ~85% baseline; raise over time, never lower to green a red check.
      + "--fail-under-lines 80 "
      + "--lcov --output-path $out";
  }
)
