# nix/checks/bench.nix
#
# Performance gate. Runs the iai-callgrind benches under callgrind and fails if a
# bench exceeds its **absolute** instruction ceiling (`--callgrind-limits='ir=…'`).
# callgrind counts are deterministic given the pinned valgrind + toolchain, so an
# absolute ceiling is a stable, baseline-free gate for a stateless `nix flake
# check` — it catches "major low-hanging fruit" regressions, not micro-noise.
#
# Ceilings live *in the bench file* as `hard_limits([(EventKind::Ir, …)])`, so a
# regression makes `cargo bench` exit non-zero (fails this check). Add a line per
# crate here as each feature PR lands its own `benches/*.rs`.
#
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
    pname = "agent-seddon-bench";
    version = "0.1.0";
    doInstallCargoArtifacts = false;
    nativeBuildInputs = (commonArgs.nativeBuildInputs or [ ]) ++ [
      versions.valgrind
      versions.iai-callgrind-runner
    ];
    # iai-callgrind finds its runner via this env (or PATH); pin it explicitly.
    IAI_CALLGRIND_RUNNER = "${versions.iai-callgrind-runner}/bin/iai-callgrind-runner";
    buildPhaseCargoCommand = ''
      cargo bench -p agent-metrics --bench metrics
    '';
    installPhaseCommand = "mkdir -p $out";
  }
)
