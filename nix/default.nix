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

  # Source filter: crane's default keeps only cargo-relevant files (Cargo.toml,
  # Cargo.lock, *.rs), which would drop `crates/agent-proto/proto/**.proto` — the
  # inputs `tonic-build` needs — and the non-.rs files under `agent-search`'s
  # `tests/fixtures/` (e.g. the .nix/.md corpus its index tests search). Union
  # both back in so codegen and the fixture-driven tests have their inputs.
  cleanedSrc = lib.cleanSourceWith {
    inherit src;
    filter =
      path: type:
      (lib.hasSuffix ".proto" path)
      || (lib.hasInfix "/tests/fixtures/" path)
      || (craneLib.filterCargoSources path type);
    name = "source";
  };

  # Arguments shared by the dependency build, the package build, and the checks.
  # The harness uses rustls (not OpenSSL), so no openssl/pkg-config for TLS is
  # required; pkg-config is kept as a harmless common native input. Add
  # `pkgs.perl` here if a future `ring` bump needs it at build time.
  commonArgs = {
    src = cleanedSrc;
    strictDeps = true;
    # `protobuf` provides `protoc`; `PROTOC` points `tonic-build` (prost-build) at
    # it so `crates/agent-proto` codegen is hermetic under crane, not reliant on a
    # protoc from the ambient environment.
    nativeBuildInputs = [
      pkgs.pkg-config
      versions.protobuf
    ];
    PROTOC = "${versions.protobuf}/bin/protoc";
  };

  # Build all workspace dependencies once; reused by the package + every check.
  cargoArtifacts = craneLib.buildDepsOnly commonArgs;

  # The `agent` binary (from crates/agent-cli).
  agent = craneLib.buildPackage (commonArgs // { inherit cargoArtifacts; });

  # The generated `crates/agent-grpc/src/constants.rs` (from nix/constants.nix).
  # One derivation, shared by the `gen-constants` app and the `constants-sync`
  # check so they can never disagree.
  constantsRs = import ./gen-constants.nix { inherit pkgs versions; };

  # Copies the generated constants into the repo. Run after editing constants.nix.
  gen-constants = pkgs.writeShellApplication {
    name = "gen-constants";
    text = ''
      dest="''${1:-crates/agent-grpc/src/constants.rs}"
      cp -f ${constantsRs} "$dest"
      chmod u+w "$dest"
      echo "wrote $dest"
    '';
  };

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
      constantsRs
      ;
  };

  # Dev shell.
  devshell = import ./devshell.nix { inherit pkgs lib versions; };

  # ClickHouse container apps (up / down / client).
  clickhouse = import ./clickhouse { inherit pkgs lib versions; };

  # ClickStack / HyperDX all-in-one apps (up / down / logs / client) — the OTLP
  # trace receiver + UI.
  clickstack = import ./clickstack { inherit pkgs lib versions; };

  # Prometheus scraper + Grafana dashboards for the agent's metrics.
  prometheus = import ./prometheus { inherit pkgs lib versions; };
  grafana = import ./grafana { inherit pkgs lib versions; };
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
    gen-constants = {
      type = "app";
      program = "${gen-constants}/bin/gen-constants";
    };
    clickstack-up = {
      type = "app";
      program = "${clickstack.clickstack-up}/bin/clickstack-up";
    };
    clickstack-down = {
      type = "app";
      program = "${clickstack.clickstack-down}/bin/clickstack-down";
    };
    clickstack-logs = {
      type = "app";
      program = "${clickstack.clickstack-logs}/bin/clickstack-logs";
    };
    clickstack-client = {
      type = "app";
      program = "${clickstack.clickstack-client}/bin/clickstack-client-wrapper";
    };
    prometheus-up = {
      type = "app";
      program = "${prometheus.prometheus-up}/bin/prometheus-up";
    };
    prometheus-down = {
      type = "app";
      program = "${prometheus.prometheus-down}/bin/prometheus-down";
    };
    grafana-up = {
      type = "app";
      program = "${grafana.grafana-up}/bin/grafana-up";
    };
    grafana-down = {
      type = "app";
      program = "${grafana.grafana-down}/bin/grafana-down";
    };
  };

  formatter = versions.nixfmt;
}
