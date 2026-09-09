# nix/checks/registry-store.nix
#
# Executes the shared-store `ProviderRegistry` backend (`StoreRegistry`) tests,
# which sit behind the non-default `registry-store` cargo feature (config design
# C41, increment A3, docs/design/config/09-increments.md). The main `test` check
# runs *default* features (the legacy Memory/File backends), and clippy
# `--all-features` compiles but does not run this code. This dedicated,
# feature-scoped check EXECUTES the shared-store backend's matrix in the gate —
# the registry convergence twin of config-store-sqlite.
#
# It runs `StoreRegistry` over `agent-config-store`'s in-memory backend (no DB
# dep), proving the card mapping + `ops` semantics agree with `MemoryRegistry`.
# The postgres arm (feature `registry-store-postgres`) needs a live server and is
# exercised `#[ignore]`-gated by `nix run .#integration` (nix/pg-integration.nix).
{
  craneLib,
  commonArgs,
  cargoArtifacts,
}:

craneLib.cargoTest (
  commonArgs
  // {
    inherit cargoArtifacts;
    cargoTestExtraArgs = "-p agent-registry --features registry-store";
  }
)
