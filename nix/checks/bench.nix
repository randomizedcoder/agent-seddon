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
      cargo bench -p agent-tools --features tool-patch --bench patch
      cargo bench -p agent-tools --features tool-edit --bench edit
      cargo bench -p agent-tools --features tool-web --bench web
      cargo bench -p agent-validate --features validate-draft07 --bench validate
      cargo bench -p agent-lsp --bench lsp_parse
      cargo bench -p agent-search --features search-vector --bench vector
      cargo bench -p agent-session --bench checkpoint
      cargo bench -p agent-core --bench registry
      cargo bench -p agent-context --bench context
      cargo bench -p agent-tokenizer --bench tokenize
    '';
    installPhaseCommand = "mkdir -p $out";
  }
)
