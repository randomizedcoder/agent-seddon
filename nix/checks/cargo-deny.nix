# nix/checks/cargo-deny.nix
#
# `cargo deny check licenses bans sources` — supply-chain / licensing static
# analysis over the whole dependency graph (config: `deny.toml` at the repo root):
#   - licenses: only the permissive allow-list is accepted (copyleft fails)
#   - bans:     duplicate versions + wildcard reqs (advisory `warn` today)
#   - sources:  only crates.io + the pinned tantivy git URL are allowed
#
# OFFLINE by design — these three checks need no network, so it runs hermetically
# under `nix flake check` (crane auto-vendors the crate sources from Cargo.lock).
# RustSec **advisories** are intentionally NOT run here: cargo-audit
# (nix/checks/cargo-audit.nix) already gates them against the pinned advisory-db, so
# running them here too would be redundant and would need network.
#
{
  craneLib,
  commonArgs,
}:

craneLib.cargoDeny (
  commonArgs
  // {
    cargoDenyChecks = "licenses bans sources";
  }
)
