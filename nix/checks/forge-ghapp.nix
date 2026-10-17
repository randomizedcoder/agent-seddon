# nix/checks/forge-ghapp.nix
#
# Executes the GitHub-App **mint IDOR gate** and its install-store refresh decorator
# (forge-auth F2 + F4b), which sit behind the non-default `forge-ghapp` cargo feature.
# The main `test` check runs *default* features (forge-ghapp off, because it pulls
# `jsonwebtoken`+`ring` into agent-forge), so the cross-tenant-minting authorization
# decision — the security boundary a prompt-injectable tenant `token_ref` is checked
# against — was previously only *compiled* (clippy `--all-features`), never *run*.
#
# This dedicated, feature-scoped check runs it in the gate. It spans BOTH crates that
# live behind `forge-ghapp`, because `forge-store` does NOT enable `forge-ghapp`, so no
# other check builds this surface with the feature on:
#
#   * agent-runtime — the pure `ghapp_allowed` mint decision (operator-vouched bootstrap
#     allowlist; the tenant-self-asserted store snapshot is deliberately NOT a source —
#     that union is deferred to forge-auth F5, behind an ownership proof), the
#     copy-on-write snapshot edits, and the `RefreshingInstalls` decorator (an F5 building
#     block, wired but unread for now).
#   * agent-forge — the `ghapp` installation-token minter (F2) and the `ghapp_oauth`
#     install-time user-OAuth ownership verifier (F5a), both over a `tiny_http` loopback,
#     plus the `webhook` HMAC verify (F3). Without this arm those authorization boundaries
#     were only *compiled* (clippy `--all-features`), never *run*.
#
# One hermetic/offline invocation, filtered to those modules across both packages
# (libtest treats several positional filters as OR). The agent-forge store arm
# (`StoreInstalls` + `github_install_bindings`, feature `forge-store`) runs under
# forge-registry-store.nix.
{
  craneLib,
  commonArgs,
  cargoArtifacts,
}:

craneLib.cargoTest (
  commonArgs
  // {
    inherit cargoArtifacts;
    cargoTestExtraArgs = "-p agent-runtime -p agent-forge --features forge-ghapp -- secrets:: install_gate:: ghapp webhook";
  }
)
