# nix/checks/forge-registry-store.nix
#
# Executes the shared-store `ForgeRegistry` backend (`StoreForges`) tests plus the
# forge-kind builder matrix (`agent_forge::kind`), which sit behind the non-default
# `forge-store` cargo feature (config design C36, increment D1,
# docs/design/config/09-increments.md). The main `test` check runs *default*
# features (which do not build the store dep for this crate), so this dedicated,
# feature-scoped check EXECUTES the C36 matrix in the gate — the forge twin of
# registry-store: card validation + repo-encoding + SSRF screen + the CRUD store.
#
# It runs over `agent-config-store`'s in-memory backend (no DB dep). The postgres
# arm (feature `forge-store-postgres`) needs a live server and is exercised
# `#[ignore]`-gated by `nix run .#integration` (nix/pg-integration.nix).
{
  craneLib,
  commonArgs,
  cargoArtifacts,
}:

craneLib.cargoTest (
  commonArgs
  // {
    inherit cargoArtifacts;
    cargoTestExtraArgs = "-p agent-forge --features forge-store";
  }
)
