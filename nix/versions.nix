# nix/versions.nix
#
# Pinned tool versions + settings for the agent-seddon Nix flake.
#
# Single source of truth — every other module reads from here.
# Changing a version here propagates to dev shell, build derivations, and checks.
#
{ pkgs }:

{
  # Rust toolchain (via rust-overlay). `stable.latest` tracks the newest stable
  # release; pin to e.g. `pkgs.rust-bin.stable."1.90.0".default` for a frozen
  # toolchain. clippy/rustfmt/rust-src are needed by the checks + rust-analyzer.
  rustToolchain = pkgs.rust-bin.stable.latest.default.override {
    extensions = [
      "clippy"
      "rustfmt"
      "rust-src"
    ];
  };

  # Nix formatter (used by `nix fmt` and the nix-fmt check).
  nixfmt = pkgs.nixfmt-rfc-style or pkgs.nixfmt;

  # Rust dev/CI tooling.
  cargo-audit = pkgs.cargo-audit;
  cargo-nextest = pkgs.cargo-nextest;
  rust-analyzer = pkgs.rust-analyzer;

  # Runtime / ops tooling.
  clickhouse = pkgs.clickhouse; # provides `clickhouse-client` in the dev shell
  docker = pkgs.docker;
  jq = pkgs.jq;
  curl = pkgs.curl;

  # ── ClickHouse container settings ──────────────────────────────────────────
  # Pin the server image so an upstream bump is an explicit change here.
  clickhouseImage = "clickhouse/clickhouse-server:24.8";
  clickhouseContainerName = "agent-seddon-clickhouse";
  clickhouseHttpPort = 8123; # HTTP interface (/ping, clickhouse-client --port for native below)
  clickhouseNativePort = 9000; # native TCP protocol
  clickhouseDatabase = "agent";
}
