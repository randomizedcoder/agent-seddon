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
  ]
  # Language servers for the `lsp` tool (LspBackend seam) — real gopls / clangd /
  # pyright / typescript-language-server / rust-analyzer on PATH in the dev shell.
  ++ builtins.attrValues versions.lspServers
  ++ [

    # Protobuf / gRPC (protoc for tonic-build codegen; grpcurl for poking servers;
    # buf for proto lint + breaking-change checks).
    versions.protobuf
    versions.grpcurl
    versions.buf

    # Benchmarks: valgrind runs the iai-callgrind benches; the runner binary must
    # match the `iai-callgrind` dev-dep version (see nix/versions.nix).
    versions.valgrind
    versions.iai-callgrind-runner

    # Ops / ClickHouse.
    versions.clickhouse # clickhouse-client
    versions.docker
    versions.jq
    versions.curl
    versions.ripgrep # `grep` tool's fast path (falls back to the in-process walk)

    # Nix.
    versions.nixfmt
  ];
}
