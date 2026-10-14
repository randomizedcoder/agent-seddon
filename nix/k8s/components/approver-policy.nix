# nix/k8s/components/approver-policy.nix — bound SAN forgery at the issuer (k8s track K5).
#
# Design: docs/design/k8s/05-identity-and-pki.md ("Who may mint an identity").
#
# A cert-manager CA ClusterIssuer signs whatever a `Certificate` asks for, so the
# right to create a `Certificate` referencing `agent-seddon-ca` is the right to mint
# *any* role identity. RBAC already limits *who* may create a Certificate (only the
# operator and ArgoCD); approver-policy limits *what* they may ask for.
#
# cert-manager's own CertificateRequest is created by the cert-manager controller
# (requester `system:serviceaccount:cert-manager:cert-manager`), NOT by the role's
# ServiceAccount — so a policy is keyed by the request's *namespace* and *issuer*,
# never by the workload identity. approver-policy only consults a policy the
# requesting identity holds the `use` verb on (the ClusterRole/Binding below), and
# APPROVES a request only when at least one such policy allows every attribute;
# otherwise — with cert-manager's built-in approver disabled on the platform
# (`--controllers=*,-certificaterequests-approver`, ~/nixos/desktop/l2) — it is
# DENIED. So an intruder Certificate asking for an unknown SAN is refused.
#
# One narrow CertificateRequestPolicy per allowed identity (so no policy can be
# reused to mint a different role), each a precise superset of exactly what the live
# CertificateRequest carries (grounded against the running cluster):
#   - the three role leaves: their SPIFFE URI SAN + DNS names, isCA:false, 24h;
#   - the shared token-signer leaf: its URI SAN only, no DNS, `digital signature`
#     only, isCA:false, 2160h;
#   - the root CA: isCA:true, CN agent-seddon-ca, issued by selfsigned-bootstrap in
#     the cert-manager namespace, no usages (cert-manager sets none on the CA CR),
#     87600h.
# Every policy pins ECDSA P-256 and the issuing ClusterIssuer + requesting namespace.
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
    roles
    roleSan
    tokenSignerSecret
    ;

  ns = target.namespace; # agent-seddon — where the role + token-signer certs are requested
  caName = "agent-seddon-ca";
  caNamespace = "cert-manager"; # where the root CA Certificate is requested
  trustDomain = "agent.${target.deployment}";
  bootstrapIssuer = "selfsigned-bootstrap";
  wave = 0; # alongside the PKI it guards (components/pki.nix)

  # Only ever P-256 ECDSA, matching every Certificate (components/pki.nix).
  ecdsaP256 = {
    algorithm = "ECDSA";
    minSize = 256;
    maxSize = 256;
  };

  # The CA ClusterIssuer and the agent-seddon namespace: the leaf requests.
  leafSelector = {
    issuerRef = {
      name = caName;
      kind = "ClusterIssuer";
      group = "cert-manager.io";
    };
    namespace.matchNames = [ ns ];
  };

  requestPolicy =
    {
      name,
      allowed,
      constraints,
      selector,
    }:
    {
      apiVersion = "policy.cert-manager.io/v1alpha1";
      kind = "CertificateRequestPolicy";
      metadata = {
        inherit name;
        labels = labels name;
        annotations = syncWave wave;
      };
      spec = {
        inherit allowed constraints selector;
      };
    };

  # A role leaf: its own SPIFFE URI SAN and the three in-cluster DNS names, a 24h
  # ceiling, server+client auth. isCA:false — a leaf is never a CA.
  rolePolicy =
    role:
    requestPolicy {
      name = "allow-${role}";
      allowed = {
        isCA = false;
        dnsNames = {
          values = [
            role
            "${role}.${ns}"
            "${role}.${ns}.svc"
          ];
          required = true;
        };
        uris = {
          values = [ (roleSan target.deployment role) ];
          required = true;
        };
        usages = [
          "server auth"
          "client auth"
          "digital signature"
        ];
      };
      constraints = {
        maxDuration = "24h";
        privateKey = ecdsaP256;
      };
      selector = leafSelector;
    };

  # The shared signing key: its URI SAN only (no DNS), `digital signature` only, so a
  # request for this SAN can never obtain an mTLS-capable certificate; 2160h ceiling.
  tokenSignerPolicy = requestPolicy {
    name = "allow-${tokenSignerSecret}";
    allowed = {
      isCA = false;
      uris = {
        values = [ "spiffe://${trustDomain}/svc/${tokenSignerSecret}" ];
        required = true;
      };
      usages = [ "digital signature" ];
    };
    constraints = {
      maxDuration = "2160h";
      privateKey = ecdsaP256;
    };
    selector = leafSelector;
  };

  # The root CA: isCA:true, its exact CN, self-signed by the bootstrap issuer in the
  # cert-manager namespace. cert-manager sets no `spec.usages` on the CA request, so
  # `allowed.usages` is left unset (the request must carry none to match). 10y ceiling.
  rootPolicy = requestPolicy {
    name = "allow-${caName}";
    allowed = {
      isCA = true;
      commonName = {
        value = caName;
        required = true;
      };
    };
    constraints = {
      maxDuration = "87600h";
      privateKey = ecdsaP256;
    };
    selector = {
      issuerRef = {
        name = bootstrapIssuer;
        kind = "ClusterIssuer";
        group = "cert-manager.io";
      };
      namespace.matchNames = [ caNamespace ];
    };
  };

  policies = map rolePolicy roles ++ [
    tokenSignerPolicy
    rootPolicy
  ];
  policyNames = map (p: p.metadata.name) policies;

  roleName = "agent-seddon-cert-policy";

  # approver-policy consults a policy only for a requester holding the `use` verb on
  # it. Bind exactly our policies to the cert-manager controller SA — nothing else
  # grants `use`, so no other policy (and no wildcard) is in play.
  clusterRole = {
    apiVersion = "rbac.authorization.k8s.io/v1";
    kind = "ClusterRole";
    metadata = {
      name = roleName;
      labels = labels roleName;
      annotations = syncWave wave;
    };
    rules = [
      {
        apiGroups = [ "policy.cert-manager.io" ];
        resources = [ "certificaterequestpolicies" ];
        verbs = [ "use" ];
        resourceNames = policyNames;
      }
    ];
  };

  clusterRoleBinding = {
    apiVersion = "rbac.authorization.k8s.io/v1";
    kind = "ClusterRoleBinding";
    metadata = {
      name = roleName;
      labels = labels roleName;
      annotations = syncWave wave;
    };
    roleRef = {
      apiGroup = "rbac.authorization.k8s.io";
      kind = "ClusterRole";
      name = roleName;
    };
    subjects = [
      {
        kind = "ServiceAccount";
        name = "cert-manager";
        namespace = caNamespace;
      }
    ];
  };

  manifest = name: obj: {
    inherit name;
    content = toYAML obj;
  };
in
{
  manifests =
    map (p: manifest "approver-policy/certificaterequestpolicy-${p.metadata.name}.yaml" p) policies
    ++ [
      (manifest "approver-policy/clusterrole-${roleName}.yaml" clusterRole)
      (manifest "approver-policy/clusterrolebinding-${roleName}.yaml" clusterRoleBinding)
      (manifest "apps/application-approver-policy.yaml" (application {
        component = "approver-policy";
        namespace = ns;
        inherit (target) repoURL revision;
        target = target.name;
      }))
    ];
}
