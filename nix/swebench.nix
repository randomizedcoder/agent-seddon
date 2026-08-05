# nix/swebench.nix
#
# Vendored, pinned build of SWE-bench (https://www.swebench.com/) — the official
# software-engineering benchmark. Its evaluation harness
# (`python -m swebench.harness.run_evaluation`) drives the per-instance Docker grading
# behind `nix run .#swebench`; the inference half (driving the REAL agent to produce a
# patch) is `test/swebench/predict.py`. See docs/swebench.md.
#
# We keep our OWN derivation (rather than a nixpkgs package — there ISN'T one; swebench
# is PyPI/GitHub-only) so the version is pinned HERE, in `nix/versions.nix`
# (`swebenchVersion`), tracking the latest release directly. This is a `buildPythonPackage`
# LIBRARY (not an application) so the harness can pull it into a `python3.withPackages`
# and run `python -m swebench.harness.run_evaluation`. Bumping the pin means bumping
# `swebenchVersion` and recomputing `src.hash` (set it to `lib.fakeHash`, build, copy the
# "got:" line).
#
# swebench is NOT a Nix flake (no flake.nix upstream), so it cannot be a flake input;
# a `buildPythonPackage` derivation is the supported path.
{
  pkgs,
  lib,
  version,
}:
let
  py = pkgs.python3Packages;
in
py.buildPythonPackage {
  pname = "swebench";
  inherit version;
  pyproject = true;

  src = pkgs.fetchFromGitHub {
    owner = "SWE-bench";
    repo = "SWE-bench";
    tag = "v${version}";
    hash = "sha256-TpXNqSRg+3+oxeb3Hh/JmBkq1wrf1utNKgBJl6tBT48=";
  };

  build-system = [ py.setuptools ];

  # `pre-commit` is a dev-only declared dependency swebench never imports at runtime;
  # drop it from the wheel metadata so `pythonRuntimeDepsCheckHook` doesn't demand it
  # (it isn't packaged under python3Packages here). Everything else is a real import.
  pythonRemoveDeps = [ "pre-commit" ];

  # The unpinned runtime deps from swebench's pyproject `dependencies`. `modal` is a hard
  # top-level import of `swebench.harness.run_evaluation` (via `swebench.harness.modal_eval`),
  # not optional — the module won't load without it. `docker` + `datasets` are the other
  # heavy ones the eval harness needs.
  dependencies = with py; [
    beautifulsoup4
    chardet
    datasets
    docker
    ghapi
    gitpython
    modal
    python-dotenv
    requests
    rich
    tenacity
    tqdm
    unidiff
  ];

  # swebench's own test-suite is Docker-bound (it builds/pulls instance images); nothing
  # to run at Nix build time.
  doCheck = false;

  # Cheap build-time proof the eval entrypoint's whole import graph resolves — this
  # transitively imports docker/datasets/modal, so a missing/broken dep fails HERE, not at
  # `nix run .#swebench` time.
  pythonImportsCheck = [
    "swebench"
    "swebench.harness.run_evaluation"
  ];

  meta = {
    description = "SWE-bench — evaluate LMs on real GitHub issues; drives the agent-seddon `nix run .#swebench` benchmark";
    homepage = "https://www.swebench.com/";
    changelog = "https://github.com/SWE-bench/SWE-bench/releases/tag/v${version}";
    license = lib.licenses.mit;
  };
}
