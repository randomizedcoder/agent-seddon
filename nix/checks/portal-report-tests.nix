# nix/checks/portal-report-tests.nix
#
# The portal report renderer is tested like product code (docs/design/
# portal-gui-testing/05-report.md): render.py's four-class tables
# (positive_/negative_/boundary_/corner_ + adversarial_ for the untrusted JSON
# inputs) plus a check-the-checks matrix asserting the renderer genuinely
# distinguishes pass/fail/skip and surfaces the backend-down legend — so an
# always-green renderer fails the build. Pure stdlib Python; no network, no model.
{
  pkgs,
}:
pkgs.runCommand "portal-report-tests"
  {
    nativeBuildInputs = [ pkgs.python3 ];
  }
  ''
    export HOME="$(mktemp -d)"
    cp -r ${../../test/portal-report} report
    chmod -R u+w report
    cd report
    echo "portal-report-tests: renderer tables + check-the-checks ..."
    python3 -m unittest test_render -v
    touch "$out"
  ''
