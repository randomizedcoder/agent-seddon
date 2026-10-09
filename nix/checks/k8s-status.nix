# nix/checks/k8s-status.nix — the `agent-k8s-status` crate's own test suite.
#
# Design: docs/design/k8s/04-manifests-and-gitops.md (the `nix run .#k8s-status` step).
#
# Gates the CORRECTNESS of the `nix run .#k8s-status` health tool's pure core: parsing a
# `kubectl get … -o json` response, grading each ArgoCD Application (Synced + Healthy)
# and role Deployment (ready + Available) and rolling them into one green/red verdict.
# Four case classes + the mandatory `adversarial_` rows — kubectl's JSON crosses a
# process boundary, so hostile/missing/wrong-typed fields must fail closed, never a
# spurious green and never a panic. The `test` check runs these too under default
# features; this names the K3 slice-8 gate, the `k8s-secrets` / `k8s-render-tests` twin.
{
  craneLib,
  commonArgs,
  cargoArtifacts,
}:
craneLib.cargoTest (
  commonArgs
  // {
    inherit cargoArtifacts;
    cargoTestExtraArgs = "-p agent-k8s-status";
  }
)
