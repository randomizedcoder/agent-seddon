# nix/openai-evals.nix
#
# Vendored, pinned build of OpenAI Evals (https://github.com/openai/evals) — the registry-
# based eval framework whose `oaieval` CLI drives `nix run .#openai-evals`. A custom
# COMPLETION FUNCTION (test/openai-evals/agent_completion_fn.py) routes each prompt through
# the REAL agent, so `oaieval` grades the whole agent, not a raw model. See docs/openai-evals.md.
#
# Pinned HERE in `nix/versions.nix` (`openaiEvalsVersion`) because nixpkgs has no `evals`
# package (it's a PyPI/GitHub project). Bump = bump the version + recompute `src.hash`.
#
# OpenAI Evals is ARCHIVED and declares a huge, messy dependency set — most of it only for
# specific eval implementations (chess/langchain/playwright/snowflake/…) that are lazily
# imported per-eval and that we never run. We provide only the CORE import-chain deps (the
# `oaieval` CLI + the basic Match/Includes graders + our completion fn) and skip the runtime-
# deps metadata check (`dontCheckRuntimeDeps`); `pythonImportsCheck` below is the real proof
# the path we use resolves. The tag's pyproject carries a static version (3.0.1.post1); the
# tag itself is the bare "3.0.1".
{
  pkgs,
  lib,
  version,
}:
let
  py = pkgs.python3Packages;
in
py.buildPythonPackage {
  pname = "openai-evals";
  inherit version;
  pyproject = true;

  src = pkgs.fetchFromGitHub {
    owner = "openai";
    repo = "evals";
    tag = version; # bare version tag, e.g. "3.0.1"
    hash = "sha256-oBoPeOdkXitjcyRBopnNeSiRFqo1FFe8eA7yIy4Hrrk=";
  };

  # No [build-system] table upstream (legacy PEP 621 + setuptools); name the backend.
  build-system = [ py.setuptools ];

  # `evals/registry.py` eagerly constructs an `OpenAI()` client at IMPORT time, which raises
  # without a non-empty key. Give the build (and thus `pythonImportsCheck`) a placeholder —
  # the client is only ever *called* by OpenAI-backed completion fns/graders, which our
  # agent-routed harness never uses. The harness sets a dummy key too.
  env.OPENAI_API_KEY = "sk-placeholder-for-import-check";

  # See the header: skip the metadata check and provide only the core-path deps.
  dontCheckRuntimeDeps = true;

  dependencies = with py; [
    openai
    tiktoken
    pyyaml
    numpy
    pandas
    tqdm
    backoff
    blobfile
    termcolor
    pydantic
    dacite
    mock
    lz4
    zstandard
    filelock
    fire
    beartype
    docker
    datasets
    jsonlines
  ];

  # Upstream's own tests are network/provider-bound; nothing to run at build time.
  doCheck = false;

  # Prove the exact path the harness uses resolves: the framework, the `oaieval` entrypoint,
  # and a basic grader (Match). A missing core dep fails HERE, not at `nix run` time.
  pythonImportsCheck = [
    "evals"
    "evals.cli.oaieval"
    "evals.elsuite.basic.match"
  ];

  meta = {
    description = "OpenAI Evals — registry-based eval framework; drives the agent-seddon `nix run .#openai-evals` harness";
    homepage = "https://github.com/openai/evals";
    changelog = "https://github.com/openai/evals/releases/tag/${version}";
    license = lib.licenses.mit;
    mainProgram = "oaieval";
  };
}
