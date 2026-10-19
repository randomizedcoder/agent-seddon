# nix/checks/forge-ghapp.nix
#
# Executes the GitHub-App **mint IDOR gate** and the ownership-verified install
# onboarding that feeds it (forge-auth F2 + F5c2), which sit behind the non-default
# `forge-ghapp` cargo feature.
# The main `test` check runs *default* features (forge-ghapp off, because it pulls
# `jsonwebtoken`+`ring` into agent-forge), so the cross-tenant-minting authorization
# decision — the security boundary a prompt-injectable tenant `token_ref` is checked
# against — was previously only *compiled* (clippy `--all-features`), never *run*.
#
# This dedicated, feature-scoped check runs it in the gate. It spans every crate with
# code behind `forge-ghapp`, because `forge-store` does NOT enable `forge-ghapp`, so no
# other check builds this surface with the feature on:
#
#   * agent-runtime — the pure `ghapp_allowed` mint decision (operator bootstrap allowlist
#     OR the ownership-verified snapshot; the tenant-self-asserted CRUD store is never a
#     source), the copy-on-write snapshot edits, and the `install_gate` onboarding backend
#     (state binding, admin proof, one-tenant-per-install) that is the snapshot's only writer.
#   * agent-forge — the `ghapp` installation-token minter (F2) and the `ghapp_oauth`
#     install-time user-OAuth ownership verifier (F5a), both over a `tiny_http` loopback,
#     plus the `webhook` HMAC verify (F3). Without this arm those authorization boundaries
#     were only *compiled* (clippy `--all-features`), never *run*.
#   * agent-runtime `install_lifecycle` (F6) — the webhook-driven narrowing of that gate
#     (uninstall/suspend unbind + forget the cached token, unsuspend rebinds only a still-
#     held claim, repo-list sync never touches the gate).
#   * agent-cli `fleet_webhook` (F6) — the `POST /webhooks/github` receiver: HMAC verified
#     before any parse, body cap, delivery replay set, over a real loopback socket.
#
# One hermetic/offline invocation, filtered to those modules across the three packages
# (libtest treats several positional filters as OR). The agent-forge store arm
# (`StoreInstalls` + `VerifiedInstalls` + `verified_install_bindings`, feature `forge-store`) runs under
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
    cargoTestExtraArgs = "-p agent-runtime -p agent-forge -p agent-cli --features forge-ghapp -- secrets:: install_gate:: install_lifecycle:: ghapp webhook";
  }
)
