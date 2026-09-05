# nix/checks/dart-analyze.nix
#
# `flutter analyze` over the Flutter/gRPC portal (`portal/`) — the Dart
# counterpart to `clippy` for the Rust tree. Fails the gate on any analyzer
# error/warning (portal/analysis_options.yaml governs the lint set).
#
# Hermetic: `buildFlutterApplication` vendors every pub.dev dependency offline
# (hashes derived from `portal/pubspec.lock`, no network at analyze time), and
# we override the build to run `flutter analyze --no-pub` instead of compiling a
# platform bundle. The generated gRPC stubs (`portal/lib/src/gen/…`, produced by
# `nix run .#gen-dart`) are committed and analyzed alongside the hand-written UI.
#
# Scope is limited to the sources the analyzer needs — `lib/` plus the pub
# manifests and `analysis_options.yaml`. `portal/test/` is deliberately excluded:
# it holds only the stale `flutter create` scaffold (references a non-existent
# `MyApp`), not a real suite.
{
  pkgs,
  lib,
  versions,
}:

let
  flutter = versions.flutter;
  portalRoot = ../../portal;
  # Only the inputs `flutter analyze` reads. Excludes test/ (stale scaffold) and
  # build artifacts, keeping the derivation's input hash tight.
  src = lib.fileset.toSource {
    root = portalRoot;
    fileset = lib.fileset.unions [
      (portalRoot + "/lib")
      (portalRoot + "/pubspec.yaml")
      (portalRoot + "/pubspec.lock")
      (portalRoot + "/analysis_options.yaml")
    ];
  };
in
(flutter.buildFlutterApplication {
  pname = "agent-portal-analyze";
  version = "0.1.0";
  inherit src;
  autoPubspecLock = portalRoot + "/pubspec.lock";
}).overrideAttrs
  (_: {
    # Analyze only — collapse the multi-output platform build to a single marker.
    outputs = [ "out" ];
    separateDebugInfo = false;
    buildPhase = ''
      runHook preBuild
      echo "flutter analyze (offline, portal)…"
      flutter analyze --no-pub
      runHook postBuild
    '';
    installPhase = ''
      echo "flutter analyze: clean" > $out
    '';
    doCheck = false;
    doInstallCheck = false;
  })
