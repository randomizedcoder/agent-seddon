# nix/versions.nix
#
# Pinned tool versions + settings for the agent-seddon Nix flake.
#
# Single source of truth — every other module reads from here.
# Changing a version here propagates to dev shell, build derivations, and checks.
#
{ pkgs }:

let
  # Per-service gRPC ports + UDS paths (single source of truth). Re-exported here
  # so every nix module reads them via `versions.grpc` / `versions.socketDir`,
  # while the Rust side gets them from the generated `constants.rs`.
  constants = import ./constants.nix;
in
{
  inherit (constants) socketDir grpc;

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

  # Protobuf / gRPC tooling. `protobuf` supplies `protoc`, which `tonic-build`
  # invokes at build time to compile `crates/agent-proto/proto/**.proto`. Pinning
  # it here keeps codegen reproducible across the dev shell, `nix build`, and the
  # checks. `grpcurl` is for manually poking gRPC servers once the transports land.
  protobuf = pkgs.protobuf;
  grpcurl = pkgs.grpcurl;

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

  # ── ClickStack (HyperDX all-in-one) settings ──────────────────────────────
  # The OTLP receiver + ClickHouse + HyperDX UI the agent's OTLP tracing exports
  # to. Pin the image so an upstream bump is an explicit change here.
  clickstackImage = "docker.hyperdx.io/hyperdx/hyperdx-all-in-one:2";
  clickstackContainerName = "agent-seddon-clickstack";
  clickstackUiPort = 8080; # HyperDX web UI
  clickstackOtlpGrpcPort = 4317; # OTLP/gRPC receiver (the endpoint the agent uses)
  clickstackOtlpHttpPort = 4318; # OTLP/HTTP receiver
}
