# nix/checks/feature-matrix.nix
#
# **Lean-feature build** gate. Every other check compiles the workspace with the
# default (or `--all-features`) feature set, so a `#[cfg(feature = …)]` that is
# too narrow on a definition, or missing on a caller, only surfaces when someone
# builds a lean binary by hand — and `agent-runtime --no-default-features` had
# silently regressed to 21 errors that way. This check type-checks a small matrix
# of minimal / single-feature builds so a gating mistake fails the gate like a
# lint would.
#
# `cargo check` only (rmeta, no codegen) so the lean rows cost minutes, not a
# rebuild. Add a row when a feature gets its own factory line or store module;
# the rule for a fix is in docs/extending.md ("Verifying an extension").
#
{
  craneLib,
  commonArgs,
  cargoArtifacts,
}:

craneLib.mkCargoDerivation (
  commonArgs
  // {
    inherit cargoArtifacts;
    pname = "agent-seddon-feature-matrix";
    version = "0.1.0";
    doInstallCargoArtifacts = false;
    buildPhaseCargoCommand = ''
      cargo check -p agent-runtime --no-default-features
      cargo check -p agent-runtime --no-default-features --features campaign
      cargo check -p agent-runtime --no-default-features --features campaign-postgres
      cargo check -p agent-runtime --no-default-features --features provider-router
      cargo check -p agent-runtime --features role-postgres
      cargo check -p agent-runtime --features auth-postgres
      cargo check -p agent-cli --no-default-features
    '';
    installPhaseCommand = "mkdir -p $out";
  }
)
