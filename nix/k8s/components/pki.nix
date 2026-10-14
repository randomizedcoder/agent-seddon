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
# map those SANs to tenants and roles are rendered from the same `k8sLib.roleSan`
# (K5), and the shared `token-signer` signing key is issued here too.
# Short 24h lifetimes keep the K4 reload path exercised every day.
#
# Security note (SAN forgery): a cert-manager CA issuer signs whatever SAN a
# `Certificate` asks for, so the right to create a `Certificate` that references
# `agent-seddon-ca` is the right to mint any role identity. The control today is
# cluster RBAC — only the operator and ArgoCD may create `Certificate` resources;
# the agent pods cannot. Constraining *which* SANs a requester may obtain needs
# cert-manager approver-policy (`CertificateRequestPolicy`), tracked as a K5 /
# hardening follow-up in 05-identity-and-pki.md and 10-increments.md.
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

  inherit (k8sLib) roles;

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
      uris = [ (k8sLib.roleSan target.deployment role) ];
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

  # The agent-token signing key (K5), shared by every role: each one signs the
  # tokens it mints and verifies the others' with it. cert-manager generates the
  # key in the cluster, so it never touches a disk outside it; the certificate
  # around it is only a carrier and is never mounted.
  #   - `rotationPolicy: Never` keeps the key across renewals, so a renewal never
  #     invalidates live tokens. Rotating the key is a deliberate operator step
  #     (`[auth.token] previous_key`, docs/design/k8s/05).
  #   - `digital signature` only, no `server auth`/`client auth`: it can never act
  #     as an mTLS identity, and no `[auth.mtls]` binding names its SAN.
  tokenSignerCertificate = {
    apiVersion = "cert-manager.io/v1";
    kind = "Certificate";
    metadata = {
      name = k8sLib.tokenSignerSecret;
      namespace = ns;
      labels = labels k8sLib.tokenSignerSecret;
      annotations = syncWave wave;
    };
    spec = {
      secretName = k8sLib.tokenSignerSecret;
      issuerRef = {
        kind = "ClusterIssuer";
        name = caName;
        group = "cert-manager.io";
      };
      privateKey = {
        algorithm = "ECDSA";
        size = 256;
        rotationPolicy = "Never";
      };
      duration = "2160h"; # 90 days; renewing keeps the key
      renewBefore = "360h";
      uris = [ "spiffe://${trustDomain}/svc/${k8sLib.tokenSignerSecret}" ];
      usages = [ "digital signature" ];
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
        # The chain is root → leaf only: this CA issues end-entity role certs and
        # must never sign a sub-CA. cert-manager's Certificate has no field for a
        # path-length constraint (no `maxPathLen`/basicConstraints on the spec), so
        # that is not pinned into the cert itself. It holds structurally instead:
        # the root is the only `isCA` cert, every role cert is a leaf (isCA unset),
        # and who may request an `isCA` cert is gated by RBAC today and by
        # approver-policy (CertificateRequestPolicy) in K5. The k8s-render-tests
        # gate asserts the leaf-only shape; k8s-kubeconform would reject a stray
        # `maxPathLen` as an unknown CRD field.
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
    (manifest "pki/certificate-${k8sLib.tokenSignerSecret}.yaml" tokenSignerCertificate)
    (manifest "apps/application-pki.yaml" (application {
      component = "pki";
      namespace = ns;
      inherit (target) repoURL revision;
      target = target.name;
    }))
  ];
}
