# nix/checks/forge-ghapp.nix
#
# Executes the GitHub-App **mint IDOR gate** and its install-store refresh decorator
# (forge-auth F2 + F4b), which sit behind the non-default `forge-ghapp` cargo feature.
# The main `test` check runs *default* features (forge-ghapp off, because it pulls
# `jsonwebtoken`+`ring` into agent-forge), so the cross-tenant-minting authorization
# decision — the security boundary a prompt-injectable tenant `token_ref` is checked
# against — was previously only *compiled* (clippy `--all-features`), never *run*.
#
# This dedicated, feature-scoped check runs it in the gate: the pure `ghapp_allowed`
# decision (operator-vouched bootstrap allowlist; the tenant-self-asserted store snapshot
# is deliberately NOT a source — that union is deferred to forge-auth F5, behind an
# ownership proof), the copy-on-write snapshot edits, and the `RefreshingInstalls`
# decorator (an F5 building block that keeps a sync store snapshot in step with the async
# per-tenant install store, wired but unread for now). The agent-forge arm (`StoreInstalls`
# + `github_install_bindings`) runs under forge-registry-store.nix; the minter's HTTP path
# (tiny_http) is exercised there too. Offline / hermetic.
{
  craneLib,
  commonArgs,
  cargoArtifacts,
}:

craneLib.cargoTest (
  commonArgs
  // {
    inherit cargoArtifacts;
    cargoTestExtraArgs = "-p agent-runtime --features forge-ghapp -- secrets:: install_gate::";
  }
)
