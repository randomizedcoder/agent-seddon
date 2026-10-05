# nix/k8s/components/pki.nix — the in-cluster PKI (k8s track K3).
#
# Design: docs/design/k8s/05-identity-and-pki.md ("CA chain", "Per-role Certificates").
#
# The agent terminates its own mTLS, so all it needs is PEM on disk; cert-manager is
# the "bring your own CA". This component renders, at ArgoCD sync-wave 0 (before any
# workload), the whole chain:
#
#   ClusterIssuer selfsigned-bootstrap            (selfSigned)
#     └─ Certificate agent-seddon-ca              (isCA, ECDSA P-256, 10y; the root
#          │                                        key is generated in-cluster and
#          │                                        never leaves it — Secret lives in
#          │                                        the cert-manager namespace so the
#          │                                        CA ClusterIssuer can read it)
#          └─ ClusterIssuer agent-seddon-ca       (ca.secretName)
#               └─ Certificate per role           (Secret tls-<role>, the deployments
#                                                   already mount it at /etc/agent/tls)
#
# Each role Certificate carries the SPIFFE URI SAN the agent enforces as the peer
# principal (`spiffe://<trust-domain>/svc/<role>`); the `[auth.mtls] bindings` that
# map those SANs to tenants and roles are rendered into the role configs at K5.
# Short 24h lifetimes keep the K4 reload path exercised every day.
{
  lib,
  k8sLib,
  constants,
  target,
  imageTags,
}:
let
  inherit (k8sLib)
    labels
    syncWave
    toYAML
    application
    ;

  ns = target.namespace; # agent-seddon — where the role certs (and their Secrets) live
  caName = "agent-seddon-ca";
  # A ClusterIssuer resolves its `ca.secretName` in cert-manager's cluster-resource
  # namespace, so the root Secret must live there, not in the workload namespace.
  caNamespace = "cert-manager";
  # The SPIFFE trust domain for this deployment: `agent.l2` on k3s (01/05).
  trustDomain = "agent.${target.deployment}";
  # The CA chain and every role cert apply before the workloads (which are wave 3+).
  wave = 0;

  # The three agent roles that terminate mTLS (gateway, sessions, fleet). The exec
  # seams never run as a cluster Service, so they get no certificate.
  roles = [
    "gateway"
    "sessions"
    "fleet"
  ];

  clusterIssuer =
    { name, spec }:
    {
      apiVersion = "cert-manager.io/v1";
      kind = "ClusterIssuer";
      metadata = {
        inherit name;
        labels = labels name;
        annotations = syncWave wave;
      };
      inherit spec;
    };

  # One role's Certificate: ECDSA P-256 rotated on every renewal, a 24h lifetime
  # renewed 8h out, the SPIFFE URI SAN plus the in-cluster DNS names, issued by the
  # CA ClusterIssuer into the `tls-<role>` Secret the deployment mounts.
  roleCertificate = role: {
    apiVersion = "cert-manager.io/v1";
    kind = "Certificate";
    metadata = {
      name = role;
      namespace = ns;
      labels = labels role;
      annotations = syncWave wave;
    };
    spec = {
      secretName = "tls-${role}";
      issuerRef = {
        kind = "ClusterIssuer";
        name = caName;
        group = "cert-manager.io";
      };
      privateKey = {
        algorithm = "ECDSA";
        size = 256;
        rotationPolicy = "Always";
      };
      duration = "24h";
      renewBefore = "8h";
      uris = [ "spiffe://${trustDomain}/svc/${role}" ];
      dnsNames = [
        role
        "${role}.${ns}"
        "${role}.${ns}.svc"
      ];
      usages = [
        "server auth"
        "client auth"
        "digital signature"
      ];
    };
  };

  manifest = name: obj: {
    inherit name;
    content = toYAML obj;
  };
in
{
  manifests = [
    (manifest "pki/clusterissuer-selfsigned-bootstrap.yaml" (clusterIssuer {
      name = "selfsigned-bootstrap";
      spec.selfSigned = { };
    }))
    # The root CA: self-signed by the bootstrap issuer, 10 years, isCA. Its key is
    # generated in the cluster and never leaves it.
    (manifest "pki/certificate-${caName}.yaml" {
      apiVersion = "cert-manager.io/v1";
      kind = "Certificate";
      metadata = {
        name = caName;
        namespace = caNamespace;
        labels = labels caName;
        annotations = syncWave wave;
      };
      spec = {
        isCA = true;
        commonName = caName;
        secretName = caName;
        privateKey = {
          algorithm = "ECDSA";
          size = 256;
        };
        duration = "87600h"; # 10 years
        issuerRef = {
          kind = "ClusterIssuer";
          name = "selfsigned-bootstrap";
          group = "cert-manager.io";
        };
      };
    })
    (manifest "pki/clusterissuer-${caName}.yaml" (clusterIssuer {
      name = caName;
      spec.ca.secretName = caName;
    }))
  ]
  ++ map (role: manifest "pki/certificate-${role}.yaml" (roleCertificate role)) roles
  ++ [
    (manifest "apps/application-pki.yaml" (application {
      component = "pki";
      namespace = ns;
      inherit (target) repoURL revision;
      target = target.name;
    }))
  ];
}
