# nix/checks/registry-sqlite.nix
#
# Executes the `sqlite` ProviderRegistry tier's tests, behind the non-default
# `registry-sqlite` cargo feature (model-router 03). PG-11 retired the bespoke
# `SqliteRegistry`: the `sqlite` tier is now `StoreRegistry` over a config-store
# `SqliteBackend`, so this feature forwards `registry-store` + `config-store-sqlite`
# and the check runs `store.rs`'s `sqlite_tests` module (CRUD/route roundtrip + hostile-id
# bind-safety over the real in-memory SQLite SQL layer). The main `test` check runs
# *default* features (memory + file only), and clippy `--all-features` compiles but does
# not run it; this dedicated, feature-scoped check is what actually EXECUTES the sqlite
# path — the exact pattern of `prompt-sqlite.nix` / `fleet-sqlite.nix`. (Before PG-11 the
# bespoke `SqliteRegistry` had no dedicated check, so its tests never ran in-gate — this
# closes that gap.)
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
    cargoTestExtraArgs = "-p agent-registry --features registry-sqlite";
  }
)
