# nix/default.nix
#
# Aggregator. Returns the per-system attribute set consumed by flake.nix.
#
{
  pkgs,
  lib,
  src,
  crane,
  advisory-db,
}:

let
  versions = import ./versions.nix { inherit pkgs; };

  # crane, bound to our pinned Rust toolchain.
  craneLib = (crane.mkLib pkgs).overrideToolchain versions.rustToolchain;

  # Arguments shared by the dependency build, the package build, and the checks.
  # The harness uses rustls (not OpenSSL), so no openssl/pkg-config for TLS is
  # required; pkg-config is kept as a harmless common native input. Add
  # `pkgs.perl` here if a future `ring` bump needs it at build time.
  commonArgs = {
    src = craneLib.cleanCargoSource src;
    strictDeps = true;
    nativeBuildInputs = [ pkgs.pkg-config ];
  };

  # Build all workspace dependencies once; reused by the package + every check.
  cargoArtifacts = craneLib.buildDepsOnly commonArgs;

  # The `agent` binary (from crates/agent-cli).
  agent = craneLib.buildPackage (commonArgs // { inherit cargoArtifacts; });

  # Static analysis + tests.
  checks = import ./checks {
    inherit
      pkgs
      lib
      craneLib
      commonArgs
      cargoArtifacts
      advisory-db
      versions
      ;
  };

  # Dev shell.
  devshell = import ./devshell.nix { inherit pkgs lib versions; };

  # ClickHouse container apps (up / down / client).
  clickhouse = import ./clickhouse { inherit pkgs lib versions; };
in
{
  packages = {
    inherit agent;
    default = agent;
  };

  devShells.default = devshell;

  inherit checks;

  apps = {
    agent = {
      type = "app";
      program = "${agent}/bin/agent";
    };
    clickhouse-up = {
      type = "app";
      program = "${clickhouse.clickhouse-up}/bin/clickhouse-up";
    };
    clickhouse-down = {
      type = "app";
      program = "${clickhouse.clickhouse-down}/bin/clickhouse-down";
    };
    clickhouse-client = {
      type = "app";
      program = "${clickhouse.clickhouse-client}/bin/clickhouse-client-wrapper";
    };
  };

  formatter = versions.nixfmt;
}
