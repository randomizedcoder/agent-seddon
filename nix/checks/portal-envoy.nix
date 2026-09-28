# nix/checks/portal-envoy.nix
#
# The portal's Envoy grpc-web bridge config (security-hardening S14), tested like
# product code: portal_envoy.py's four-class tables (adversarial_ for every env knob
# that reaches the config: origins, bind, JWKS, TLS paths, the OTLP key), then — with
# the real Envoy (the cached `envoy-bin`, same minor as the container image) and
# offline step-cli certificates — `envoy --mode validate` on every rendered mode
# (auth off / local JWKS / remote JWKS / LAN bind / TLS + upstream mTLS) over the
# real listener spec, and check-the-checks: validate must REJECT a corrupt JWKS, an
# unknown provider and a missing key file (an always-green validator fails the build).
{
  pkgs,
  versions,
}:
let
  envoySpec = import ../portal/envoy-spec.nix { inherit pkgs versions; };
in
pkgs.runCommand "portal-envoy"
  {
    nativeBuildInputs = [
      pkgs.python3
      pkgs.step-cli
    ];
  }
  ''
    export HOME="$(mktemp -d)"
    export STEPPATH="$HOME/.step"
    export PORTAL_ENVOY_BIN="${versions.envoy-bin}/bin/envoy"
    export PORTAL_ENVOY_STEP="${pkgs.step-cli}/bin/step"
    export PORTAL_ENVOY_SPEC="${envoySpec.file}"
    cp -r ${../../test/portal-envoy} portal-envoy
    chmod -R u+w portal-envoy
    cd portal-envoy
    echo "portal-envoy: tables + envoy --mode validate + check-the-checks ..."
    python3 -m unittest test_portal_envoy -v
    touch "$out"
  ''
