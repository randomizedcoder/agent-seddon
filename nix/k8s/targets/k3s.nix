# nix/k8s/targets/k3s.nix — the k3s-on-l2 target (k8s track K3).
#
# Design: docs/design/k8s/01-deployment-targets.md, 02-cluster-platform.md.
#
# The short-term single-node target: k3s on the l2 desktop. One replica per role
# (no HA overlay — that is the full-k8s target, K9). The GitOps root Application on
# l2 watches this repo's `main` at `rendered/k3s/apps`, so `repoURL`/`revision`
# below must match what `~/nixos/desktop/l2/k3s.nix` installs.
{
  name = "k3s";

  # The namespace every agent workload lands in.
  namespace = "agent-seddon";

  # The SPIFFE trust-domain suffix for this deployment: identities are
  # `spiffe://agent.l2/svc/<role>` (per-role Certificates, K5). l2 is the node.
  deployment = "l2";

  # The GitOps source ArgoCD syncs from (matches the l2 root Application).
  repoURL = "https://github.com/randomizedcoder/agent-seddon";
  revision = "main";

  # Single node: one replica per role.
  replicas = 1;
}
