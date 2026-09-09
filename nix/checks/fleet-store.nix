# nix/checks/fleet-store.nix
#
# Executes the shared-store `FleetRegistry` backend (`StoreFleet`) tests, which
# sit behind the non-default `fleet-store` cargo feature (config design C41,
# increment A3b, docs/design/config/09-increments.md). The main `test` check runs
# *default* features (the legacy Memory/File backends), and clippy `--all-features`
# compiles but does not run this code. This dedicated, feature-scoped check
# EXECUTES the shared-store backend's matrix in the gate — the fleet twin of
# registry-store.
#
# It runs `StoreFleet` over `agent-config-store`'s in-memory backend (no DB dep),
# proving the roster mapping + `ops` semantics agree with `MemoryFleet`. The
# postgres arm (feature `fleet-store-postgres`) needs a live server and is
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
    cargoTestExtraArgs = "-p agent-review-fleet --features fleet-store";
  }
)
