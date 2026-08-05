# nix/inspect-ai.nix
#
# Vendored, pinned build of Inspect AI (https://inspect.aisi.org.uk/) — UK AISI's
# LLM/agent evaluation framework. Its CLI (`inspect eval …`) drives `nix run .#inspect`,
# where a custom SOLVER (test/inspect/agent_solver.py) shells out to the REAL agent so
# Inspect grades the whole agent, not a raw model. See docs/inspect.md.
#
# We keep our OWN derivation (nixpkgs has no `inspect-ai`; it's a PyPI/GitHub project) so
# the version is pinned HERE, in `nix/versions.nix` (`inspectAiVersion`), tracking upstream
# directly. This is a `buildPythonPackage` LIBRARY (+ an `inspect` console script) so the
# harness pulls it into a `python3.withPackages`. Bump = bump `inspectAiVersion` and
# recompute `src.hash` (set it to `lib.fakeHash`, build, copy the "got:" line).
#
# Two upstream quirks handled here:
#   * inspect_ai versions with setuptools_scm from git metadata, which a GitHub *tarball*
#     lacks — so we pin the version explicitly via `SETUPTOOLS_SCM_PRETEND_VERSION`.
#   * its `requirements.txt` names two deps missing/too-old in nixpkgs — `nest_asyncio2`
#     (absent) and `agent-client-protocol >= 0.12` (nixpkgs has 0.11.1). Both are tiny and
#     vendored as `let` bindings below. `zipfile-zstd` / `exceptiongroup` are only required
#     on Python < 3.14 / < 3.11 respectively, and our python3 is 3.14, so they drop out.
{
  pkgs,
  lib,
  version,
}:
let
  py = pkgs.python3Packages;

  # nest_asyncio2 — a zero-dependency pure-python fork of nest_asyncio; not in nixpkgs.
  nest-asyncio2 = py.buildPythonPackage {
    pname = "nest-asyncio2";
    version = "1.7.2";
    pyproject = true;
    src = py.fetchPypi {
      pname = "nest_asyncio2";
      version = "1.7.2";
      hash = "sha256-GSHXC5LMRhLDdJKNCBVS77Wbg9kbK3idk1xmX6AXKag=";
    };
    env.SETUPTOOLS_SCM_PRETEND_VERSION = "1.7.2"; # sdist has no git metadata for setuptools_scm
    build-system = [
      py.setuptools
      py.setuptools-scm
    ];
    doCheck = false;
  };

  # agent-client-protocol — nixpkgs pins 0.11.1; inspect_ai wants >= 0.12.0. A pure
  # version+src bump of the packaged derivation (its runtime dep, pydantic, is unchanged).
  agent-client-protocol = py.agent-client-protocol.overridePythonAttrs (_old: {
    version = "0.12.0";
    src = py.fetchPypi {
      pname = "agent_client_protocol";
      version = "0.12.0";
      hash = "sha256-TUKsaAOIVWsDjLbdWNu70VYfnlRaM0waNSzAVfmLHHk=";
    };
    # 0.12's test suite imports test-only extras (uvicorn, …) the packaged derivation
    # doesn't carry; we only need the library importable, so skip its checks.
    doCheck = false;
  });
in
py.buildPythonPackage {
  pname = "inspect-ai";
  inherit version;
  pyproject = true;

  src = pkgs.fetchFromGitHub {
    owner = "UKGovernmentBEIS";
    repo = "inspect_ai";
    tag = version; # tags are the bare version, e.g. "0.3.252" (no "v" prefix)
    hash = "sha256-u+lcBikKP/J5mhlVr8iD48Csn3YkqGcRSvX7NPY93W4=";
  };

  # A GitHub tarball has no git metadata, so setuptools_scm can't derive the version;
  # tell it what we're building (matches the pinned tag).
  env.SETUPTOOLS_SCM_PRETEND_VERSION = version;

  build-system = [
    py.setuptools
    py.setuptools-scm
  ];

  # inspect_ai declares its deps dynamically (from requirements.txt) with hard version
  # pins on many; we provide the nixpkgs equivalents and skip the runtime-deps metadata
  # check (`dontCheckRuntimeDeps`) rather than chase every `>=` pin — `pythonImportsCheck`
  # below is the real proof the import graph resolves.
  dontCheckRuntimeDeps = true;

  dependencies =
    (with py; [
      aioboto3
      anyio
      beautifulsoup4
      boto3
      click
      debugpy
      docstring-parser
      fastapi
      fsspec
      httpx
      ijson
      jsonlines
      jsonpatch
      jsonpath-ng
      jsonref
      jsonschema
      mmh3
      numpy
      platformdirs
      psutil
      pydantic
      python-dotenv
      pyyaml
      rich
      s3fs
      semver
      shortuuid
      sniffio
      tenacity
      textual
      tiktoken
      typing-extensions
      universal-pathlib
      uvicorn
      zipp
      zstandard
    ])
    ++ [
      nest-asyncio2
      agent-client-protocol
    ];

  # Upstream's own test-suite needs network + provider keys; nothing to run at build time.
  doCheck = false;

  # Cheap build-time proof the framework's import graph resolves under our deps — a missing
  # or broken dep fails HERE, not at `nix run .#inspect` time.
  pythonImportsCheck = [ "inspect_ai" ];

  meta = {
    description = "Inspect AI — UK AISI's LLM/agent eval framework; drives the agent-seddon `nix run .#inspect` harness";
    homepage = "https://inspect.aisi.org.uk/";
    changelog = "https://github.com/UKGovernmentBEIS/inspect_ai/releases/tag/${version}";
    license = lib.licenses.mit;
    mainProgram = "inspect";
  };
}
