# nix/promptfoo.nix
#
# Vendored, pinned build of promptfoo (https://www.promptfoo.dev/) — the LLM eval +
# red-team harness that drives the agent in `nix run .#eval` / `.#redteam`.
#
# We keep our OWN derivation (rather than `pkgs.promptfoo`) so the version is pinned
# HERE, in `nix/versions.nix` (`promptfooVersion`), and can track the latest release
# independently of nixpkgs' lag. It is a straight `buildNpmPackage` mirroring the
# nixpkgs definition; bumping the pin means bumping `promptfooVersion` and recomputing
# the two fixed-output hashes below (set a hash to `lib.fakeHash`, build, copy the
# "got:" line — src.hash first, then npmDepsHash).
#
# promptfoo is NOT a Nix flake (no flake.nix upstream), so it cannot be a flake input;
# a `buildNpmPackage` derivation is the supported path.
{
  pkgs,
  lib,
  version,
}:
pkgs.buildNpmPackage (finalAttrs: {
  pname = "promptfoo";
  inherit version;

  src = pkgs.fetchFromGitHub {
    owner = "promptfoo";
    repo = "promptfoo";
    tag = finalAttrs.version;
    hash = "sha256-CMIN0zDbNC1VWeAFMJ9B4+9Vuft/oQ15GDS+eOuldDQ=";
  };

  npmDepsHash = "sha256-sAgddC0kqFCK7X4zCq5Dfk08qnByjfJyKhzALyQm5BQ=";

  # 0.122.0 pulls transitive cloud SDKs (`mongodb`, azure/gcp, `onnxruntime-node`) whose
  # dependency install scripts break the offline sandbox, so we skip all dependency
  # lifecycle scripts. Both flags are hash-stable (install phase, not the fixed-output
  # deps fetch):
  #   * `--legacy-peer-deps`: a `peerOptional` `gcp-metadata` override npm otherwise
  #     tries to re-resolve at install (ENOTCACHED — not in the locked set).
  #   * `--ignore-scripts`: dependency postinstalls reach out to `api.nuget.org`
  #     (EAI_AGAIN, e.g. `onnxruntime-node`) for optional .NET/native binaries we don't
  #     use. The native sqlite bindings still load.
  npmFlags = [
    "--legacy-peer-deps"
    "--ignore-scripts"
  ];

  # `--ignore-scripts` (above) also suppresses the `build` script's own `postbuild`
  # hook, which copies non-TS runtime assets into `dist/` — HTML templates, the
  # python/ruby/golang custom-provider wrappers, and the **drizzle DB migrations**
  # (`dist/drizzle`; without them the CLI dies "Can't find meta/_journal.json"). Run it
  # explicitly here (a pure offline `tsx` fs-copy) so the packaged CLI is complete.
  postBuild = ''
    npm run postbuild
  '';

  # https://github.com/NixOS/nixpkgs/issues/474535 — build under Node 22.
  nodejs = pkgs.nodejs_22;

  # Don't fetch the Playwright browser (a large, network-bound, unused-at-eval-time
  # download); promptfoo only needs it for browser-provider scenarios we don't use.
  env.PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD = "1";

  # The repo is an npm workspace (app/, docs/); after install these dangling
  # workspace symlinks would break `fixupPhase`. Mirror nixpkgs' cleanup.
  preFixup = ''
    rm -rf \
      $out/lib/node_modules/promptfoo/node_modules/app \
      $out/lib/node_modules/promptfoo/node_modules/promptfoo-docs
  '';

  meta = {
    description = "Test, evaluate and red-team LLM apps — drives the agent-seddon eval/redteam harnesses";
    mainProgram = "promptfoo";
    homepage = "https://www.promptfoo.dev/";
    changelog = "https://github.com/promptfoo/promptfoo/releases/tag/${finalAttrs.version}";
    license = lib.licenses.mit;
  };
})
