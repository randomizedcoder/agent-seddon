# nix/checks/ast-scip.nix
#
# SCIP-substrate coverage for the AstBackend `scip` engine (docs/components/ast.md).
# Runs the end-to-end test that indexes a fixture Go module with `scip-go` and asserts
# the engine ingests the real `.scip` output and resolves the implicit
# interface-implementation relation (both implementers of `Greeter`).
#
# Offline + hermetic: a stdlib-only fixture means `go`/`scip-go` resolve locally with
# no downloads. Needs `scip-go` + the pinned `go` toolchain on PATH.
{
  craneLib,
  commonArgs,
  cargoArtifacts,
  versions,
}:

craneLib.mkCargoDerivation (
  commonArgs
  // {
    inherit cargoArtifacts;
    pname = "agent-seddon-ast-scip";
    version = "0.1.0";
    doInstallCargoArtifacts = false;
    nativeBuildInputs = (commonArgs.nativeBuildInputs or [ ]) ++ [
      versions.scip-go
      versions.go
    ];
    buildPhaseCargoCommand = ''
      export HOME="$(mktemp -d)"
      export GOPROXY=off
      cargo test -p agent-ast --features ast-scip --test scip_e2e -- --nocapture
    '';
    installPhaseCommand = "mkdir -p $out";
  }
)
