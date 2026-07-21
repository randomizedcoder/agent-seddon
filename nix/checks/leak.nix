# nix/checks/leak.nix
#
# Heap **leak + allocation-budget** gate. Runs the dhat-instrumented tests, which
# install dhat's global allocator (only under `--features dhat-heap`) and assert
# that a hot path frees everything it allocates and stays under an allocation
# ceiling. Deterministic and sandbox-safe (no valgrind).
#
# Per-crate because the `dhat-heap` feature is per-crate — add a line here as each
# feature PR lands its own `tests/leak.rs`.
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
    pname = "agent-seddon-leak";
    version = "0.1.0";
    doInstallCargoArtifacts = false;
    buildPhaseCargoCommand = ''
      cargo test -p agent-metrics --features dhat-heap --test leak
      cargo test -p agent-tools --features dhat-heap,tool-patch,tool-edit,tool-search,tool-web --test leak
      cargo test -p agent-tokenizer --features dhat-heap --test leak
      cargo test -p agent-tasks --features dhat-heap --test leak
      cargo test -p agent-validate --features dhat-heap --test leak
      cargo test -p agent-lsp --features dhat-heap --test leak
      cargo test -p agent-sandbox --features dhat-heap --test leak
      cargo test -p agent-search --features dhat-heap,search-vector --test leak
      cargo test -p agent-session --features dhat-heap --test leak
      cargo test -p agent-reference --features dhat-heap --test leak
      cargo test -p agent-scanner --features dhat-heap --test leak
    '';
    installPhaseCommand = "mkdir -p $out";
  }
)
