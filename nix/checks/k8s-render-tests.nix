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
# The crate's tests read the committed tree via `env!("CARGO_MANIFEST_DIR")/../../rendered/k3s`,
# so the rendered YAML is whitelisted into the crane source filter (see nix/default.nix).
# The workspace-wide `test` check also exercises these tests under default features; this
# dedicated, crate-scoped check is the named Slice-5 gate (and the one the "hand-break a
# manifest → gate fails" meta-check targets).
{
  craneLib,
  commonArgs,
  cargoArtifacts,
}:

craneLib.cargoTest (
  commonArgs
  // {
    inherit cargoArtifacts;
    cargoTestExtraArgs = "-p agent-k8s-render";
  }
)
