# nix/swe-rex.nix
#
# Vendored, pinned build of SWE-ReX (https://github.com/SWE-agent/SWE-ReX) — the sandbox
# execution runtime that SWE-agent (nix/swe-agent.nix) depends on. Not a user-facing tool;
# it exists only so `swe-agent` imports. See docs/swe-agent.md.
#
# Pinned HERE in `nix/versions.nix` (`sweRexVersion`) because nixpkgs has no `swe-rex`.
# Version is a plain literal (`swerex.__version__`), so a GitHub tarball builds cleanly (no
# setuptools_scm). Bump = bump the version + recompute `src.hash`. The `dev`/`modal`/`fargate`/
# `daytona` extras (modal/boto3/daytona-sdk/…) are optional and NOT installed.
{
  pkgs,
  lib,
  version,
}:
let
  py = pkgs.python3Packages;
in
py.buildPythonPackage {
  pname = "swe-rex";
  inherit version;
  pyproject = true;

  src = pkgs.fetchFromGitHub {
    owner = "SWE-agent";
    repo = "SWE-ReX";
    tag = "v${version}";
    hash = "sha256-ITX9iTDy8LWH2I2aL0XOADnB1gEr+V5+jAhZkOrQaec=";
  };

  build-system = [ py.setuptools ];

  # The core (non-extra) runtime deps, all from nixpkgs.
  dependencies = with py; [
    fastapi
    uvicorn
    requests
    pydantic
    pexpect
    bashlex
    python-multipart
    rich
  ];

  # Upstream tests need Docker/cloud sandboxes; nothing to run at build time.
  doCheck = false;

  pythonImportsCheck = [ "swerex" ];

  meta = {
    description = "SWE-ReX — sandboxed execution runtime for SWE-agent (agent-seddon `nix run .#swe-agent` baseline)";
    homepage = "https://github.com/SWE-agent/SWE-ReX";
    changelog = "https://github.com/SWE-agent/SWE-ReX/releases/tag/v${version}";
    license = lib.licenses.mit;
  };
}
