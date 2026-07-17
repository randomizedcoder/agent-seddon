# nix/checks/clippy.nix
#
# `cargo clippy --all-targets -- -D warnings` — fails on any lint.
#
{
  craneLib,
  commonArgs,
  cargoArtifacts,
}:

craneLib.cargoClippy (
  commonArgs
  // {
    inherit cargoArtifacts;
    cargoClippyExtraArgs = "--all-targets --all-features -- -D warnings";
  }
)
