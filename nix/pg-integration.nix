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
    pkgs.python3
    pkgs.gnugrep
    pkgs.coreutils
    versions.docker
    versions.podman # a podman-only host (l2) probes + runs via CONTAINER_RUNTIME=podman
    pkgs.git
    pkgs.nix # runs the crate suite through the pinned dev shell
    postgres-up
    postgres-down
  ];
  text = ''
    set -uo pipefail
  ''
  + harness.contract
  + ''

    export AGENT_PG_ARTIFACT_DIR="''${AGENT_PG_ARTIFACT_DIR:-$(mktemp -d /tmp/agent-pg-results.XXXXXX)}"
    mkdir -p "$AGENT_PG_ARTIFACT_DIR"
    export AGENT_TEST_REVISION="''${AGENT_TEST_REVISION:-$(git rev-parse HEAD 2>/dev/null || echo unknown)}"
    record() { python3 ${./pg-suite.py} "$1" record "$2" "$3"; }
    expected_suites="config-store registry review-fleet prompt role runtime-pg_tenant_tests scheduler-pg_tests forge-pg_tests slack-pg_tests digest runtime-pg_digest_wiring_tests campaign repo-graph repo-graph-index repo-graph-operator"
    record harness failed "incomplete: run started"
    for suite in $expected_suites; do record "$suite" failed "incomplete: not run"; done
    runtime="''${CONTAINER_RUNTIME:-docker}"
    if ! "$runtime" info >/dev/null 2>&1; then
      if [ "''${AGENT_REQUIRE_POSTGRES_TESTS:-0}" = 1 ]; then
        record harness failed "required container runtime unavailable"
        note_fail 1
        contract_exit "pg-integration: required runtime unavailable"
      fi
      for suite in $expected_suites; do record "$suite" skipped "optional container runtime unavailable"; done
      record harness skipped "optional container runtime unavailable"
      contract_exit "pg-integration: SKIPPED (no container runtime; no tests executed)"
    fi

    # Cleanup failure overrides a successful run and is persisted on every exit path.
    # shellcheck disable=SC2329
    cleanup() {
      local result=$? names
      trap - EXIT
      if ! "$runtime" info >/dev/null 2>&1 || ! postgres-down >"$AGENT_PG_ARTIFACT_DIR/cleanup.log" 2>&1; then
        record cleanup failed "container cleanup failed"
        record harness failed "container cleanup failed"
        exit 1
      fi
      if ! names="$("$runtime" ps -a --format '{{.Names}}')" || printf '%s\n' "$names" | grep -qx '${versions.postgresContainerName}'; then
        record cleanup failed "container removal could not be verified"
        record harness failed "container removal could not be verified"
        exit 1
      fi
      record cleanup passed "container removed"
      exit "$result"
    }
    trap cleanup EXIT
    if ! postgres-up >"$AGENT_PG_ARTIFACT_DIR/startup.log" 2>&1; then
      record harness failed "Postgres startup failed"
      note_fail 1
      contract_exit "pg-integration: startup failed"
    fi

    run_suite() {
      local suite="$1" result
      shift
      result=0
      python3 ${./pg-suite.py} "$suite" nix develop --extra-experimental-features 'nix-command flakes' -c "$@" || result=$?
      if [ "$result" -ne 0 ]; then note_fail "$result"; fi
    }

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
    # The repo-graph store (repo-knowledge RK-02) shares the server and the
    # `tenants` table; its own versioned runner (distinct advisory-lock key)
    # creates `repos` / `graph_snapshots` / `graph_nodes` / `graph_node_versions`
    # / `graph_edges`, and its suite truncates only those five.
    export AGENT_REPO_GRAPH_TEST_DSN="$AGENT_CONFIG_STORE_TEST_DSN"

    echo "==> pg-integration: running the ignored config-store postgres suite"
    run_suite config-store cargo test -p agent-config-store --features config-store-postgres -- --ignored --test-threads=1

    # The registry convergence (config C41 / A3): the same real server proves the
    # `StoreRegistry` postgres arm (CRUD + routing agree with memory). A dedicated
    # tenant keeps it isolated, so it may share the DB with the suite above.
    echo "==> pg-integration: running the ignored registry postgres suite"
    run_suite registry cargo test -p agent-registry --features registry-store-postgres -- --ignored --test-threads=1

    # The fleet convergence (config C41 / A3b): the same real server proves the
    # `StoreFleet` postgres arm AND the `StorePostLease` approve→post idempotency lease
    # (review-fleet C17 / PG-09) — the whole crate's `#[ignore]` suite under
    # `fleet-store-postgres`, incl. `pg_lease_tests` (reconnect durability + a
    # concurrent-acquire race). Dedicated tenants keep them isolated.
    echo "==> pg-integration: running the ignored fleet + post-lease postgres suites"
    run_suite review-fleet cargo test -p agent-review-fleet --features fleet-store-postgres -- --ignored --test-threads=1

    # The prompt convergence (config C41 / A3c): the same real server proves the
    # `StorePrompt` postgres arm. A dedicated tenant keeps it isolated.
    echo "==> pg-integration: running the ignored prompt postgres suite"
    run_suite prompt cargo test -p agent-prompt --features prompt-store-postgres -- --ignored --test-threads=1

    # The RBAC role convergence (config C1b): the same real server proves the
    # `StoreRoles` postgres arm. A dedicated tenant keeps it isolated.
    echo "==> pg-integration: running the ignored role postgres suite"
    run_suite role cargo test -p agent-role --features role-store-postgres -- --ignored --test-threads=1

    # The per-tenant plane (config C2): the same real server proves `PerTenant`
    # isolates tenants over the postgres store, routed by verified identity.
    echo "==> pg-integration: running the ignored per-tenant postgres suite"
    run_suite runtime-pg_tenant_tests cargo test -p agent-runtime --features registry-postgres -- --ignored --test-threads=1 pg_tenant_tests

    # The durable scheduler (config C2c): a job scheduled under one verified tenant
    # is invisible to another over the postgres store — the tenant-keying the
    # tenant-fanning driver relies on, proven over a real server.
    echo "==> pg-integration: running the ignored scheduler postgres suite"
    run_suite scheduler-pg_tests cargo test -p agent-scheduler --features scheduler-store-postgres -- --ignored --test-threads=1 pg_tests

    # The forge registry (config C36 / D1): the same real server proves the
    # `StoreForges` postgres arm. A dedicated tenant keeps it isolated.
    echo "==> pg-integration: running the ignored forge-registry postgres suite"
    run_suite forge-pg_tests cargo test -p agent-forge --features forge-store-postgres -- --ignored --test-threads=1 pg_tests

    # The transport registry (config C37 / D2): the same real server proves the
    # `StoreTransports` postgres arm. A dedicated tenant keeps it isolated.
    echo "==> pg-integration: running the ignored transport-registry postgres suite"
    run_suite slack-pg_tests cargo test -p agent-slack --features transport-store-postgres -- --ignored --test-threads=1 pg_tests

    # The digest ledger (PG-07/PG-08): the crate-local `PgDigests` suite proves the
    # DigestStore contract over the real server; its own versioned runner creates
    # the `digests` table. Shares the DB (distinct tables), so no reset needed.
    echo "==> pg-integration: running the ignored digest postgres suite"
    run_suite digest cargo test -p agent-digest --features digest-postgres -- --ignored --test-threads=1

    # The runtime wiring (config C41 / PG-08): the `[digest] store = "postgres"`
    # arm builds `PgDigests` from the shared `[config_store] dsn_ref` and
    # round-trips a row end-to-end through the builder helper.
    echo "==> pg-integration: running the ignored digest wiring suite"
    run_suite runtime-pg_digest_wiring_tests cargo test -p agent-runtime --features digest-postgres -- --ignored --test-threads=1 pg_digest_wiring_tests

    # The campaign store (campaigns CP-02): `PgCampaigns` proves the `CampaignStore`
    # contract over the real server — the shared T3–T8 conformance rows (each
    # followed by the T15 invariants query), T14 multi-tenant isolation, and the
    # three concurrency cases (two pools, `SKIP LOCKED`, one-winner decompose).
    echo "==> pg-integration: running the ignored campaign postgres suite"
    run_suite campaign cargo test -p agent-campaign --features campaign-postgres -- --ignored --test-threads=1

    # The repo-graph store (repo-knowledge RK-02): `PgRepoGraph` proves the
    # RepoGraphStore contract over the real server — the shared R3 conformance rows
    # (each followed by `assert_invariants`) plus the R4 pg-only rows (migration
    # idempotence, shared bodies, the id-collision rollback, concurrent
    # duplicate-identity, cross-tenant isolation, orphan-body sweep, corrupt-row
    # decode, a CHECK -> Invalid, the diff cap). Its own versioned runner creates
    # the graph tables; the suite truncates only those five.
    echo "==> pg-integration: running the ignored repo-graph postgres suite"
    run_suite repo-graph cargo test -p agent-repo-graph --features repo-graph-postgres -- --ignored --test-threads=1

    run_suite repo-graph-index cargo test -p agent-repo-graph --features index,repo-graph-postgres --test indexer_postgres -- --ignored --test-threads=1
    run_suite repo-graph-operator cargo test -p agent-runtime --features repo-graph -- --ignored --test-threads=1 positive_pg_repo_index_operator_workflow
    # Persist the identities and graph hashes exercised by I2 before teardown.
    # Only fixture-owned tenants are exported, and output is bounded by the query.
    if "$runtime" exec '${versions.postgresContainerName}' psql -X -U '${versions.postgresUser}' -d '${versions.postgresDatabase}' -At \
      -c "SELECT json_build_object('tenant', tenant, 'repo_id', repo_id, 'snapshot_id', snapshot_id, 'commit_sha', commit_sha, 'extractor_version', extractor_version, 'graph_hash', graph_hash, 'status', status, 'node_count', node_count, 'edge_count', edge_count) FROM graph_snapshots WHERE tenant LIKE 'rk06-%' ORDER BY tenant, snapshot_id LIMIT 200" \
      >"$AGENT_PG_ARTIFACT_DIR/indexed-snapshots.jsonl" 2>"$AGENT_PG_ARTIFACT_DIR/snapshot-export.log"; then
      record snapshot-export passed "fixture snapshot identities and hashes exported"
    else
      record snapshot-export failed "fixture snapshot export failed"
      note_fail 1
    fi
    if [ "$worst" -eq 0 ]; then
      record harness passed "all required suites executed"
    else
      record harness failed "one or more suites failed; inspect suite results"
    fi

    contract_exit "PASS: pg-integration — postgres config-store + registry + fleet + prompt + role + per-tenant + scheduler + forge + transport + digest + campaign + repo-graph suites green."
  '';
}
