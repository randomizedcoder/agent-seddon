# nix/k8s/components/gateway.nix — the `gateway` role (k8s track K3).
#
# Design: docs/design/k8s/04-manifests-and-gitops.md, 08-sandbox.md.
#
# The gateway is `agent --serve-all`: every enabled seam behind one port (:50100).
# On the cluster it is a network Service, so its rendered `agent.toml` sets
# `[grpc.gateway] exclude = ["sandbox", "pty", "forge"]` — the exec seams never get
# a listener here (the Rust half of that guard shipped in #580). It terminates its
# own rustls mTLS from the cert-manager Secret; Cilium never sees the plaintext.
#
# Sync wave 3: the gateway issues tokens and serves JWKS, so it comes up before the
# sessions/fleet roles (wave 4) that exchange a `svc:` token with it.
{
  lib,
  k8sLib,
  constants,
  target,
  imageTags,
}:
let
  gw = constants.grpc.gateway;
  image = "agent-seddon/agent:${imageTags.agent}";
  ns = target.namespace;
  wave = 3;

  # The gateway's rendered config. Only the cluster-relevant blocks: the exec-seam
  # exclusion, the mTLS material mounted from the cert-manager Secret, and the
  # metrics bind. Fuller wiring (providers, stores, OTLP) lands with K8.
  agentToml = ''
    # gateway role — rendered by nix/k8s/components/gateway.nix.
    # Do not edit by hand; run `nix run .#k8s-render-manifests`.

    # `--serve-all` hosts every enabled seam on one port. On a cluster Service the
    # exec seams must not get a listener (docs/design/k8s/08); an unknown name here
    # is a config-load error (crates/agent-cli, #580).
    [grpc.gateway]
    listen = "0.0.0.0:${toString gw.port}"
    exclude = ["sandbox", "pty", "forge"]

    # rustls mTLS from the cert-manager Secret `tls-gateway` (kubernetes.io/tls),
    # mounted read-only at /etc/agent/tls. Cilium must never terminate this.
    [grpc.tls]
    cert = "/etc/agent/tls/tls.crt"
    key = "/etc/agent/tls/tls.key"
    client_ca = "/etc/agent/tls/ca.crt"

    [metrics]
    listen = "0.0.0.0:${toString gw.metrics_port}"
  '';
in
{
  manifests = [
    {
      name = "gateway/configmap-gateway.yaml";
      content = k8sLib.toYAML (
        k8sLib.configMapFromToml {
          component = "gateway";
          namespace = ns;
          wave = 2;
          toml = agentToml;
        }
      );
    }
    {
      name = "gateway/deployment-gateway.yaml";
      content = k8sLib.toYAML (
        k8sLib.deployment {
          component = "gateway";
          namespace = ns;
          inherit image wave;
          args = [ "--serve-all" ];
          port = gw.port;
          metricsPort = gw.metrics_port;
          configMapName = "gateway-config";
          tlsSecretName = "tls-gateway";
        }
      );
    }
    {
      name = "gateway/service-gateway.yaml";
      content = k8sLib.toYAML (
        k8sLib.service {
          component = "gateway";
          namespace = ns;
          port = gw.port;
          metricsPort = gw.metrics_port;
          wave = 2;
        }
      );
    }
    {
      name = "apps/application-gateway.yaml";
      content = k8sLib.toYAML (
        k8sLib.application {
          component = "gateway";
          namespace = ns;
          inherit (target) repoURL revision;
          target = target.name;
        }
      );
    }
  ];
}
