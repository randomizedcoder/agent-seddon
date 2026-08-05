# nix/inspect-evals.nix
#
# Vendored, pinned build of inspect_evals (https://github.com/UKGovernmentBEIS/inspect_evals)
# — UK AISI's suite of 100+ standardized benchmarks (humaneval, gsm8k, gaia, its own
# swe_bench, …) built ON TOP of inspect_ai. `nix run .#inspect` can point at any of them
# via `INSPECT_TASK=inspect_evals/<name>`, running it through our agent solver. See
# docs/inspect.md.
#
# Pinned HERE by a git **commit rev** (in `nix/versions.nix`, `inspectEvalsRev`), NOT a
# version tag: inspect_evals ships from git, not versioned PyPI releases. Bump = bump the
# rev + recompute `src.hash`. `version` is a synthetic string (there's no upstream release
# number) fed to setuptools_scm, which otherwise can't derive a version from a tarball.
#
# Only benchmarks whose optional per-benchmark extra deps are present will actually run;
# agentic ones (e.g. its swe_bench) additionally need a Docker sandbox. The core install
# below is enough to load the package and run the text-graded benchmarks.
{
  pkgs,
  lib,
  version,
  rev,
  hash,
  inspect-ai,
}:
let
  py = pkgs.python3Packages;
in
py.buildPythonPackage {
  pname = "inspect-evals";
  inherit version;
  pyproject = true;

  src = pkgs.fetchFromGitHub {
    owner = "UKGovernmentBEIS";
    repo = "inspect_evals";
    inherit rev hash;
  };

  # setuptools_scm needs a PEP440-valid version; our informative nix `version` (with the
  # `-unstable-<date>` suffix) isn't, and inspect_evals has no upstream release number, so
  # feed it a fixed placeholder — the wheel's internal __version__ is unused here.
  env.SETUPTOOLS_SCM_PRETEND_VERSION = "0.1.0";

  build-system = [
    py.setuptools
    py.setuptools-scm
  ];

  # Like inspect_ai: deps are version-pinned upstream; provide the nixpkgs equivalents and
  # skip the metadata check, letting `pythonImportsCheck` be the real proof.
  dontCheckRuntimeDeps = true;

  dependencies =
    (with py; [
      backoff
      datasets
      huggingface-hub
      hf-xet
      jinja2
      numpy
      pillow
      pydantic
      pyyaml
      requests
      tiktoken
      toml
    ])
    ++ [ inspect-ai ];

  doCheck = false;

  pythonImportsCheck = [ "inspect_evals" ];

  meta = {
    description = "inspect_evals — UK AISI's standardized benchmark suite for Inspect AI";
    homepage = "https://github.com/UKGovernmentBEIS/inspect_evals";
    license = lib.licenses.mit;
  };
}
