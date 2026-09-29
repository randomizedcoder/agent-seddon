# nix/checks/ch-creds-tests.nix
#
# The ClickHouse credentials helper (`nix run .#clickhouse-creds`, security-hardening
# S16) tested like product code: ch_creds.py's four-class tables (adversarial_ for the
# password-file contents and role names, which are untrusted input), plus
# check-the-checks for the live RLS harness's row matcher (rls_harness.py) — a
# matcher that always passed would make `nix run .#ch-integration` meaningless. The
# live matrix itself needs a container runtime, so it runs in `.#integration`.
{
  pkgs,
}:
pkgs.runCommand "ch-creds-tests"
  {
    nativeBuildInputs = [ pkgs.python3 ];
  }
  ''
    export HOME="$(mktemp -d)"
    cp -r ${../../test/clickhouse} clickhouse
    chmod -R u+w clickhouse
    cd clickhouse
    export CH_SCHEMA=${../../nix/clickhouse/schema.sql}
    echo "ch-creds-tests: credential tables + harness check-the-checks ..."
    python3 -m unittest test_ch_creds -v
    touch "$out"
  ''
