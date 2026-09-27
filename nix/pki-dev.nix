# `nix run .#pki-dev` — an offline development PKI for gRPC TLS / mTLS
# (security-hardening S4, docs/design/security-hardening/07-transport-tls-and-pki.md).
#
# Mints a P-256 root CA, a token-signer key (for the S5 agent-token service) and one
# leaf per `--service` (SANs localhost / 127.0.0.1 / ::1 / <name> /
# spiffe://agent.<deployment>/svc/<name>) with smallstep's `step certificate create`
# — no `step-ca` daemon, no network — into $XDG_RUNTIME_DIR/agent-seddon/pki (or
# `--out`), then prints the matching `[grpc.tls]` block. Idempotent; `--force`
# regenerates; `--verify` checks every leaf chains to the root. The logic lives in
# test/pki-dev/pki_dev.py (tested by the `pki-dev-tests` check).
{
  pkgs,
}:
pkgs.writeShellApplication {
  name = "pki-dev";
  runtimeInputs = [
    pkgs.python3
    pkgs.step-cli
  ];
  text = ''
    exec python3 "${../test/pki-dev}/pki_dev.py" --step "${pkgs.step-cli}/bin/step" "$@"
  '';
}
