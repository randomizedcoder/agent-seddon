# `nix run .#auth-e2e` — the whole sign-in chain over real processes (security-
# hardening S15a, docs/design/security-hardening/04-service-integration.md "Process
# wire"): a fake OIDC issuer, the offline dev PKI, `agent --serve-memory` (B) and
# `agent --serve-all` (A, memory = "grpc" → B), both over mTLS under `mode = "oidc"`,
# driven with grpcurl. The same run is the `auth-e2e` gate check; this app is for
# re-running it by hand (`-- --keep` keeps the work directory and server logs).
# The logic lives in test/auth-e2e/auth_e2e.py.
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
in
pkgs.writeShellApplication {
  name = "auth-e2e";
  runtimeInputs = [ python ];
  text = ''
    exec python3 "${../test/auth-e2e}/auth_e2e.py" \
      --agent "${agent}/bin/agent" \
      --grpcurl "${versions.grpcurl}/bin/grpcurl" \
      --pki-dev "${python}/bin/python3 ${../test/pki-dev}/pki_dev.py --step ${pkgs.step-cli}/bin/step" \
      "$@"
  '';
}
