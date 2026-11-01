# nix/checks/clippy.nix
#
# `cargo clippy -p <crate> --all-targets --all-features -- -D warnings` for one
# workspace crate, on that crate's source slice (nix/workspace-slice.nix): the check
# re-runs only when the crate or one of its workspace dependencies changes. Each crate
# is linted under all of its own features, as the old workspace-wide `--all-features`
# run did. `nix/checks/default.nix` makes one `clippy-<crate>` per member, and
# `clippy` aggregates them.
#
{
  craneLib,
  commonArgs,
  workspaceSlice,
  crate,
}:

craneLib.cargoClippy (
  commonArgs
  // workspaceSlice.clippySliceArgs [ crate ]
  // {
    pname = "clippy-${crate}";
    cargoClippyExtraArgs = "-p ${crate} --all-targets --all-features -- -D warnings";
  }
)
