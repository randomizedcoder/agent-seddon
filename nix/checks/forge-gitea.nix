# nix/checks/forge-gitea.nix
#
# Executes the Gitea `Forge` backend's tests + the gitea arm of the forge-kind
# builder (`agent_forge::kind`), which sit behind the non-default `forge-gitea`
# cargo feature (config design C36, increment D1b, docs/design/config/04-forge-registry.md).
# The main `test` check runs *default* features (github+gitlab only), so this
# dedicated, feature-scoped check EXECUTES the gitea mapper matrix + the
# card→forge build in the gate — the D1b twin of forge-registry-store.
#
# Gitea is opt-in (self-hosted, GitHub-shaped `/api/v1` with its own auth/merge/
# paging dialect); enabling the feature also keeps the default github+gitlab on,
# so `known_kinds()` asserts all three. Hermetic: the tests are pure JSON mapping +
# the no-token early-error path (no live endpoint).
{
  craneLib,
  commonArgs,
  cargoArtifacts,
}:

craneLib.cargoTest (
  commonArgs
  // {
    inherit cargoArtifacts;
    cargoTestExtraArgs = "-p agent-forge --features forge-gitea";
  }
)
