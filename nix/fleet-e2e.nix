# `nix run .#fleet-e2e` — live review-fleet end-to-end against REAL PRs.
#
# One `--serve-fleet` process hosts a two-row roster (two different GitHub repos),
# is `ReviewNow`n for each, grounds each review against THAT row's own repo + forge
# (multi-repo grounding, #289), and writes a redacted draft `.md` per PR. This is
# the "does it actually work" proof the hermetic in-process tests can't give — it
# needs a real model, a real GITHUB_TOKEN, and the network. Nothing is posted (the
# fleet stops at `drafted`).
#
# Not a hermetic check (needs a model + forge token + socket): opt-in only, and
# auto-included in `nix run .#integration`'s model tier when reachable. The harness
# logic lives in test/fleet-e2e/run.sh so it also runs from the dev shell.
#
# Env knobs (required + optional) are documented at the top of test/fleet-e2e/run.sh.
{
  pkgs,
  lib,
  versions,
  agent,
}:
pkgs.writeShellApplication {
  name = "fleet-e2e";
  runtimeInputs = [
    agent
    versions.grpcurl # ReviewNow + health over the wire
    pkgs.git # the per-row checkout/fetch the fleet drives
    pkgs.curl # generator-endpoint preflight
    pkgs.python3 # roster JSON + reply parsing + draft assertions
    pkgs.coreutils
    pkgs.findutils
  ];
  text = ''
    exec bash "${../test/fleet-e2e}/run.sh" "$@"
  '';
}
