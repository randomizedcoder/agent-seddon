# nix/test-features.nix
#
# Renders `nix/test-features.json`: for each workspace crate, the cargo features it
# has in a whole-workspace build (`cargo metadata`'s resolve). Each `test-<crate>`
# check passes that crate's list, so testing one crate on its own slice runs the same
# feature-gated tests the old whole-workspace `cargo test` ran: without it, a crate
# whose features another crate turns on (agent-runtime's `auth`, agent-tools' tools,
# the store backends, …) would quietly skip those tests.
#
# One derivation, shared by the `gen-test-features` app (writes the committed file)
# and the `test-features-sync` check (fails when it is stale). It reads manifests and
# the lockfile only (crane's dummy source), so it re-runs only when those change.
{
  pkgs,
  craneLib,
  commonArgs,
  workspaceSlice,
}:

craneLib.mkCargoDerivation (
  commonArgs
  // {
    pname = "test-features";
    src = workspaceSlice.dummy;
    cargoArtifacts = null;
    doInstallCargoArtifacts = false;
    nativeBuildInputs = (commonArgs.nativeBuildInputs or [ ]) ++ [ pkgs.jq ];
    buildPhaseCargoCommand = ''
      cargo metadata --format-version 1 --offline --locked \
        | jq -S '
            . as $m
            | [ $m.resolve.nodes[]
                | select(.id as $id | $m.workspace_members | index($id))
                | . as $n
                | { key: ($m.packages[] | select(.id == $n.id) | .name),
                    value: ($n.features - ["default"]) } ]
            | from_entries' > test-features.json
    '';
    installPhaseCommand = "cp test-features.json $out";
  }
)
