# nix/loadtest-loop.nix
#
# `nix run .#loadtest-loop` — the opt-in FULL-LOOP concurrency probe
# (docs/design/loadtest, increment 05). Builds one in-process `Agent` (scripted
# model, auto-approve, temp dirs) and drives N concurrent `agent.run()`, then
# correlates the client-side run latency against the loop's own server-side metrics
# (agent_runs_total / agent_run_duration_seconds / agent_provider_request_seconds)
# read in-process via MetricsProbe — no network in between.
#
# NOT a check (throughput is machine-dependent); kept honest in the gate by the
# model-free `loadtest-smoke` check. Compiles-on-run like `nix run .#bench`. E.g.:
#   nix run .#loadtest-loop -- --concurrency 32 --runs 512
#   nix run .#loadtest-loop -- --concurrency 8 --runs 64 --json
{
  pkgs,
  versions,
}:
pkgs.writeShellApplication {
  name = "loadtest-loop";
  runtimeInputs = [
    versions.rustToolchain
    versions.protobuf
  ];
  text = ''
    export PROTOC="${versions.protobuf}/bin/protoc"
    exec cargo run --release -p agent-runtime --example loadtest_loop -- "$@"
  '';
}
