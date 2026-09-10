# nix/checks/transport-registry-store.nix
#
# Executes the shared-store `TransportRegistry` backend (`StoreTransports`) tests plus
# the transport-kind builder matrix (`agent_slack::kind`), which sit behind the
# non-default `transport-store` cargo feature (config design C37, increment D2,
# docs/design/config/09-increments.md). The main `test` check runs *default* features
# (which do not build the store dep for this crate), so this dedicated, feature-scoped
# check EXECUTES the C37 matrix in the gate — the transport twin of
# forge-registry-store: card validation + channel purpose + endpoint SSRF screen +
# the CRUD store + the rate-limit / soft-fail primitives.
#
# It runs over `agent-config-store`'s in-memory backend (no DB dep). The postgres arm
# (feature `transport-store-postgres`) needs a live server and is exercised
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
    cargoTestExtraArgs = "-p agent-slack --features transport-store";
  }
)
