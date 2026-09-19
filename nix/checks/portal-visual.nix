# nix/checks/portal-visual.nix
#
# Layer-A **visual + a11y** gate for the portal (docs/design/portal-gui-testing/
# 03b-visual-and-a11y.md): `flutter test test/visual` — `matchesGoldenFile` pins
# each page's rendered pixels (the "all-icons-white" / broken-layout class the
# RPC-level widget tests can't see) and `meetsGuideline` asserts tap-target size,
# labeled tappables, and text contrast.
#
# Hermetic exactly like `portal-widget.nix` (same offline `autoPubspecLock`
# vendoring); the source fileset includes `portal/test/` — the committed golden
# PNGs under `test/visual/goldens/` ride along. Goldens are generated AND checked
# on the same pinned `versions.flutter`, so rendering is deterministic; regenerate
# with `flutter test --update-goldens test/visual`.
{
  pkgs,
  lib,
  versions,
}:

let
  flutter = versions.flutter;
  portalRoot = ../../portal;
  src = lib.fileset.toSource {
    root = portalRoot;
    fileset = lib.fileset.unions [
      (portalRoot + "/lib")
      (portalRoot + "/test")
      (portalRoot + "/pubspec.yaml")
      (portalRoot + "/pubspec.lock")
      (portalRoot + "/analysis_options.yaml")
      (portalRoot + "/assets")
      (portalRoot + "/fonts")
    ];
  };
in
(flutter.buildFlutterApplication {
  pname = "agent-portal-visual";
  version = "0.1.0";
  inherit src;
  autoPubspecLock = portalRoot + "/pubspec.lock";
}).overrideAttrs
  (_: {
    outputs = [ "out" ];
    separateDebugInfo = false;
    buildPhase = ''
      runHook preBuild
      echo "flutter test (offline, portal visual + a11y)…"
      mkdir -p "$TMPDIR/report"
      set -o pipefail
      flutter test --no-pub test/visual --machine | tee "$TMPDIR/report/visual.jsonl"
      runHook postBuild
    '';
    installPhase = ''
      mkdir -p "$out"
      cp "$TMPDIR/report/visual.jsonl" "$out/visual.jsonl" || true
      echo "portal-visual: golden + a11y suite passed" > "$out/result"
    '';
    doCheck = false;
    doInstallCheck = false;
  })
