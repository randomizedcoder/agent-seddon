# nix/checks/cargo-audit.nix
#
# `cargo audit` against the pinned RustSec advisory-db input. Hermetic — uses
# the flake's `advisory-db` source rather than fetching at eval time.
#
{
  craneLib,
  commonArgs,
  advisory-db,
}:

craneLib.cargoAudit {
  inherit (commonArgs) src;
  inherit advisory-db;
}
