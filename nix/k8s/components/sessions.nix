# nix/k8s/components/sessions.nix — the `sessions` role (k8s track K3).
#
# Design: docs/design/k8s/01-deployment-targets.md, 04-manifests-and-gitops.md.
#
# `agent --serve-sessions`: the portal-driven sessions gateway (SessionRegistry +
# the driving AgentSessionService + the idle reaper) on :50080. Sync wave 4 — it
# comes up after the gateway (wave 3), with which it exchanges a `svc:` token. It
# terminates its own rustls mTLS from the cert-manager Secret; Cilium never sees
# the plaintext. Auth/JWKS wiring to the gateway lands with K5.
{
  lib,
  k8sLib,
  constants,
  target,
  imageTags,
}:
let
  s = constants.grpc.sessions;
  image = "agent-seddon/agent:${imageTags.agent}";
  ns = target.namespace;
  wave = 4;

  agentToml = ''
    # sessions role — rendered by nix/k8s/components/sessions.nix.
    # Do not edit by hand; run `nix run .#k8s-render-manifests`.

    ${k8sLib.roleBaseToml}
    [grpc.sessions]
    listen = "0.0.0.0:${toString s.port}"

    # rustls mTLS from the cert-manager Secret `tls-sessions` (kubernetes.io/tls),
    # mounted read-only at /etc/agent/tls. Cilium must never terminate this.
    [grpc.tls]
    cert = "/etc/agent/tls/tls.crt"
    key = "/etc/agent/tls/tls.key"
    client_ca = "/etc/agent/tls/ca.crt"

    [metrics]
    listen = "0.0.0.0:${toString s.metrics_port}"
  '';
in
{
  manifests = [
    {
      name = "sessions/configmap-sessions.yaml";
      content = k8sLib.toYAML (
        k8sLib.configMapFromToml {
          component = "sessions";
          namespace = ns;
          wave = 2;
          toml = agentToml;
        }
      );
    }
    {
      name = "sessions/deployment-sessions.yaml";
      content = k8sLib.toYAML (
        k8sLib.deployment {
          component = "sessions";
          namespace = ns;
          inherit image wave;
          args = [ "--serve-sessions" ];
          port = s.port;
          metricsPort = s.metrics_port;
          configMapName = "sessions-config";
          tlsSecretName = "tls-sessions";
        }
      );
    }
    {
      name = "sessions/service-sessions.yaml";
      content = k8sLib.toYAML (
        k8sLib.service {
          component = "sessions";
          namespace = ns;
          port = s.port;
          metricsPort = s.metrics_port;
          wave = 2;
        }
      );
    }
    {
      name = "apps/application-sessions.yaml";
      content = k8sLib.toYAML (
        k8sLib.application {
          component = "sessions";
          namespace = ns;
          inherit (target) repoURL revision;
          target = target.name;
        }
      );
    }
  ];
}
