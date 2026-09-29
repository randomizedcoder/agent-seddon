# nix/checks/campaign-e2e.nix
#
# The campaign track's first-autonomous-PR path, end to end (CP-06b,
# docs/design/campaigns/05-increments.md). Runs exactly
# `crates/agent-runtime/tests/campaign_e2e.rs`: the shipped driver, planner and
# forge poller, the in-process worker (worktree → Implement session → checkpoint →
# push → create_pr), a real `git` checkout with a bare origin on disk, a scripted
# model and a trait-level fake forge; MemCampaigns as the store (no Postgres).
#
# Redundant with the workspace `test` check by design: the gate names the
# increment, so a regression in the campaign path fails a check called
# `campaign-e2e` rather than hiding in the workspace run. Hermetic: the push goes
# to a bare repo under the test's tempdir, there is no network and no `$HOME` git
# config (the fixture sets its identity on the checkout).
{
  pkgs,
  craneLib,
  commonArgs,
  cargoArtifacts,
}:

craneLib.cargoTest (
  commonArgs
  // {
    inherit cargoArtifacts;
    cargoTestExtraArgs = "-p agent-runtime --test campaign_e2e";
    # The hermetic check sandbox has no host PATH: the worker's checkpoint and push
    # shell out to `git` through `agent-git`'s CLI backend.
    nativeBuildInputs = (commonArgs.nativeBuildInputs or [ ]) ++ [ pkgs.git ];
  }
)
