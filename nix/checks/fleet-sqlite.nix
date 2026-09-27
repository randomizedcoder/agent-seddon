# nix/checks/fleet-sqlite.nix
#
# Executes the `sqlite` FleetRegistry roster tier's tests, behind the non-default
# `fleet-sqlite` cargo feature (review-fleet C2, docs/design/review-fleet/03-fleet-core.md).
# PG-11 retired the bespoke `SqliteFleet`: the `sqlite` tier is now `StoreFleet` over a
# config-store `SqliteBackend`, so this feature forwards `fleet-store` + `config-store-sqlite`
# and the check runs `store.rs`'s `sqlite_tests` module (CRUD/agree roundtrip + hostile-id
# bind-safety over the real in-memory SQLite SQL layer). The main `test` check runs *default*
# features (memory + file only), and clippy `--all-features` compiles but does not run it;
# this dedicated, feature-scoped check is what actually EXECUTES the sqlite path — the exact
# pattern of `prompt-sqlite.nix` / `registry-sqlite.nix`.
#
# `rusqlite`'s `bundled` feature (pulled by `config-store-sqlite`) compiles vendored
# `sqlite3.c` with the stdenv C toolchain crane already provides — no system libsqlite3.
{
  craneLib,
  commonArgs,
  cargoArtifacts,
}:

craneLib.cargoTest (
  commonArgs
  // {
    inherit cargoArtifacts;
    cargoTestExtraArgs = "-p agent-review-fleet --features fleet-sqlite";
  }
)
