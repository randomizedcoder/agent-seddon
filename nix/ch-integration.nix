# nix/ch-integration.nix
#
# `ch-integration` — the ClickHouse credential + row-level-security matrix
# (security-hardening S16) against a REAL, throwaway ClickHouse: its own container
# name and ports (nix/versions.nix), a fresh credentials directory, the repo's
# config.xml / users.xml / schema.sql, then every login and tenant boundary asserted,
# plus the ignored Rust test for the shared-connection scope bug. Never touches the
# long-lived agent ClickHouse. The logic lives in test/clickhouse/rls_harness.py
# (its matcher is check-the-checked by the `ch-creds-tests` gate check).
#
# Registered in `nix/integration.nix` (model-free tier). Self-skips (exit 0) without a
# container runtime; honours $CONTAINER_RUNTIME (default docker) like every container
# app, so the podman-only l2 box runs it with CONTAINER_RUNTIME=podman.
#
# Exit codes (the shared 0/1/2 contract): 0 clean or skipped, 1 a harness failure,
# 2 a contract failure (a matrix row did not hold).
{
  pkgs,
  versions,
}:
pkgs.writeShellApplication {
  name = "ch-integration";
  runtimeInputs = [
    pkgs.python3
    pkgs.nix # the Rust regression runs through the pinned dev shell
    versions.docker
    versions.podman
  ];
  text = ''
    exec python3 "${../test/clickhouse}/rls_harness.py" \
      --runtime "''${CONTAINER_RUNTIME:-docker}" \
      --image "docker.io/${versions.clickhouseImage}" \
      --name "${versions.clickhouseRlsTestContainerName}" \
      --http-port ${toString versions.clickhouseRlsTestHttpPort} \
      --native-port ${toString versions.clickhouseRlsTestNativePort} \
      --schema ${./clickhouse/schema.sql} \
      --config-xml ${./clickhouse/config.xml} \
      --users-xml ${./clickhouse/users.xml} \
      --cargo "$@"
  '';
}
