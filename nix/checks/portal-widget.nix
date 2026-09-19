# nix/checks/portal-widget.nix
#
# Layer A of the portal GUI test framework (docs/design/portal-gui-testing): the
# hermetic widget-breadth + L0 unit + completeness-critic suite, run under
# `flutter test` on the Dart VM. The GUI counterpart to the Rust `test` check.
#
# Hermetic exactly like `dart-analyze.nix`: `buildFlutterApplication` vendors every
# pub dep offline from the tracked `portal/pubspec.lock` (no network at test time),
# and we override the build to run `flutter test --no-pub` instead of compiling a
# platform bundle. Unlike `dart-analyze`, the source fileset **includes
# `portal/test/`** — the real suite (testkit fakes, robots, page/spec/meta tests),
# not the deleted `flutter create` scaffold.
#
# The tests dial an in-process fake gRPC server over a real ephemeral **loopback**
# socket (the design's key-feasibility path); the nix build sandbox provides
# loopback, so this stays hermetic and network-free.
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
  pname = "agent-portal-widget";
  version = "0.1.0";
  inherit src;
  autoPubspecLock = portalRoot + "/pubspec.lock";
}).overrideAttrs
  (_: {
    # Test only — collapse the multi-output platform build to one marker output.
    outputs = [ "out" ];
    separateDebugInfo = false;
    buildPhase = ''
      runHook preBuild
      echo "flutter test (offline, portal Layer A)…"
      mkdir -p "$TMPDIR/report"
      # `--machine` emits the JSON event stream the report renderer (inc 8) will
      # consume; tee it so the build log still shows progress, and let pipefail
      # surface a red suite as a failed build.
      set -o pipefail
      flutter test --no-pub --machine | tee "$TMPDIR/report/widget.jsonl"
      runHook postBuild
    '';
    installPhase = ''
      mkdir -p "$out"
      cp "$TMPDIR/report/widget.jsonl" "$out/widget.jsonl" || true
      echo "portal-widget: Layer A suite passed" > "$out/result"
    '';
    doCheck = false;
    doInstallCheck = false;
  })
