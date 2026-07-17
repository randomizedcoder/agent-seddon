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

    # RustSec advisory database for the hermetic `cargo-audit` check.
    advisory-db = {
      url = "github:rustsec/advisory-db";
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
      advisory-db,
    }:
    flake-utils.lib.eachSystem [ "x86_64-linux" ] (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };
        lib = nixpkgs.lib;

        aggregator = import ./nix {
          inherit
            pkgs
            lib
            crane
            advisory-db
            ;
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
