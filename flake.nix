#
# flake.nix — agent-seddon
#
# Thin orchestrator. Every concern lives under ./nix/ and is wired up here.
# See ./nix/default.nix for the per-system aggregator.
#
# Quick references:
#   nix develop                          # dev shell (pinned Rust toolchain + tools)
#   nix build   .#agent                  # build the `agent` binary
#   nix run     .#agent -- --config config/agent.toml "list files"
#   nix flake check                      # clippy + rustfmt + tests + cargo-audit + nix-fmt
#   nix fmt                              # format all .nix files
#   nix run     .#clickhouse-up          # start the ClickHouse container (docker)
#   nix run     .#clickhouse-client -- -q 'SHOW TABLES FROM agent'
#   nix run     .#clickhouse-down        # stop + remove the container
#
{
  description = "agent-seddon — experimental modular coding-agent harness in Rust";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";

    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    crane.url = "github:ipetkov/crane";

    # Charon (AeneasVerif) — the rustc-driver MIR extractor behind the `rust`
    # AstBackend engine (docs/components/ast.md). It dumps a crate's type decls,
    # function bodies (with resolved call sites), and trait implementations to a
    # JSON `.llbc`, which the engine ingests for the precise Rust call graph +
    # implementations. Not in nixpkgs, so pinned as a flake input (frozen via
    # flake.lock); the flake bundles its own rustc toolchain, so its build is
    # independent of our pinned stable toolchain. We consume only the `charon`
    # binary (put on PATH for the engine's `Sandbox` invocation), never its lib.
    charon.url = "github:AeneasVerif/charon";

    # RustSec advisory database for the hermetic `cargo-audit` check.
    advisory-db = {
      url = "github:rustsec/advisory-db";
      flake = false;
    };

    # Pinned Go source trees for the code-review evaluation corpus
    # (docs/design/code-review/eval/). Each is a hash-locked snapshot of a
    # code-heavy xtcp2 change (base + head commit), so the Go base rate is
    # reproducible and independent of any local clone. See the eval README.
    # #56 feat/s3-secret-file (2 .go files):
    xtcp2-56-base = {
      url = "github:randomizedcoder/xtcp2/a36a9334fbe5ef4007ef57c5b32a004dac4796fd";
      flake = false;
    };
    xtcp2-56-head = {
      url = "github:randomizedcoder/xtcp2/6ea5ffd3821cfc1e404fae9d5637ceb393aec23f";
      flake = false;
    };
    # #52 fix-ns-churn-thread-leak (5 .go files, incl. a race test):
    xtcp2-52-base = {
      url = "github:randomizedcoder/xtcp2/6903ea55b5b4585e77a67ba7dcaab6d943a99298";
      flake = false;
    };
    xtcp2-52-head = {
      url = "github:randomizedcoder/xtcp2/07f1469d40ac0642c2b27ecd49d5473fdf40a3b2";
      flake = false;
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      rust-overlay,
      crane,
      charon,
      advisory-db,
      xtcp2-56-base,
      xtcp2-56-head,
      xtcp2-52-base,
      xtcp2-52-head,
    }:
    flake-utils.lib.eachSystem [ "x86_64-linux" ] (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };
        lib = nixpkgs.lib;

        # The pinned Go review-eval corpus, as { label = { base; head; }; } store
        # paths passed to the eval app + the review-go check.
        reviewGoCorpus = {
          "s3-secret-file" = {
            base = xtcp2-56-base;
            head = xtcp2-56-head;
          };
          "ns-churn-thread-leak" = {
            base = xtcp2-52-base;
            head = xtcp2-52-head;
          };
        };

        aggregator = import ./nix {
          inherit
            pkgs
            lib
            crane
            advisory-db
            reviewGoCorpus
            ;
          # The charon binary for this system (the `rust` AstBackend engine's MIR
          # extractor). Passed through to nix/versions.nix as the single pin point.
          charonPkg = charon.packages.${system}.charon;
          src = ./.;
        };
      in
      {
        inherit (aggregator)
          packages
          devShells
          checks
          apps
          formatter
          ;
      }
    );
}
