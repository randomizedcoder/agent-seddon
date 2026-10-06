# nix/checks/k8s-render-tests.nix — the `k8s-render-tests` gate (k8s track K3).
#
# A table-driven Python suite that walks the committed `rendered/k3s/` tree and pins the
# renderer's invariants (hardened securityContext, probes, sync waves, labels, the
# gateway exec-seam exclude, no exec-seam exposure, SPIFFE SAN shape, the CA chain and
# tls-Secret cross-references, no secret-looking ConfigMap material) plus the mandatory
# `adversarial_` check-the-checks rows. Sits beside `k8s-rendered`, which proves the
# committed tree equals a fresh render; this one proves it is *correct*.
#
# Sources live in test/k8s-render/ (repo convention: one dir per check, python3 -m
# unittest). PyYAML is the one non-stdlib dep, brought in hermetically via withPackages.
{
  pkgs,
  src,
}:
let
  python = pkgs.python3.withPackages (p: [ p.pyyaml ]);
in
pkgs.runCommand "k8s-render-tests"
  {
    nativeBuildInputs = [ python ];
  }
  ''
    export HOME="$(mktemp -d)"
    export AGENT_RENDERED_K3S=${src}/rendered/k3s
    cp -r ${../../test/k8s-render} k8s-render
    chmod -R u+w k8s-render
    cd k8s-render
    echo "k8s-render-tests: invariant tables + check-the-checks fixtures ..."
    python3 -m unittest test_render -v
    touch "$out"
  ''
