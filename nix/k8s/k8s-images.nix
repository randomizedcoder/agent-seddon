# nix/k8s/k8s-images.nix — the `nix run .#k8s-images` release step (k8s track K2).
#
# Design: docs/design/k8s/03-images-and-registry.md ("Getting images onto nodes").
#
# Builds the agent image stream once (baked in below) and, per the flag:
#   (default)          rewrite the committed nix/k8s/image-tags.nix with the current
#                      content-addressed tag — the release "cut" the renderer (K3) reads;
#   --import           pipe the stream into `k3s ctr images import -` on this host
#                      (k3s target), then rewrite the tags;
#   --push <registry>  `skopeo copy` the stream to <registry>/agent-seddon/agent:<tag>
#                      (full-k8s target, K9), then rewrite the tags.
#
# The tag is baked at build time (`tags.agent`), so `--import`/`--push` and the tag
# written to image-tags.nix are always the SAME image. The stream never lands a
# multi-GB tarball in the store; it goes straight to the importer over a pipe.
{
  pkgs,
  lib,
  images,
  tags,
}:
pkgs.writeShellApplication {
  name = "k8s-images";
  runtimeInputs = [
    pkgs.coreutils
    pkgs.gnused
    pkgs.git
    pkgs.skopeo
  ];
  text = ''
    stream=${lib.escapeShellArg "${images.agent}"}
    tag=${lib.escapeShellArg tags.agent}

    action="cut"
    registry=""
    while [ "$#" -gt 0 ]; do
      case "$1" in
        --import) action="import"; shift ;;
        --push)
          action="push"
          registry="''${2:-}"
          if [ -z "$registry" ]; then
            echo "usage: k8s-images --push <registry>" >&2
            exit 2
          fi
          shift 2 ;;
        -h|--help)
          echo "usage: k8s-images [--import | --push <registry>]"
          echo "  (default)          rewrite nix/k8s/image-tags.nix with the current tag"
          echo "  --import           k3s ctr images import - on this host, then rewrite tags"
          echo "  --push <registry>  skopeo copy to <registry>/agent-seddon/agent:<tag>, then rewrite tags"
          exit 0 ;;
        *) echo "k8s-images: unknown argument '$1'" >&2; exit 2 ;;
      esac
    done

    case "$action" in
      import)
        echo "importing agent-seddon/agent:$tag into k3s (k8s.io namespace)…" >&2
        "$stream" | sudo k3s ctr -n k8s.io images import - ;;
      push)
        ref="docker://$registry/agent-seddon/agent:$tag"
        echo "pushing $ref…" >&2
        "$stream" | skopeo copy docker-archive:/dev/stdin "$ref" ;;
    esac

    # Rewrite the committed tags (the release cut). Locate the repo from the git
    # toplevel so the command works from any subdir.
    root="$(git rev-parse --show-toplevel)"
    dest="$root/nix/k8s/image-tags.nix"
    {
      # The literal backticks below are file content, not a command substitution.
      # shellcheck disable=SC2016
      printf '%s\n' \
        '# nix/k8s/image-tags.nix — committed content-addressed image tags (k8s track K2).' \
        '#' \
        '# WRITTEN BY `nix run .#k8s-images`; do not edit by hand. The renderer (K3) reads' \
        '# these so an unrelated Rust PR never changes rendered/ and fails the drift check' \
        '# (docs/design/k8s/03-images-and-registry.md).' \
        '{' \
        "  agent = \"$tag\";" \
        '}'
    } > "$dest"
    echo "wrote $dest (agent = $tag)" >&2
  '';
}
