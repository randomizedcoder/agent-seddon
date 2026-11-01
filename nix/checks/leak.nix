# nix/checks/leak.nix
#
# Heap **leak + allocation-budget** gate. Runs the dhat-instrumented tests, which
# install dhat's global allocator (only under `--features dhat-heap`) and assert
# that a hot path frees everything it allocates and stays under an allocation
# ceiling. Deterministic and sandbox-safe (no valgrind).
#
# One `leak-<crate>` check per crate below, each on that crate's source slice
# (nix/workspace-slice.nix), so an edit re-runs only the leak tests that can see it;
# `nix/checks/default.nix` aggregates them as `leak`. The `dhat-heap` feature is
# per-crate: add a row here as each feature PR lands its own `tests/leak.rs`.
#
{
  lib,
  craneLib,
  commonArgs,
  workspaceSlice,
}:

let
  # crate → the `cargo test -p <crate>` arguments of each of its leak tests.
  runs = {
    agent-metrics = [ "--features dhat-heap --test leak" ];
    agent-tools = [ "--features dhat-heap,tool-patch,tool-edit,tool-search,tool-web --test leak" ];
    agent-tokenizer = [ "--features dhat-heap --test leak" ];
    agent-tasks = [ "--features dhat-heap --test leak" ];
    agent-validate = [ "--features dhat-heap --test leak" ];
    agent-lsp = [ "--features dhat-heap --test leak" ];
    agent-sandbox = [ "--features dhat-heap --test leak" ];
    agent-search = [ "--features dhat-heap,search-vector,search-tantivy --test leak" ];
    agent-ast = [ "--features dhat-heap --test leak" ];
    agent-repo-graph = [ "--features extract-rust,dhat-heap --test leak" ];
    agent-session = [ "--features dhat-heap --test leak" ];
    agent-reference = [ "--features dhat-heap --test leak" ];
    agent-scanner = [ "--features dhat-heap --test leak" ];
    agent-cache = [ "--features dhat-heap --test leak" ];
    agent-web-search = [ "--features dhat-heap --test leak" ];
    agent-export = [ "--features dhat-heap --test leak" ];
    agent-mode = [ "--features dhat-heap --test leak" ];
    agent-context = [ "--features dhat-heap,context-mode-aware --test leak" ];
    agent-memory = [ "--features dhat-heap,memory-dimensions --test leak" ];
    agent-providers = [
      "--features dhat-heap,provider-pool --test leak"
      "--features dhat-heap,provider-router --test route_leak"
      "--features dhat-heap,provider-branching --test branch_leak"
    ];
    agent-digest = [ "--features dhat-heap,digest-sqlite --test leak" ];
    agent-graph = [ "--features dhat-heap --test leak" ];
    agent-registry = [ "--features dhat-heap --test leak" ];
  };

  leakCheck =
    crate: args:
    craneLib.mkCargoDerivation (
      commonArgs
      // workspaceSlice.sliceArgs [ crate ]
      // {
        pname = "leak-${crate}";
        version = "0.1.0";
        doInstallCargoArtifacts = false;
        buildPhaseCargoCommand = lib.concatMapStrings (a: "cargo test -p ${crate} ${a}\n") args;
        installPhaseCommand = "mkdir -p $out";
      }
    );
in
lib.mapAttrs' (crate: args: lib.nameValuePair "leak-${crate}" (leakCheck crate args)) runs
