# nix/checks/review-toolbox.nix
#
# Asserts the review static-analysis toolbox (nix/review-tools.nix) is provisioned
# and on the PACKAGED agent's runtime PATH. This is the reproducibility contract of
# the review-analysis-depth track: every analysis tool is nix-provided (one nixpkgs
# pin floats them all), and a `--serve-fleet` process / container finds the linters
# with no ambient environment — the same wrapper mechanism that put `agent-go-ast`
# on PATH (nix/default.nix `agentRuntimePath`).
#
# Hermetic + offline: it only resolves symlinks and greps the built agent wrapper.
{
  pkgs,
  agent,
  toolbox,
}:
pkgs.runCommand "review-toolbox-check" { } ''
  set -eu

  # 1. The toolbox bundles the expected analysis binaries (the symlinks resolve to
  #    executables) — so a single nixpkgs bump floats a real, runnable suite.
  for t in golangci-lint gosec go gofmt cargo-audit cargo-deny; do
    if [ ! -x "${toolbox}/bin/$t" ]; then
      echo "FAIL: review toolbox is missing an executable '$t'" >&2
      exit 1
    fi
  done

  # 2. The packaged agent carries the toolbox on its runtime PATH (the
  #    `agentRuntimePath` makeWrapper prefix), so the fleet reviewer resolves the
  #    linters without an ambient environment. The wrapper is a shell script that
  #    embeds the toolbox store path in its `PATH=` prefix.
  if ! grep -q "${toolbox}" "${agent}/bin/agent"; then
    echo "FAIL: the packaged agent does not put the review toolbox on PATH" >&2
    exit 1
  fi

  echo "OK: review toolbox provisioned + on the agent runtime PATH" > "$out"
''
