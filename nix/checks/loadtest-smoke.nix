# nix/checks/loadtest-smoke.nix
#
# Model-free smoke of the load/overload harness (docs/design/loadtest). Keeps the
# harness compiling + running in `nix flake check` WITHOUT asserting any perf number
# (throughput is machine-dependent — the perf gate stays iai-callgrind). It only
# checks the harness's *behaviour*: a tiny overload run must shed with
# RESOURCE_EXHAUSTED (and only that — a non-shed error exits 2), and the ramp path
# runs clean. Fully hermetic (loopback + agent-testkit doubles, no model/network).
{
  craneLib,
  commonArgs,
  cargoArtifacts,
}:
craneLib.mkCargoDerivation (
  commonArgs
  // {
    inherit cargoArtifacts;
    pname = "agent-loadtest-smoke";
    version = "0.1.0";
    doInstallCargoArtifacts = false;
    buildPhaseCargoCommand = ''
      echo "loadtest-smoke: overload contract (must shed RESOURCE_EXHAUSTED, no other error) ..."
      cargo run --release -p agent-grpc --example loadtest -- \
        --scenario overload --cap 2 --concurrency 16 --requests 48 --require-shed --transport tcp
      echo "loadtest-smoke: ramp path runs clean ..."
      cargo run --release -p agent-grpc --example loadtest -- \
        --scenario ramp --seams tokenizer,memory --concurrency 4 --requests 40 --transport uds
    '';
    installPhaseCommand = "mkdir -p $out";
  }
)
