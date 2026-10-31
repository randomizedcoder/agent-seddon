# nix/portal/web-bundle.nix
#
# A HERMETIC build of the portal's Flutter web bundle — the static tree
# `portal-web` builds at run time with `flutter build web`, produced here as a
# store path instead, so a NixOS host can serve it as a service
# (`services.agentSeddon.portal`) with no Flutter SDK, network, or repo checkout.
#
# Pub deps are vendored offline from the git-tracked `portal/pubspec.lock` (the
# same `buildFlutterApplication` + `autoPubspecLock` route the `dart-analyze` gate
# uses). `portal/web/` is not tracked (`flutter create` regenerates it), so it is
# re-created in `preBuild`; the post-build steps mirror `portal-web` exactly: no
# service worker (`--pwa-strategy=none` + delete), agent-seddon branding + title.
#
# The endpoints are `--dart-define` COMPILE-TIME constants (portal/lib/src/config.dart),
# so they are an argument: `defines` maps `PORTAL_*` keys to values, and a different
# endpoint set is a different bundle. Unset keys keep config.dart's localhost defaults.
{
  pkgs,
  lib,
  versions,
}:

{
  defines ? { },
}:

let
  flutter = versions.flutter;
  portalRoot = ../../portal;
  src = lib.fileset.toSource {
    root = portalRoot;
    fileset = lib.fileset.unions [
      (portalRoot + "/lib")
      (portalRoot + "/pubspec.yaml")
      (portalRoot + "/pubspec.lock")
      (portalRoot + "/analysis_options.yaml")
      (portalRoot + "/assets")
      (portalRoot + "/fonts")
      (portalRoot + "/branding")
    ];
  };
  # Keys are PORTAL_* names; values are operator-owned URLs/hosts. buildFlutterApplication
  # word-splits `$flutterBuildFlags`, so a value with whitespace would become extra
  # flutter arguments — reject it at eval time rather than build a mis-wired bundle.
  defineFlags = lib.mapAttrsToList (
    k: v:
    let
      val = toString v;
    in
    assert lib.assertMsg (
      builtins.match "PORTAL_[A-Z0-9_]+" k != null
    ) "portal web bundle: define key `${k}` is not a PORTAL_* name";
    assert lib.assertMsg (
      builtins.match ".*[[:space:]].*" val == null
    ) "portal web bundle: define `${k}` must not contain whitespace";
    "--dart-define=${k}=${val}"
  ) defines;
in
flutter.buildFlutterApplication {
  pname = "agent-portal-web";
  version = "0.1.0";
  inherit src;
  autoPubspecLock = portalRoot + "/pubspec.lock";
  targetFlutterPlatform = "web";
  flutterBuildFlags = [ "--pwa-strategy=none" ] ++ defineFlags;

  preBuild = ''
    flutter create --no-pub --platforms=web --project-name agent_portal . >/dev/null
  '';

  postInstall = ''
    rm -f "$out/flutter_service_worker.js"
    install -m0644 branding/favicon.png "$out/favicon.png"
    mkdir -p "$out/icons"
    for icon in Icon-192 Icon-512 Icon-maskable-192 Icon-maskable-512; do
      install -m0644 "branding/$icon.png" "$out/icons/$icon.png"
    done
    sed -i 's#<title>agent_portal</title>#<title>Agent Seddon</title>#' "$out/index.html"
  '';
}
