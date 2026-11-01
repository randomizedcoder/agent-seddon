# RK-03: execute E1 with extraction enabled; includes pinned fixture hash.
{
  craneLib,
  commonArgs,
  workspaceSlice,
}:
craneLib.cargoTest (
  commonArgs
  // {
    inherit (workspaceSlice.sliceArgs [ "agent-repo-graph" ]) src cargoArtifacts;
    cargoTestExtraArgs = "-p agent-repo-graph --features extract-rust";
  }
)
