# nix/checks/fleet-sqlite.nix
#
# Executes the `SqliteFleet` roster backend's tests, which sit behind the non-default
# `fleet-sqlite` cargo feature (review-fleet C2, docs/design/review-fleet/03-fleet-core.md).
# The main `test` check runs *default* features (so it exercises the memory + file
# backends but never the sqlite one), and clippy `--all-features` compiles + lints this
# code but does not run it. This dedicated, feature-scoped check is what actually
# EXECUTES the sqlite backend's tests in the gate — the exact pattern of
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
    cargoTestExtraArgs = "-p agent-review-fleet --features fleet-sqlite";
  }
)
