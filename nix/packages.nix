# nix/packages.nix
#
# Dev-shell package list. Kept separate from devshell.nix so the set of tools
# is a single, greppable list (mirrors the xtcp2 split).
#
{ pkgs, versions }:

{
  allDevPackages = [
    # Rust toolchain (rustc, cargo, clippy, rustfmt, rust-src).
    versions.rustToolchain

    # Rust dev/CI tooling.
    versions.cargo-audit
    versions.cargo-nextest
    versions.rust-analyzer

    # Protobuf / gRPC (protoc for tonic-build codegen; grpcurl for poking servers).
    versions.protobuf
    versions.grpcurl

    # Benchmarks: valgrind runs the iai-callgrind benches; the runner binary must
    # match the `iai-callgrind` dev-dep version (see nix/versions.nix).
    versions.valgrind
    versions.iai-callgrind-runner

    # Ops / ClickHouse.
    versions.clickhouse # clickhouse-client
    versions.docker
    versions.jq
    versions.curl

    # Nix.
    versions.nixfmt
  ];
}
