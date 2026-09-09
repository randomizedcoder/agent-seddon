# nix/pg-integration.nix
#
# `pg-integration` — the opt-in Postgres tier of `agent-config-store` (config C41
# / A2) exercised against a REAL server. The hermetic gate proves the trait over
# the bundled-SQLite + file + memory tiers; this harness proves the same matrix
# (plus MVCC rollback + a concurrent-writer conflict) over Postgres, which cannot
# run inside `nix flake check` (no docker/network in the sandbox).
#
# It is the FIRST composed `up → barrier → cargo-test → down` harness in the repo:
# it brings up the container (`postgres-up` owns the readiness barrier), runs the
# crate's `#[ignore]`-gated suite through the pinned dev shell (`nix develop -c`,
# so the toolchain matches CLAUDE.md), then tears the container down. Registered
# in `nix/integration.nix` (model-free tier, guarded by a docker check).
#
# Exit codes (the shared 0/1/2 contract): 0 clean or skipped (no docker), 1 a
# harness failure (server never came up), 2 a contract failure (a test failed).
{
  pkgs,
  lib,
  versions,
  harness,
  postgres-up,
  postgres-down,
}:
pkgs.writeShellApplication {
  name = "pg-integration";
  runtimeInputs = [
    pkgs.coreutils
    versions.docker
    pkgs.nix # runs the crate suite through the pinned dev shell
    postgres-up
    postgres-down
  ];
  text = ''
    set -uo pipefail
  ''
  + harness.contract
  + ''

    # Opt-in resource: skip-with-notice (exit 0) on a bare machine so the whole
    # `nix run .#integration` aggregate stays runnable without docker.
    if ! docker info >/dev/null 2>&1; then
      echo "pg-integration: SKIP — docker daemon not reachable (the postgres tier is opt-in)."
      contract_exit "PASS: pg-integration skipped (no docker)."
    fi

    # shellcheck disable=SC2329  # invoked indirectly via the EXIT trap below.
    cleanup() { postgres-down >/dev/null 2>&1 || true; }
    trap cleanup EXIT

    echo "==> pg-integration: bringing up Postgres"
    if ! postgres-up; then
      echo "pg-integration: postgres-up did not come up" >&2
      note_fail 1
      contract_exit "done"
    fi

    # The DSN mirrors the pins in nix/versions.nix; passed to the ignored suite,
    # which resets the tables and connects (see crates/agent-config-store/src/tests.rs).
    export AGENT_CONFIG_STORE_TEST_DSN="postgres://${versions.postgresUser}:${versions.postgresPassword}@127.0.0.1:${toString versions.postgresPort}/${versions.postgresDatabase}"

    echo "==> pg-integration: running the ignored config-store postgres suite"
    set +e
    # Single-threaded: the shared DB is reset per test, so tests must not race.
    nix develop --extra-experimental-features 'nix-command flakes' -c \
      cargo test -p agent-config-store --features config-store-postgres \
      -- --ignored --test-threads=1
    rc=$?
    set -e
    # A failing test is the very thing this tier exists to catch → CONTRACT (2).
    if [ "$rc" -ne 0 ]; then note_fail 2; fi

    # The registry convergence (config C41 / A3): the same real server proves the
    # `StoreRegistry` postgres arm (CRUD + routing agree with memory). A dedicated
    # tenant keeps it isolated, so it may share the DB with the suite above.
    echo "==> pg-integration: running the ignored registry postgres suite"
    set +e
    nix develop --extra-experimental-features 'nix-command flakes' -c \
      cargo test -p agent-registry --features registry-store-postgres \
      -- --ignored --test-threads=1
    rc=$?
    set -e
    if [ "$rc" -ne 0 ]; then note_fail 2; fi

    # The fleet convergence (config C41 / A3b): the same real server proves the
    # `StoreFleet` postgres arm. A dedicated tenant keeps it isolated.
    echo "==> pg-integration: running the ignored fleet postgres suite"
    set +e
    nix develop --extra-experimental-features 'nix-command flakes' -c \
      cargo test -p agent-review-fleet --features fleet-store-postgres \
      -- --ignored --test-threads=1
    rc=$?
    set -e
    if [ "$rc" -ne 0 ]; then note_fail 2; fi

    contract_exit "PASS: pg-integration — postgres config-store + registry + fleet suites green."
  '';
}
