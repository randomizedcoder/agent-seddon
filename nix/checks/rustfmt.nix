# nix/checks/rustfmt.nix
#
# `cargo fmt --check` — fails on any unformatted Rust file.
#
{
  craneLib,
  commonArgs,
}:

craneLib.cargoFmt { inherit (commonArgs) src; }
