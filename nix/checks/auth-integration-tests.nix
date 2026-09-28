# nix/checks/auth-integration-tests.nix
#
# The auth-integration harness (`nix run .#auth-integration`, security-hardening
# S15b) tested like product code: auth_integration.py's four-class tables
# (adversarial_ for the Postgres password in the DSN, psql/ClickHouse rows and
# certificate inspection, which are untrusted input) and check-the-checks — every
# contract step runs against a fake that behaves and against fakes that each break
# one promise (a renewal that keeps the serial, a token honoured off a bound
# certificate, sessions forgotten on restart, a replayed refresh handle accepted,
# a missing audit event, a reader that sees another tenant, token material in an
# audit row) and must fail on every broken one. The live tiers need a step-ca
# daemon and a container runtime, so they run in `.#integration`.
{
  pkgs,
}:
let
  python = pkgs.python3.withPackages (p: [
    p.pyjwt
    p.cryptography
  ]);
in
pkgs.runCommand "auth-integration-tests"
  {
    nativeBuildInputs = [ python ];
  }
  ''
    export HOME="$(mktemp -d)"
    # The harness imports its siblings (auth-e2e, clickhouse) by relative path.
    cp -r ${../../test/auth-e2e} auth-e2e
    cp -r ${../../test/clickhouse} clickhouse
    cp -r ${../../test/auth-integration} auth-integration
    chmod -R u+w .
    cd auth-integration
    echo "auth-integration-tests: tables + check-the-checks ..."
    python3 -m unittest test_auth_integration -v
    touch "$out"
  ''
