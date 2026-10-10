# nix/k8s/components/fleet.nix — the `fleet` role (k8s track K3).
#
# Design: docs/design/k8s/01-deployment-targets.md, 04-manifests-and-gitops.md.
#
# `agent --serve-fleet`: the review fleet (roster control plane + orchestrator +
# reconcile) on :50086. Sync wave 4 — after the gateway (wave 3), with which it
# exchanges a `svc:` token. mTLS from the cert-manager Secret; Cilium never sees the
# plaintext. The fleet needs forge credentials (a Secret, K3's `k8s-secrets` slice)
# and a checkout workspace — for now the read-only rootfs's writable /tmp; a
# dedicated PVC/emptyDir workspace is a later refinement.
{
  lib,
  k8sLib,
  constants,
  target,
  imageTags,
}:
let
  f = constants.grpc.fleet;
  image = "agent-seddon/agent:${imageTags.agent}";
  ns = target.namespace;
  wave = 4;

  agentToml = ''
    # fleet role — rendered by nix/k8s/components/fleet.nix.
    # Do not edit by hand; run `nix run .#k8s-render-manifests`.

    ${k8sLib.roleBaseToml}
    [grpc.fleet]
    listen = "0.0.0.0:${toString f.port}"

    # rustls mTLS from the cert-manager Secret `tls-fleet` (kubernetes.io/tls),
    # mounted read-only at /etc/agent/tls. Cilium must never terminate this.
    [grpc.tls]
    cert = "/etc/agent/tls/tls.crt"
    key = "/etc/agent/tls/tls.key"
    client_ca = "/etc/agent/tls/ca.crt"

    [metrics]
    listen = "0.0.0.0:${toString f.metrics_port}"
  '';
in
{
  manifests = [
    {
      name = "fleet/configmap-fleet.yaml";
      content = k8sLib.toYAML (
        k8sLib.configMapFromToml {
          component = "fleet";
          namespace = ns;
          wave = 2;
          toml = agentToml;
        }
      );
    }
    {
      name = "fleet/deployment-fleet.yaml";
      content = k8sLib.toYAML (
        k8sLib.deployment {
          component = "fleet";
          namespace = ns;
          inherit image wave;
          args = [ "--serve-fleet" ];
          port = f.port;
          metricsPort = f.metrics_port;
          configMapName = "fleet-config";
          tlsSecretName = "tls-fleet";
        }
      );
    }
    {
      name = "fleet/service-fleet.yaml";
      content = k8sLib.toYAML (
        k8sLib.service {
          component = "fleet";
          namespace = ns;
          port = f.port;
          metricsPort = f.metrics_port;
          wave = 2;
        }
      );
    }
    {
      name = "apps/application-fleet.yaml";
      content = k8sLib.toYAML (
        k8sLib.application {
          component = "fleet";
          namespace = ns;
          inherit (target) repoURL revision;
          target = target.name;
        }
      );
    }
  ];
}
