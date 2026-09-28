# nix/portal-auth-e2e.nix
#
# `portal-auth-e2e` — browser sign-in through the hardened edge (security-hardening
# S15c). A real headless Chromium (W3C WebDriver via chromedriver) loads the real
# portal web build and signs in: fake OIDC IdP with the authorization-code flow →
# `AuthService.Begin` → the IdP's consent redirect → `Exchange` (the agent redeems the
# code with the client secret) → an agent token the S14 Envoy bridge's `jwt_authn`
# accepts. It also checks the refusals: no/forged bearer at the edge, a replayed or
# forged callback, an IdP refusal, and a signed-out session's refresh handle.
#
# Everything is on loopback with its own ports and container name (nix/versions.nix),
# so the long-lived portal bridge and gateways are never touched. The Envoy bridge is
# a container: without a reachable runtime ($CONTAINER_RUNTIME, default docker) the
# run is skipped with a notice (exit 0). Chromium + chromedriver come from the
# binary-cached nixpkgs registry at run time, as for `portal-e2e` (override with
# PORTAL_E2E_CHROMIUM / PORTAL_E2E_CHROMEDRIVER). The logic lives in
# test/portal-auth-e2e/portal_auth_e2e.py, tested by the `portal-auth-e2e-tests` gate
# check. Registered in `nix/integration.nix`.
#
# Exit codes (the shared 0/1/2 contract): 0 clean or skipped, 1 a harness failure,
# 2 a contract failure.
{
  pkgs,
  versions,
  agent,
}:
let
  python = pkgs.python3.withPackages (p: [
    p.pyjwt
    p.cryptography
  ]);
  # portal_auth_e2e.py imports auth-e2e as a sibling by path (Path.resolve() follows
  # symlinks, so copies, not a linkFarm).
  src = pkgs.runCommand "portal-auth-e2e-src" { } ''
    mkdir -p "$out"
    cp -r ${../test/auth-e2e} "$out/auth-e2e"
    cp -r ${../test/portal-auth-e2e} "$out/portal-auth-e2e"
  '';
in
pkgs.writeShellApplication {
  name = "portal-auth-e2e";
  runtimeInputs = [
    python
    pkgs.nix # resolve the cached chromium + chromedriver at run time
    versions.docker
    versions.podman
  ];
  text = ''
    exec python3 "${src}/portal-auth-e2e/portal_auth_e2e.py" \
      --agent "${agent}/bin/agent" \
      --grpcurl "${versions.grpcurl}/bin/grpcurl" \
      --pki-dev "${python}/bin/python3 ${../test/pki-dev}/pki_dev.py --step ${pkgs.step-cli}/bin/step" \
      --flutter "${versions.flutter}/bin/flutter" \
      --static-web-server "${versions.static-web-server}/bin/static-web-server" \
      --portal-src "${../portal}" \
      --portal-envoy "${../test/portal-envoy}/portal_envoy.py" \
      --proto-dir "${../crates/agent-proto/proto}" \
      --envoy-image "docker.io/${versions.envoyImage}" \
      --envoy-name "${versions.portalAuthTestEnvoyContainerName}" \
      --edge-port ${toString versions.portalAuthTestEdgePort} \
      --web-port ${toString versions.portalAuthTestWebPort} \
      --runtime "''${CONTAINER_RUNTIME:-docker}" \
      "$@"
  '';
}
