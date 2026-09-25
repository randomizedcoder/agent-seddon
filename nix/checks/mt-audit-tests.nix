# nix/checks/mt-audit-tests.nix
#
# The multi-tenancy auditor is tested like product code (docs/design/multi-tenancy/):
# audit.py's four-class tables (positive_/negative_/boundary_/corner_ + adversarial_ for the
# untrusted source text the parsers ingest) plus a check-the-checks matrix asserting every
# checker genuinely REJECTS a fail fixture (an always-clean auditor fails the build), and a
# self-consistency test that the committed manifest.toml loads and its classifications are
# well-formed. Pure stdlib Python; no network, no model, no repo build.
#
# This gates the auditor's OWN correctness. The repo-wide `mt-audit --gate` gate (which runs
# the auditor against the real source) is added once the known coverage gaps it reports are
# fixed — the constants-sync/buf duality, one entrypoint shared by report + gate.
{
  pkgs,
}:
pkgs.runCommand "mt-audit-tests"
  {
    nativeBuildInputs = [ pkgs.python3 ];
  }
  ''
    export HOME="$(mktemp -d)"
    cp -r ${../../test/mt-audit} mt-audit
    chmod -R u+w mt-audit
    cd mt-audit
    echo "mt-audit-tests: parser + checker tables + check-the-checks + manifest consistency ..."
    python3 -m unittest test_audit -v
    touch "$out"
  ''
