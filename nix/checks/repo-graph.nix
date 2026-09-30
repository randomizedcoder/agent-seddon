# nix/checks/repo-graph.nix
#
# Executes the `PgRepoGraph` tier's **DB-free** unit tables — the `with_tenant`
# refusals, the read clamps, the `UNNEST` array builders and `map_db` — which sit
# behind the non-default `repo-graph-postgres` cargo feature (repo-knowledge RK-02,
# docs/design/repo-knowledge/08-test-matrix.md tier P1). The main `test` check runs
# *default* features (so it never builds the postgres tier at all), and clippy
# `--all-features` compiles + lints this code but does not run it. This dedicated,
# feature-scoped check is what actually EXECUTES the pure helpers' P1 table in the
# gate — the exact pattern of `config-store-sqlite.nix` / `fleet-sqlite.nix`.
#
# It stays hermetic by NOT passing `--ignored`: the P1 rows need no database, while
# the R3 conformance reuse and the R4 pg-only rows are `#[ignore]`-gated and run only
# under `nix run .#pg-integration` (which sets `AGENT_REPO_GRAPH_TEST_DSN`). The
# feature omits `sqlx/macros` + `sqlx/migrate`, so no `DATABASE_URL` is needed to
# build and no `sqlx-mysql`/`rsa` enters the tree.
{
  craneLib,
  commonArgs,
  cargoArtifacts,
}:

craneLib.cargoTest (
  commonArgs
  // {
    inherit cargoArtifacts;
    cargoTestExtraArgs = "-p agent-repo-graph --features repo-graph-postgres";
  }
)
