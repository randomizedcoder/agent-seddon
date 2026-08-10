# nix/checks/ast-cpp.nix
#
# Syntactic C/C++ code-graph coverage for the AstBackend `cpp` engine (docs/components/
# ast.md). The engine parses the tree in-process with the tree-sitter C/C++ grammars —
# no external tool, no build, no compile database — so the "hermetic check" is simply
# its table-driven test suite over checked-in C/C++ source fixtures: static call edges,
# a call chain, C++ inheritance → implementations, the `#include` dependency path, and
# the adversarial cases (binary/oversized files skipped, name-resolution bounded).
#
# Fully offline (in-crate parsing; the grammars are vendored Rust crates).
{
  craneLib,
  commonArgs,
  cargoArtifacts,
}:

craneLib.mkCargoDerivation (
  commonArgs
  // {
    inherit cargoArtifacts;
    pname = "agent-seddon-ast-cpp";
    version = "0.1.0";
    doInstallCargoArtifacts = false;
    buildPhaseCargoCommand = ''
      cargo test -p agent-ast --features ast-cpp --lib cpp:: -- --nocapture
    '';
    installPhaseCommand = "mkdir -p $out";
  }
)
