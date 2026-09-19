# `nix run .#portal-test-report -- <jsonl>...` — render the portal GUI test report.
#
# Merges the per-layer JSON a portal test run emits (the hermetic checks'
# `flutter test --machine` streams — e.g. `portal-widget`'s `$out/widget.jsonl` —
# and the Layer-B `portal-e2e` app's rich records) into one page → element → case
# Markdown report (docs/design/portal-gui-testing/05-report.md).
#
# Per the repo's avoid-bash convention this wrapper is a pure exec shim; all
# parsing/rendering lives in test/portal-report/render.py (unit-tested by the
# `portal-report-tests` flake check).
{
  pkgs,
}:
pkgs.writeShellApplication {
  name = "portal-test-report";
  runtimeInputs = [ pkgs.python3 ];
  text = ''
    exec python3 "${../test/portal-report}/render.py" "$@"
  '';
}
