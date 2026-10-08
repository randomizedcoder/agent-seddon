# nix/checks/k8s-kubeconform.nix — schema-validate every rendered object.
#
# Design: docs/design/k8s/04-manifests-and-gitops.md ("Gate").
#
# `k8s-rendered` proves the committed tree is a faithful render and
# `k8s-render-tests` proves the objects obey our hardening/identity invariants —
# but neither proves the manifests are *valid Kubernetes*: a typo'd field, a wrong
# type, or an apiVersion that no schema accepts would sail through both. This check
# runs `kubeconform -strict` over the whole `rendered/k3s` tree, including the
# cert-manager and ArgoCD custom resources.
#
# Hermetic + offline: the sandbox has no network, so we cannot let kubeconform
# reach its default upstream (yannh master). Instead we VENDOR exactly the schemas
# our objects need, each pinned by repository commit SHA *and* content hash, and
# point kubeconform at two local `-schema-location` templates (core + CRD). No
# `-ignore-missing-schemas`: if a rendered kind has no vendored schema the gate
# fails loudly rather than skipping it — so adding a new object kind forces a
# matching schema pin here, by design.
#
# Pins (refresh a hash with `nix store prefetch-file --json <raw-github-url>`):
#   - core k8s schemas: yannh/kubernetes-json-schema, standalone-strict, v1.36.4
#     (matches the k3s node, k8s 1.36.4); naming `<kind>-<group>-<version>.json`.
#   - CRD schemas: datreeio/CRDs-catalog; naming `<group>/<kind>_<version>.json`.
# Cilium CRDs are intentionally absent: no CiliumNetworkPolicy renders at K3 (the
# policy component lands later); its schema pin arrives with it.
{
  pkgs,
  lib,
  src,
}:
let
  # yannh/kubernetes-json-schema @ this commit, v1.36.4-standalone-strict/.
  k8sSchemaRev = "8df8a883b68a24a104b4a9e43c1288090ae60b3b";
  k8sSchemaUrl =
    name:
    "https://raw.githubusercontent.com/yannh/kubernetes-json-schema/${k8sSchemaRev}/v1.36.4-standalone-strict/${name}";

  # datreeio/CRDs-catalog @ this commit, <group>/<kind>_<version>.json.
  crdSchemaRev = "fd90051867733c60d32d16450556e9cd18459aef";
  crdSchemaUrl =
    name: "https://raw.githubusercontent.com/datreeio/CRDs-catalog/${crdSchemaRev}/${name}";

  # Core kinds rendered at K3: Deployment (apps/v1), Service + ConfigMap (v1).
  coreSchemas = {
    "deployment-apps-v1.json" = "sha256-NyV4L7AePyfYvi2lZeLWU9e3i/bevlRAgEzqmTyHuPk=";
    "service-v1.json" = "sha256-i/AZhU2u1RHnwXSJaomBc/pl2I7Fk3xoejcwPUzJNRs=";
    "configmap-v1.json" = "sha256-4Ord69Z3wIqgkrLaImTYasT8NO7RErn6wpRbPwDB6bE=";
  };

  # Custom resources rendered at K3: cert-manager Certificate + ClusterIssuer,
  # ArgoCD Application. Keyed by the datreeio `<group>/<kind>_<version>.json` path.
  crdSchemas = {
    "cert-manager.io/certificate_v1.json" = "sha256-/jjlcrX9nsSWMQRLbEIHhjEm0EZc6bbgZzziAPykcbo=";
    "cert-manager.io/clusterissuer_v1.json" = "sha256-qvQgdTNYmO9YieA3C+yEMlET44hjW+G49yDRFpHN/Lo=";
    "argoproj.io/application_v1alpha1.json" = "sha256-mcN4d7Bibq8h9C9w9PxDKkuCDeVI2I7ZDMdTga31GSw=";
  };

  # Assemble a flat directory of the core schemas (yannh `<kind><suffix>.json`
  # naming → kubeconform template `{{.ResourceKind}}{{.KindSuffix}}.json`).
  coreDir = pkgs.runCommand "k8s-core-schemas" { } (
    ''
      mkdir -p "$out"
    ''
    + lib.concatStrings (
      lib.mapAttrsToList (name: hash: ''
        cp ${
          pkgs.fetchurl {
            url = k8sSchemaUrl name;
            inherit hash;
          }
        } "$out/${name}"
      '') coreSchemas
    )
  );

  # Assemble the CRD schemas under their group subdirectory (datreeio
  # `<group>/<kind>_<version>.json` → template `{{.Group}}/{{.ResourceKind}}_{{.ResourceAPIVersion}}.json`).
  crdDir = pkgs.runCommand "k8s-crd-schemas" { } (
    ''
      mkdir -p "$out"
    ''
    + lib.concatStrings (
      lib.mapAttrsToList (name: hash: ''
        mkdir -p "$out/$(dirname ${name})"
        cp ${
          pkgs.fetchurl {
            url = crdSchemaUrl name;
            inherit hash;
          }
        } "$out/${name}"
      '') crdSchemas
    )
  );
in
pkgs.runCommand "k8s-kubeconform"
  {
    nativeBuildInputs = [ pkgs.kubeconform ];
  }
  ''
    echo "k8s-kubeconform: validating rendered/k3s against vendored schemas (kubeconform -strict) ..."
    kubeconform \
      -strict \
      -summary \
      -schema-location '${coreDir}/{{.ResourceKind}}{{.KindSuffix}}.json' \
      -schema-location '${crdDir}/{{.Group}}/{{.ResourceKind}}_{{.ResourceAPIVersion}}.json' \
      ${src}/rendered/k3s
    touch $out
  ''
