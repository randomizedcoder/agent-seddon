# nix/checks/default.nix
#
# Aggregates every `nix flake check` target for agent-seddon.
#
# All of these run by default. crane reuses the shared `cargoArtifacts`
# dependency build, so clippy/test/audit only recompile first-party crates.
#
{
  pkgs,
  lib,
  craneLib,
  commonArgs,
  cargoArtifacts,
  advisory-db,
  versions,
  constantsRs,
  agent,
  reviewGoCorpus,
}:

{
  clippy = import ./clippy.nix { inherit craneLib commonArgs cargoArtifacts; };
  rustfmt = import ./rustfmt.nix { inherit craneLib commonArgs; };
  test = import ./test.nix {
    inherit
      pkgs
      craneLib
      commonArgs
      cargoArtifacts
      ;
  };
  cargo-audit = import ./cargo-audit.nix { inherit craneLib commonArgs advisory-db; };
  # Deterministic perf gate (iai-callgrind under valgrind, absolute Ir ceilings)
  # + heap leak/allocation-budget gate (dhat). See docs/components/benchmarking.md.
  bench = import ./bench.nix {
    inherit
      craneLib
      commonArgs
      cargoArtifacts
      versions
      ;
  };
  leak = import ./leak.nix { inherit craneLib commonArgs cargoArtifacts; };
  nix-fmt = import ./nix-fmt.nix { inherit pkgs versions; };
  # `buf lint` + `buf breaking` over the .proto contracts (see buf.yaml). Breaking
  # is gated against the committed image; regenerate it with `nix run .#buf-image`.
  buf = import ./buf.nix { inherit pkgs versions; };
  # `constants.rs` must match what `nix/constants.nix` renders (see gen-constants).
  constants-sync = import ./constants-sync.nix {
    inherit pkgs constantsRs;
    src = commonArgs.src;
  };
  # Reproducible Go coverage for the review flow: reconstruct a flake-pinned
  # xtcp2 change and assert `agent --review` detects Go + the changed files. The
  # pinned trees are offline store paths, so this runs in the hermetic sandbox
  # (unlike the real-repo Rust sweep in `nix run .#review-eval`).
  review-go = import ./review-go.nix { inherit pkgs agent reviewGoCorpus; };
  # Static-analysis coverage for the review flow: a self-contained, stdlib-only Go
  # module with a deliberate lint hit + the pinned `go`/`golangci-lint` on PATH;
  # assert `agent --review` surfaces the finding. Offline (no module download), so
  # it runs in the hermetic sandbox. clippy is covered live (dev shell + eval).
  review-analyze = import ./review-analyze.nix { inherit pkgs versions agent; };
}
