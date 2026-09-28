# nix/checks/auth-e2e.nix
#
# The sign-in chain end to end over real processes (security-hardening S15a):
# auth_e2e.py's four-class tables and check-the-checks (every step must go red
# against a fake agent that gets it wrong), then the live run — a fake OIDC issuer,
# the offline dev PKI (step-cli), `agent --serve-memory` and `agent --serve-all`
# (memory = "grpc" → the first) over mTLS under `mode = "oidc"`, driven with
# grpcurl on loopback. Checks: login tokens only at `Exchange` and only when they
# verify; the verified tenant beats the header; a write through A lands in B's
# partition for that tenant (the bearer crosses the `= "grpc"` hop) and the other
# tenant can't read it; mTLS service identity; refresh rotation, replay and logout;
# the forwarding-hop ceiling. Loopback only, no network.
{
  pkgs,
  agent,
  versions,
}:
let
  python = pkgs.python3.withPackages (p: [
    p.pyjwt
    p.cryptography
  ]);
in
pkgs.runCommand "auth-e2e"
  {
    nativeBuildInputs = [
      python
      pkgs.step-cli
    ];
  }
  ''
    export HOME="$(mktemp -d)"
    export STEPPATH="$HOME/.step"
    export AUTH_E2E_AGENT="${agent}/bin/agent"
    export AUTH_E2E_GRPCURL="${versions.grpcurl}/bin/grpcurl"
    export AUTH_E2E_PKI_DEV="${python}/bin/python3 ${../../test/pki-dev}/pki_dev.py --step ${pkgs.step-cli}/bin/step"
    cp -r ${../../test/auth-e2e} auth-e2e
    chmod -R u+w auth-e2e
    cd auth-e2e
    echo "auth-e2e: tables + check-the-checks + the live chain ..."
    python3 -m unittest test_auth_e2e -v
    touch "$out"
  ''
