# nix/checks/k8s-render-tests.nix
#
# Correctness gate for the rendered Kubernetes manifests (k8s track K3, Slice 5). Where
# `k8s-rendered` proves the committed tree equals a fresh render, this proves the tree is
# *correct*: hardened securityContext, probes, waves, labels, exec-seam exclude (parsed,
# not regex-scraped), no exec-seam exposure, SPIFFE SAN shape, CA chain + tls-Secret
# bijection, and no secret-looking ConfigMap material (plaintext `data` AND base64
# `binaryData`). The invariants and their `adversarial_` check-the-checks live in the
# `agent-k8s-render` crate; this runs that crate's test binary.
#
# The crate's tests read the committed tree from `$AGENT_RENDERED_K3S`, pointed here at the
# raw flake source's rendered/k3s. The YAML is deliberately NOT in the crane source: the
# manifests name the image by its content-hash tag, so building the agent from a source that
# contains them would move the tag on every re-render (see nix/default.nix). For the same
# reason the package build excludes this crate. The workspace `test` and `coverage` checks
# also point it at the tree; this crate-scoped check is the named Slice-5 gate (and the one
# the "hand-break a manifest → gate fails" meta-check targets).
{
  craneLib,
  commonArgs,
  cargoArtifacts,
  renderedK3s,
}:

craneLib.cargoTest (
  commonArgs
  // {
    inherit cargoArtifacts;
    cargoTestExtraArgs = "-p agent-k8s-render";
    AGENT_RENDERED_K3S = renderedK3s;
  }
)
