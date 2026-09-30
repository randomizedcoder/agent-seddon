# nix/checks/k8s-image-smoke.nix
#
# Gate for the k8s agent image (k8s track K2, docs/design/k8s/03-images-and-registry.md).
#
# Streams the REAL artifact `nix run .#k8s-images` ships (`streamLayeredImage`'s
# docker-archive), unpacks it, and asserts the security posture the manifests rely
# on:
#   - the config runs as a non-root user (uid 10001, not 0);
#   - the entrypoint is the agent binary (`/bin/agent`), not a shell;
#   - no shell sits on a guessable path (`/bin/sh`, `/bin/bash`, `/usr/bin/*`), so a
#     tool with an exec seam can't drop into one. (The wrapped agent still carries a
#     bash at its own store path — makeWrapper's launcher — which is not on PATH; the
#     check is about the image's `/bin`, matching the design's "no shell present".)
#   - the archive is tagged `agent-seddon/agent:<hash>` (content-addressed tag).
# It then runs the binary out of the unpacked image root with `--help` (the agent
# CLI has no `--version`; `--help` exits 0, reads no config and calls nothing), so a
# broken image — wrong interpreter, missing loader, unruntime binary — fails here.
#
# Offline/hermetic: no registry, no daemon; `streamLayeredImage` writes a tar to
# stdout and we read it.
{
  pkgs,
  lib,
  versions,
  agent,
}:
let
  inherit
    (import ../k8s/images.nix {
      inherit
        pkgs
        lib
        versions
        agent
        ;
    })
    images
    ;
in
pkgs.runCommand "k8s-image-smoke"
  {
    nativeBuildInputs = [
      pkgs.coreutils
      pkgs.gnutar
      pkgs.gzip
      pkgs.jq
    ];
  }
  ''
    set -euo pipefail
    export HOME="$(mktemp -d)"

    # Stream the docker-archive and unpack it.
    ${images.agent} > image.tar
    mkdir unpacked
    tar -xf image.tar -C unpacked

    manifest=unpacked/manifest.json
    test -f "$manifest" || { echo "FAIL: no manifest.json in the archive" >&2; exit 1; }

    repotag="$(jq -r '.[0].RepoTags[0]' "$manifest")"
    echo "RepoTag: $repotag"
    case "$repotag" in
      agent-seddon/agent:*) : ;;
      *) echo "FAIL: image must be tagged agent-seddon/agent:<tag>, got '$repotag'" >&2; exit 1 ;;
    esac

    cfg="unpacked/$(jq -r '.[0].Config' "$manifest")"
    test -f "$cfg" || { echo "FAIL: config JSON '$cfg' missing" >&2; exit 1; }

    user="$(jq -r '.config.User' "$cfg")"
    echo "User: $user"
    if [ "$user" != "10001:10001" ]; then
      echo "FAIL: image must run as non-root uid 10001, got User='$user'" >&2
      exit 1
    fi

    entrypoint="$(jq -c '.config.Entrypoint' "$cfg")"
    echo "Entrypoint: $entrypoint"
    if [ "$entrypoint" != '["/bin/agent"]' ]; then
      echo "FAIL: entrypoint must be the agent binary, got $entrypoint" >&2
      exit 1
    fi

    # Merge every layer into one rootfs so path lookups see the final image.
    mkdir root
    for layer in $(jq -r '.[0].Layers[]' "$manifest"); do
      tar -xf "unpacked/$layer" -C root 2>/dev/null || true
    done

    # No shell on a guessable path. (-e follows symlinks; -L catches a dangling one.)
    for s in bin/sh bin/bash usr/bin/sh usr/bin/bash; do
      if [ -e "root/$s" ] || [ -L "root/$s" ]; then
        echo "FAIL: image exposes a shell at /$s" >&2
        exit 1
      fi
    done

    # The entrypoint must actually run out of the image root.
    if ! help="$(root/bin/agent --help)"; then
      echo "FAIL: /bin/agent --help must exit 0 from the image root" >&2
      exit 1
    fi
    case "$help" in
      *"--serve-<seam>"*) : ;;
      *) echo "FAIL: /bin/agent --help did not render the expected usage" >&2; exit 1 ;;
    esac

    echo "OK: agent image is non-root, shell-free on PATH, and runs $repotag" > "$out"
  ''
