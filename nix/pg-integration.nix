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
# in `nix/integration.nix` (model-free tier; self-skips without a container runtime).
#
# The container runtime is picked the same way every container app picks it
# (`nix/lib/mk-container-app.nix`): `$CONTAINER_RUNTIME` (default `docker`), so a
# docker-less, podman-only host (e.g. the headless l2 box) runs this harness with
# `CONTAINER_RUNTIME=podman` instead of self-skipping — `postgres-up`/`-down`
# inherit the same env var.
#
# Exit codes (the shared 0/1/2 contract): 0 clean or skipped (no runtime), 1 a
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
    versions.podman # a podman-only host (l2) probes + runs via CONTAINER_RUNTIME=podman
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
    # `nix run .#integration` aggregate stays runnable without a container runtime.
    # Pick the runtime like every container app does (mk-container-app.nix): honor
    # $CONTAINER_RUNTIME (default docker) so a podman-only host is not skipped.
    runtime="''${CONTAINER_RUNTIME:-docker}"
    if ! "$runtime" info >/dev/null 2>&1; then
      echo "pg-integration: SKIP — container runtime ($runtime) not reachable (the postgres tier is opt-in)."
      contract_exit "PASS: pg-integration skipped (no container runtime)."
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
    # The digest ledger (PG-08) shares the same server; its `#[ignore]` suite
    # gates on AGENT_DIGEST_TEST_DSN, and its own versioned runner creates the
    # `digests` table (distinct from the config-store `cards`/`tenants`).
    export AGENT_DIGEST_TEST_DSN="$AGENT_CONFIG_STORE_TEST_DSN"
    # The campaign store (campaigns CP-02) shares the server and the `tenants`
    # table; its own versioned runner creates `tasks` / `task_events` /
    # `task_attempts`, and its suite truncates only those three.
    export AGENT_CAMPAIGN_TEST_DSN="$AGENT_CONFIG_STORE_TEST_DSN"

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
    # `StoreFleet` postgres arm AND the `StorePostLease` approve→post idempotency lease
    # (review-fleet C17 / PG-09) — the whole crate's `#[ignore]` suite under
    # `fleet-store-postgres`, incl. `pg_lease_tests` (reconnect durability + a
    # concurrent-acquire race). Dedicated tenants keep them isolated.
    echo "==> pg-integration: running the ignored fleet + post-lease postgres suites"
    set +e
    nix develop --extra-experimental-features 'nix-command flakes' -c \
      cargo test -p agent-review-fleet --features fleet-store-postgres \
      -- --ignored --test-threads=1
    rc=$?
    set -e
    if [ "$rc" -ne 0 ]; then note_fail 2; fi

    # The prompt convergence (config C41 / A3c): the same real server proves the
    # `StorePrompt` postgres arm. A dedicated tenant keeps it isolated.
    echo "==> pg-integration: running the ignored prompt postgres suite"
    set +e
    nix develop --extra-experimental-features 'nix-command flakes' -c \
      cargo test -p agent-prompt --features prompt-store-postgres \
      -- --ignored --test-threads=1
    rc=$?
    set -e
    if [ "$rc" -ne 0 ]; then note_fail 2; fi

    # The RBAC role convergence (config C1b): the same real server proves the
    # `StoreRoles` postgres arm. A dedicated tenant keeps it isolated.
    echo "==> pg-integration: running the ignored role postgres suite"
    set +e
    nix develop --extra-experimental-features 'nix-command flakes' -c \
      cargo test -p agent-role --features role-store-postgres \
      -- --ignored --test-threads=1
    rc=$?
    set -e
    if [ "$rc" -ne 0 ]; then note_fail 2; fi

    # The per-tenant plane (config C2): the same real server proves `PerTenant`
    # isolates tenants over the postgres store, routed by verified identity.
    echo "==> pg-integration: running the ignored per-tenant postgres suite"
    set +e
    nix develop --extra-experimental-features 'nix-command flakes' -c \
      cargo test -p agent-runtime --features registry-postgres \
      -- --ignored --test-threads=1 pg_tenant_tests
    rc=$?
    set -e
    if [ "$rc" -ne 0 ]; then note_fail 2; fi

    # The durable scheduler (config C2c): a job scheduled under one verified tenant
    # is invisible to another over the postgres store — the tenant-keying the
    # tenant-fanning driver relies on, proven over a real server.
    echo "==> pg-integration: running the ignored scheduler postgres suite"
    set +e
    nix develop --extra-experimental-features 'nix-command flakes' -c \
      cargo test -p agent-scheduler --features scheduler-store-postgres \
      -- --ignored --test-threads=1 pg_tests
    rc=$?
    set -e
    if [ "$rc" -ne 0 ]; then note_fail 2; fi

    # The forge registry (config C36 / D1): the same real server proves the
    # `StoreForges` postgres arm. A dedicated tenant keeps it isolated.
    echo "==> pg-integration: running the ignored forge-registry postgres suite"
    set +e
    nix develop --extra-experimental-features 'nix-command flakes' -c \
      cargo test -p agent-forge --features forge-store-postgres \
      -- --ignored --test-threads=1 pg_tests
    rc=$?
    set -e
    if [ "$rc" -ne 0 ]; then note_fail 2; fi

    # The transport registry (config C37 / D2): the same real server proves the
    # `StoreTransports` postgres arm. A dedicated tenant keeps it isolated.
    echo "==> pg-integration: running the ignored transport-registry postgres suite"
    set +e
    nix develop --extra-experimental-features 'nix-command flakes' -c \
      cargo test -p agent-slack --features transport-store-postgres \
      -- --ignored --test-threads=1 pg_tests
    rc=$?
    set -e
    if [ "$rc" -ne 0 ]; then note_fail 2; fi

    # The digest ledger (PG-07/PG-08): the crate-local `PgDigests` suite proves the
    # DigestStore contract over the real server; its own versioned runner creates
    # the `digests` table. Shares the DB (distinct tables), so no reset needed.
    echo "==> pg-integration: running the ignored digest postgres suite"
    set +e
    nix develop --extra-experimental-features 'nix-command flakes' -c \
      cargo test -p agent-digest --features digest-postgres \
      -- --ignored --test-threads=1
    rc=$?
    set -e
    if [ "$rc" -ne 0 ]; then note_fail 2; fi

    # The runtime wiring (config C41 / PG-08): the `[digest] store = "postgres"`
    # arm builds `PgDigests` from the shared `[config_store] dsn_ref` and
    # round-trips a row end-to-end through the builder helper.
    echo "==> pg-integration: running the ignored digest wiring suite"
    set +e
    nix develop --extra-experimental-features 'nix-command flakes' -c \
      cargo test -p agent-runtime --features digest-postgres \
      -- --ignored --test-threads=1 pg_digest_wiring_tests
    rc=$?
    set -e
    if [ "$rc" -ne 0 ]; then note_fail 2; fi

    # The campaign store (campaigns CP-02): `PgCampaigns` proves the `CampaignStore`
    # contract over the real server — the shared T3–T8 conformance rows (each
    # followed by the T15 invariants query), T14 multi-tenant isolation, and the
    # three concurrency cases (two pools, `SKIP LOCKED`, one-winner decompose).
    echo "==> pg-integration: running the ignored campaign postgres suite"
    set +e
    nix develop --extra-experimental-features 'nix-command flakes' -c \
      cargo test -p agent-campaign --features campaign-postgres \
      -- --ignored --test-threads=1
    rc=$?
    set -e
    if [ "$rc" -ne 0 ]; then note_fail 2; fi

    contract_exit "PASS: pg-integration — postgres config-store + registry + fleet + prompt + role + per-tenant + scheduler + forge + transport + digest + campaign suites green."
  '';
}
