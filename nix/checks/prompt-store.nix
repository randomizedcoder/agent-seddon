# nix/checks/prompt-store.nix
#
# Executes the shared-store `PromptStore` backend (`StorePrompt`) tests, which sit
# behind the non-default `prompt-store` cargo feature (config design C41, increment
# A3c, docs/design/config/09-increments.md). The main `test` check runs *default*
# features (the file backend), and clippy `--all-features` compiles but does not run
# this code. This dedicated, feature-scoped check EXECUTES the shared-store backend's
# matrix in the gate — the prompt (outlier) twin of registry-store / fleet-store.
#
# It runs `StorePrompt` over `agent-config-store`'s in-memory backend (no DB dep),
# proving defaults/overrides, tag-derived `select`, and preview agree with the file
# backend. The postgres arm (feature `prompt-store-postgres`) needs a live server
# and is exercised `#[ignore]`-gated by `nix run .#integration` (nix/pg-integration.nix).
{
  craneLib,
  commonArgs,
  cargoArtifacts,
}:

craneLib.cargoTest (
  commonArgs
  // {
    inherit cargoArtifacts;
    cargoTestExtraArgs = "-p agent-prompt --features prompt-store";
  }
)
