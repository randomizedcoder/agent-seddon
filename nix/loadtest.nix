# nix/loadtest.nix
#
# `nix run .#loadtest` — the opt-in seam load / overload-conformance harness
# (docs/design/loadtest). NOT a check: throughput is machine-dependent, so the perf
# gate stays deterministic (iai-callgrind). The harness itself is kept honest in the
# gate by the model-free `loadtest-smoke` check (nix/checks/loadtest-smoke.nix).
#
# Compiles-on-run like `nix run .#bench` (cached after the first build). Examples:
#   nix run .#loadtest -- --scenario ramp
#   nix run .#loadtest -- --scenario overload --cap 4 --concurrency 128
{
  pkgs,
  versions,
}:
pkgs.writeShellApplication {
  name = "loadtest";
  runtimeInputs = [
    versions.rustToolchain
    versions.protobuf
  ];
  text = ''
    export PROTOC="${versions.protobuf}/bin/protoc"
    exec cargo run --release -p agent-grpc --example loadtest -- "$@"
  '';
}
