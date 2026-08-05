# nix/swe-agent.nix
#
# Vendored, pinned build of SWE-agent (https://github.com/SWE-agent/SWE-agent) — Princeton's
# reference agent scaffold for SWE-bench. It drives `nix run .#swe-agent`, which is a
# COMPARISON BASELINE (not an eval of our agent): run SWE-agent with the SAME model on the
# SAME SWE-bench instances, grade with the swebench Docker harness we already have, and read
# resolved% side-by-side with `nix run .#swebench`. See docs/swe-agent.md.
#
# Pinned HERE in `nix/versions.nix` (`sweAgentVersion`) because nixpkgs has no `sweagent`.
# Version is a plain literal (`sweagent.__version__`), so a GitHub tarball builds cleanly.
# Everything but SWE-ReX (nix/swe-rex.nix, also vendored) comes from nixpkgs — including the
# heavy `litellm`. Bump = bump the version + recompute `src.hash`.
{
  pkgs,
  lib,
  version,
  swe-rex,
}:
let
  py = pkgs.python3Packages;
in
py.buildPythonPackage {
  pname = "swe-agent";
  inherit version;
  pyproject = true;

  src = pkgs.fetchFromGitHub {
    owner = "SWE-agent";
    repo = "SWE-agent";
    tag = "v${version}";
    hash = "sha256-WDZZXjN/nWFB2onikvZAlmfFWfrt0ImhYJzC/GRgIdE=";
  };

  build-system = [ py.setuptools ];

  # Its deps carry version pins (litellm/textual/ghapi/swe-rex); nixpkgs satisfies them, but
  # skip the metadata check and let `pythonImportsCheck` be the proof rather than chase pins.
  dontCheckRuntimeDeps = true;

  dependencies =
    (with py; [
      datasets
      numpy
      pandas
      rich
      ruamel-yaml
      tenacity
      unidiff
      simple-parsing
      rich-argparse
      flask
      flask-cors
      flask-socketio
      pydantic
      python-dotenv
      pydantic-settings
      litellm
      gitpython
      ghapi
      tabulate
      textual
      requests
    ])
    ++ [ swe-rex ];

  doCheck = false;

  # SWE-agent's `__init__` asserts that `config/`, `tools/` and `trajectories/` dirs exist
  # next to the package — a repo layout that a wheel install drops. Ship config/ + tools/
  # (read-only data) under $out/share and point the env vars (honored by BOTH the import
  # asserts and the CLI) at them. trajectories/ is an output dir; provide a default so a bare
  # import passes, and let the harness override SWE_AGENT_TRAJECTORY_DIR with a writable path.
  #
  # SWE_AGENT_CONFIG_ROOT is the base a config's RELATIVE paths (e.g. a bundle `tools/registry`
  # in the shipped configs) resolve against (`_convert_path_to_abspath`); it defaults to the
  # package dir, where `tools/` isn't. Point it at $out/share/swe-agent so the shipped configs
  # find their tool bundles.
  postInstall = ''
    mkdir -p $out/share/swe-agent/trajectories
    cp -r config tools $out/share/swe-agent/
  '';

  env = {
    SWE_AGENT_CONFIG_DIR = "${placeholder "out"}/share/swe-agent/config";
    SWE_AGENT_TOOLS_DIR = "${placeholder "out"}/share/swe-agent/tools";
    SWE_AGENT_TRAJECTORY_DIR = "${placeholder "out"}/share/swe-agent/trajectories";
    SWE_AGENT_CONFIG_ROOT = "${placeholder "out"}/share/swe-agent";
  };

  # Bake the same defaults into the `sweagent` console script so the CLI works out of the box;
  # `--set-default` lets the harness (or an operator) override any of them.
  makeWrapperArgs = [
    "--set-default SWE_AGENT_CONFIG_DIR ${placeholder "out"}/share/swe-agent/config"
    "--set-default SWE_AGENT_TOOLS_DIR ${placeholder "out"}/share/swe-agent/tools"
    "--set-default SWE_AGENT_TRAJECTORY_DIR ${placeholder "out"}/share/swe-agent/trajectories"
    "--set-default SWE_AGENT_CONFIG_ROOT ${placeholder "out"}/share/swe-agent"
  ];

  pythonImportsCheck = [ "sweagent" ];

  meta = {
    description = "SWE-agent — reference SWE-bench agent scaffold; the agent-seddon `nix run .#swe-agent` comparison baseline";
    homepage = "https://swe-agent.com/";
    changelog = "https://github.com/SWE-agent/SWE-agent/releases/tag/v${version}";
    license = lib.licenses.mit;
    mainProgram = "sweagent";
  };
}
