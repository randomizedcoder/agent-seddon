# nix/checks/role-store.nix
#
# Executes the shared-store `RoleRegistry` backend (`StoreRoles`) tests, which sit
# behind the non-default `role-store` cargo feature (config design C34, increment
# C1b, docs/design/config/09-increments.md). The main `test` check runs *default*
# features (which do not build the store dep for this crate), and clippy
# `--all-features` compiles but does not run this code. This dedicated,
# feature-scoped check EXECUTES the shared-store backend's matrix in the gate —
# the RBAC-role-card twin of registry-store.
#
# It runs `StoreRoles` over `agent-config-store`'s in-memory backend (no DB dep),
# proving the prost card mapping + `RoleRegistry` semantics (reserved-id/bad-action
# rejection, catalog folding) hold. The postgres arm (feature
# `role-store-postgres`) needs a live server and is exercised `#[ignore]`-gated by
# `nix run .#integration` (nix/pg-integration.nix).
{
  craneLib,
  commonArgs,
  cargoArtifacts,
}:

craneLib.cargoTest (
  commonArgs
  // {
    inherit cargoArtifacts;
    cargoTestExtraArgs = "-p agent-role --features role-store";
  }
)
