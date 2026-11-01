# nix/checks/bench.nix
#
# Performance gate. Runs the iai-callgrind benches under callgrind and fails if a
# bench exceeds its **absolute** instruction ceiling (`--callgrind-limits='ir=…'`).
# callgrind counts are deterministic given the pinned valgrind + toolchain, so an
# absolute ceiling is a stable, baseline-free gate for a stateless `nix flake
# check` — it catches "major low-hanging fruit" regressions, not micro-noise.
#
# Ceilings live *in the bench file* as `hard_limits([(EventKind::Ir, …)])`, so a
# regression makes `cargo bench` exit non-zero (fails this check).
#
# One `bench-<crate>` check per crate below, each on that crate's source slice
# (nix/workspace-slice.nix), so an edit re-runs only the benches that can see it;
# `nix/checks/default.nix` aggregates them as `bench`. Add a row here as each
# feature PR lands its own `benches/*.rs`.
#
{
  lib,
  craneLib,
  commonArgs,
  workspaceSlice,
  versions,
}:

let
  # crate → the `cargo bench -p <crate>` arguments of each of its benches.
  runs = {
    agent-metrics = [ "--bench metrics" ];
    agent-tools = [
      "--features tool-patch --bench patch"
      "--features tool-edit --bench edit"
      "--features tool-web --bench web"
    ];
    agent-validate = [ "--features validate-draft07 --bench validate" ];
    agent-lsp = [ "--bench lsp_parse" ];
    agent-search = [ "--features search-vector --bench vector" ];
    agent-ast = [ "--bench ast" ];
    agent-repo-graph = [ "--features extract-rust --bench extract" ];
    agent-session = [ "--bench checkpoint" ];
    agent-reference = [ "--bench parse" ];
    agent-scanner = [ "--bench scan" ];
    agent-cache = [ "--bench place" ];
    agent-web-search = [ "--bench rank" ];
    agent-export = [ "--bench render" ];
    agent-scheduler = [ "--bench next_fire" ];
    agent-core = [ "--bench registry" ];
    agent-context = [
      "--bench context"
      "--features context-mode-aware --bench mode_aware"
      "--features context-instant --bench instant_assemble"
    ];
    agent-mode = [ "--bench classify" ];
    agent-memory = [ "--features memory-dimensions --bench dimensions" ];
    agent-providers = [
      "--features provider-pool --bench pool_select"
      "--bench route_resolve"
      "--features provider-consensus --bench gate_verdict"
      "--features provider-branching --bench branch_dispatch"
    ];
    agent-digest = [ "--features digest-sqlite --bench digest_query" ];
    agent-graph = [ "--bench graph_load" ];
    agent-registry = [ "--bench registry_route" ];
    agent-tokenizer = [ "--bench tokenize" ];
  };

  benchCheck =
    crate: args:
    craneLib.mkCargoDerivation (
      commonArgs
      // workspaceSlice.sliceArgs [ crate ]
      // {
        pname = "bench-${crate}";
        version = "0.1.0";
        doInstallCargoArtifacts = false;
        nativeBuildInputs = (commonArgs.nativeBuildInputs or [ ]) ++ [
          versions.valgrind
          versions.iai-callgrind-runner
        ];
        # iai-callgrind finds its runner via this env (or PATH); pin it explicitly.
        IAI_CALLGRIND_RUNNER = "${versions.iai-callgrind-runner}/bin/iai-callgrind-runner";
        buildPhaseCargoCommand = lib.concatMapStrings (a: "cargo bench -p ${crate} ${a}\n") args;
        installPhaseCommand = "mkdir -p $out";
      }
    );
in
lib.mapAttrs' (crate: args: lib.nameValuePair "bench-${crate}" (benchCheck crate args)) runs
