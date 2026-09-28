# nix/checks/portal-auth-e2e-tests.nix
#
# The portal-auth-e2e harness (`nix run .#portal-auth-e2e`, security-hardening S15c)
# tested like product code: portal_auth_e2e.py's four-class tables (adversarial_ for
# what it reads from the fake IdP's callers, the edge's grpc-web answers, chromedriver
# and the portal's tab storage, which are untrusted input) and check-the-checks —
# every contract step runs against a fake portal + agent + edge that behaves and
# against fakes that each break one promise (the edge admitting a forged bearer, a
# redirect URI off the list, `?code&state` left in the address bar, the PKCE verifier
# left in storage, a replayed callback redeemed, a reload that goes back to the IdP,
# sign-out that leaves the session alive, …), and must fail on every broken one. The
# live run needs a browser and a container runtime, so it runs in `.#integration`.
{
  pkgs,
}:
let
  python = pkgs.python3.withPackages (p: [
    p.pyjwt
    p.cryptography
  ]);
in
pkgs.runCommand "portal-auth-e2e-tests"
  {
    nativeBuildInputs = [ python ];
  }
  ''
    export HOME="$(mktemp -d)"
    # The harness imports its siblings (auth-e2e, portal-envoy) by relative path.
    cp -r ${../../test/auth-e2e} auth-e2e
    cp -r ${../../test/portal-envoy} portal-envoy
    cp -r ${../../test/portal-auth-e2e} portal-auth-e2e
    chmod -R u+w .
    cd portal-auth-e2e
    echo "portal-auth-e2e-tests: tables + check-the-checks ..."
    python3 -m unittest test_portal_auth_e2e -v
    touch "$out"
  ''
