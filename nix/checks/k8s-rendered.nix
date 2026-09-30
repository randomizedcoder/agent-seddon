# nix/checks/k8s-rendered.nix — the committed rendered/ must equal a fresh render.
#
# Design: docs/design/k8s/04-manifests-and-gitops.md ("Gate").
#
# The hermetic twin of `nix run .#k8s-render-manifests -- --check`: editing a
# component (or bumping an image tag) without re-rendering fails the gate. Only the
# generated *.yaml is compared; hand-maintained files (e.g. the apps/README.md)
# are left alone, and an orphaned generated file that the renderer no longer emits
# is caught too.
{
  pkgs,
  lib,
  tree,
  src,
  targetNames,
}:
pkgs.runCommand "k8s-rendered"
  {
    nativeBuildInputs = [ pkgs.diffutils ];
  }
  ''
    fail=0
    for t in ${lib.concatStringsSep " " targetNames}; do
      # Every generated file must be committed and identical.
      while IFS= read -r -d "" f; do
        rel=''${f#${tree}/}
        if ! diff -u "${src}/rendered/$rel" "$f"; then
          echo "drift: rendered/$rel differs from a fresh render" >&2
          fail=1
        fi
      done < <(find "${tree}/$t" -type f -print0)

      # No committed *.yaml may linger that the renderer no longer produces.
      if [ -d "${src}/rendered/$t" ]; then
        while IFS= read -r -d "" c; do
          rel=''${c#${src}/rendered/}
          if [ ! -f "${tree}/$rel" ]; then
            echo "drift: rendered/$rel is not produced by the renderer" >&2
            fail=1
          fi
        done < <(find "${src}/rendered/$t" -type f -name '*.yaml' -print0)
      fi
    done

    if [ "$fail" -ne 0 ]; then
      echo "run 'nix run .#k8s-render-manifests' to update rendered/" >&2
      exit 1
    fi
    echo "rendered/ matches the renderer"
    touch $out
  ''
