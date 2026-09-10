# nix/checks/forge-bitbucket.nix
#
# Executes the Bitbucket Cloud `Forge` backend's tests + the bitbucket arm of the
# forge-kind builder (`agent_forge::kind`), which sit behind the non-default
# `forge-bitbucket` cargo feature (config design C36, increment D1b,
# docs/design/config/04-forge-registry.md). The main `test` check runs *default*
# features (github+gitlab only), so this dedicated, feature-scoped check EXECUTES
# the bitbucket mapper matrix (envelope pagination, no-review-object, nested
# `content.raw`/`links.html.href`, upper-case state) + the card→forge build + the
# loopback e2e (Bearer auth + nested mapping) in the gate — the divergent-API twin
# of forge-gitea.
#
# Bitbucket is opt-in; enabling the feature also keeps the default github+gitlab
# on, so `known_kinds()` asserts all three built kinds. Hermetic: pure JSON mapping
# + the no-token early-error path (no live endpoint).
{
  craneLib,
  commonArgs,
  cargoArtifacts,
}:

craneLib.cargoTest (
  commonArgs
  // {
    inherit cargoArtifacts;
    cargoTestExtraArgs = "-p agent-forge --features forge-bitbucket";
  }
)
