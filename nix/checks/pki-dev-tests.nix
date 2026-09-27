# nix/checks/pki-dev-tests.nix
#
# The dev-PKI generator (`nix run .#pki-dev`) tested like product code: pki_dev.py's
# four-class tables (adversarial_ for the names that become paths + SANs) against a
# fake `step`, then — with the real step-cli, offline inside the sandbox — an
# end-to-end mint + `--verify`, and check-the-checks: `--verify` must REJECT a leaf
# swapped in from another CA and a corrupt certificate (an always-green verifier fails
# the build).
{
  pkgs,
}:
pkgs.runCommand "pki-dev-tests"
  {
    nativeBuildInputs = [
      pkgs.python3
      pkgs.step-cli
    ];
  }
  ''
    export HOME="$(mktemp -d)"
    export STEPPATH="$HOME/.step"
    export PKI_DEV_STEP="${pkgs.step-cli}/bin/step"
    cp -r ${../../test/pki-dev} pki-dev
    chmod -R u+w pki-dev
    cd pki-dev
    echo "pki-dev-tests: tables + real step-cli mint/verify + check-the-checks ..."
    python3 -m unittest test_pki_dev -v
    touch "$out"
  ''
