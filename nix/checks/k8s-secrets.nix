# nix/checks/k8s-secrets.nix — the `agent-k8s-secrets` crate's own test suite.
#
# Design: docs/design/k8s/07-secrets.md ("Tests").
#
# Gates the CORRECTNESS of the `nix run .#k8s-secrets` deploy tool's pure core:
# manifest parsing, fail-closed source validation (perms / size cap / symlink-escape /
# unsafe key), Secret construction, and the redaction guarantee that no secret value
# reaches an error string or a dry-run summary. Four case classes + the mandatory
# `adversarial_` rows (the untrusted-ish deploy input must be refused). The `test` check
# runs these too under default features; this names the K3 slice-7 gate, the
# `k8s-render-tests` twin.
{
  craneLib,
  commonArgs,
  cargoArtifacts,
}:
craneLib.cargoTest (
  commonArgs
  // {
    inherit cargoArtifacts;
    cargoTestExtraArgs = "-p agent-k8s-secrets";
  }
)
