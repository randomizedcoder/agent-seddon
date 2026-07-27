# nix/checks/prompt-sqlite.nix
#
# Executes the `SqlitePromptStore` backend's tests, which sit behind the non-default
# `prompt-sqlite` cargo feature — the workspace's only database dependency
# (docs/design/prompts/05-storage.md). The main `test` check runs *default* features
# (it must not enable feature-gated allocators like `dhat-heap`), and clippy
# `--all-features` compiles + lints this code but does not run it. This dedicated,
# feature-scoped check is what actually EXECUTES the sqlite backend's tests in the gate.
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
    cargoTestExtraArgs = "-p agent-prompt --features prompt-sqlite";
  }
)
