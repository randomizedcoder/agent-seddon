# nix/checks/mt-audit.nix
#
# The repo-wide multi-tenancy coverage GATE: runs the auditor (`mt-audit --gate`)
# against the real source and fails the build on ANY finding — an unclassified new
# service or metric family, a `scoped` handler gone span-only, a `PerTenant`-wrapped
# seam served non-scoped, a manifest class outside the closed set, or a removed
# span/log/config mechanism. This is the constants-sync / buf-breaking duality: the
# SAME `audit.py` entrypoint backs both `nix run .#mt-audit` (report) and this gate,
# so report and gate can never disagree. The fix for a failure is to classify the new
# surface in `test/mt-audit/manifest.toml` (the deliberate, reviewed baseline move).
#
# The auditor's OWN correctness (parsers + checkers + manifest self-consistency) is
# gated separately by `mt-audit-tests`; this gates the source against the manifest.
#
# `src` is the crane-filtered source (all `crates/**/*.rs` — every file the audit
# reads); `audit.py` + `manifest.toml` travel together from the repo tree by nix path.
{
  pkgs,
  src,
}:
pkgs.runCommand "mt-audit-gate"
  {
    nativeBuildInputs = [ pkgs.python3 ];
  }
  ''
    export HOME="$(mktemp -d)"
    echo "mt-audit: reconciling the multi-tenancy surface against the committed manifest ..."
    python3 ${../../test/mt-audit}/audit.py --gate --repo-root ${src}
    touch "$out"
  ''
