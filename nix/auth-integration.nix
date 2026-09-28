# nix/auth-integration.nix
#
# `auth-integration` — the sign-in stack over the infrastructure a deployment
# actually uses (security-hardening S15b; the S15a `auth-e2e` gate check covers the
# same chain inside the sandbox with offline certificates and file sessions):
#
# - step-ca tier (always): a real `step-ca` daemon on loopback issues every
#   certificate; renewal through the daemon keeps the service identity; a
#   certificate from another CA with the same SPIFFE name is refused.
# - Postgres tier: agent A keeps sign-in sessions in a throwaway Postgres (schema
#   applied by the agent); rows per tenant, no secret in clear, sessions and
#   revocations survive restarts.
# - ClickHouse tier: agent A writes `agent_auth_events` into a throwaway ClickHouse
#   with the shipped schema and credentials; the expected events land, the reader
#   sees one tenant, no row carries token material.
#
# Containers use their own names and ports (nix/versions.nix) and are removed on
# exit. Without a container runtime the Postgres and ClickHouse tiers are skipped
# with a notice (exit 0); $CONTAINER_RUNTIME (default docker) picks it, so the
# podman-only l2 box runs everything with CONTAINER_RUNTIME=podman. The logic lives
# in test/auth-integration/auth_integration.py (check-the-checked by the
# `auth-integration-tests` gate check). Registered in `nix/integration.nix`.
#
# Exit codes (the shared 0/1/2 contract): 0 clean or tiers skipped, 1 a harness
# failure, 2 a contract failure.
{
  pkgs,
  versions,
  agent,
}:
let
  python = pkgs.python3.withPackages (p: [
    p.pyjwt
    p.cryptography
  ]);
  # auth_integration.py imports auth-e2e and clickhouse as siblings by path
  # (Path.resolve() follows symlinks, so copies, not a linkFarm).
  src = pkgs.runCommand "auth-integration-src" { } ''
    mkdir -p "$out"
    cp -r ${../test/auth-e2e} "$out/auth-e2e"
    cp -r ${../test/clickhouse} "$out/clickhouse"
    cp -r ${../test/auth-integration} "$out/auth-integration"
  '';
in
pkgs.writeShellApplication {
  name = "auth-integration";
  runtimeInputs = [
    python
    pkgs.step-cli
    pkgs.step-ca
    versions.docker
    versions.podman
  ];
  text = ''
    exec python3 "${src}/auth-integration/auth_integration.py" \
      --agent "${agent}/bin/agent" \
      --grpcurl "${versions.grpcurl}/bin/grpcurl" \
      --step "${pkgs.step-cli}/bin/step" \
      --step-ca "${pkgs.step-ca}/bin/step-ca" \
      --pki-dev "${python}/bin/python3 ${../test/pki-dev}/pki_dev.py --step ${pkgs.step-cli}/bin/step" \
      --ca-port ${toString versions.stepCaAuthTestPort} \
      --runtime "''${CONTAINER_RUNTIME:-docker}" \
      --pg-image "${versions.postgresImage}" \
      --pg-name "${versions.postgresAuthTestContainerName}" \
      --pg-port ${toString versions.postgresAuthTestPort} \
      --ch-image "${versions.clickhouseImage}" \
      --ch-name "${versions.clickhouseAuthTestContainerName}" \
      --ch-http-port ${toString versions.clickhouseAuthTestHttpPort} \
      --ch-native-port ${toString versions.clickhouseAuthTestNativePort} \
      --schema ${./clickhouse/schema.sql} \
      --config-xml ${./clickhouse/config.xml} \
      --users-xml ${./clickhouse/users.xml} \
      "$@"
  '';
}
