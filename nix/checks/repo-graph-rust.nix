# RK-03: execute E1 with extraction enabled; includes pinned fixture hash.
{
  craneLib,
  commonArgs,
  cargoArtifacts,
}:
craneLib.cargoTest (
  commonArgs
  // {
    inherit cargoArtifacts;
    cargoTestExtraArgs = "-p agent-repo-graph --features extract-rust";
  }
)
