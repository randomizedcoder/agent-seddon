# nix/checks/prompt-sqlite.nix
#
# Executes the `sqlite` PromptStore tier's HERMETIC tests, behind the non-default
# `prompt-sqlite` cargo feature (docs/design/prompts/05-storage.md). PG-10 retired the
# bespoke `SqlitePromptStore`: the `sqlite` tier is now `StorePrompt` over a config-store
# `SqliteBackend`, so this feature forwards `prompt-store` + `config-store-sqlite` and
# the check runs `store.rs`'s `sqlite_tests` module (CRUD/select roundtrip + hostile
# tag/source_ref bind-safety over the real in-memory SQLite SQL layer). The main `test`
# check runs *default* features and clippy `--all-features` only compiles this code;
# this dedicated, feature-scoped check is what actually EXECUTES the sqlite path.
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
    cargoTestExtraArgs = "-p agent-prompt --features prompt-sqlite";
  }
)
