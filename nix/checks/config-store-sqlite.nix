# nix/checks/config-store-sqlite.nix
#
# Executes the `SqliteBackend` config-store tests, which sit behind the non-default
# `config-store-sqlite` cargo feature (config design C41, increment A1,
# docs/design/config/09-increments.md). The main `test` check runs *default*
# features (so it exercises the memory + file backends but never the sqlite one),
# and clippy `--all-features` compiles + lints this code but does not run it. This
# dedicated, feature-scoped check is what actually EXECUTES the sqlite backend's
# trait matrix in the gate — the exact pattern of `fleet-sqlite.nix` /
# `prompt-sqlite.nix`.
#
# `rusqlite`'s `bundled` feature compiles vendored `sqlite3.c` with the stdenv C
# toolchain crane already provides — no system libsqlite3, no extra build input.
{
  craneLib,
  commonArgs,
  cargoArtifacts,
}:

craneLib.cargoTest (
  commonArgs
  // {
    inherit cargoArtifacts;
    cargoTestExtraArgs = "-p agent-config-store --features config-store-sqlite";
  }
)
