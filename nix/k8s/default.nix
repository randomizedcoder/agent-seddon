# nix/k8s/default.nix — the manifest renderer (k8s track K3).
#
# Design: docs/design/k8s/04-manifests-and-gitops.md.
#
# Assembles each component's structured manifests for a target, serialises them to
# YAML with `k8sLib.toYAML`, and exposes:
#   - `tree`               a hermetic derivation of the whole rendered/ tree, for
#                          the `k8s-rendered` drift check;
#   - `render-manifests`   the `nix run .#k8s-render-manifests` app that rewrites the
#                          committed rendered/ (or, with `--check`, exits 1 on drift).
#
# The committed rendered/ is the source of truth ArgoCD applies; this renderer is
# what keeps it honest. The image tags come from the separately-committed
# `image-tags.nix` (written only by `nix run .#k8s-images`), so an unrelated Rust
# PR never churns this tree.
{
  pkgs,
  lib,
  constants,
  imageTags,
}:
let
  k8sLib = import ./lib.nix { inherit lib; };

  # Every deployment target. k8s (full HA) arrives at K9.
  targets = {
    k3s = import ./targets/k3s.nix;
  };

  # Every component function, in wave order. pki (CA chain + Certificates) lands in
  # a later K3 slice; edge/portal-web are K6.
  components = [
    ./components/gateway.nix
    ./components/sessions.nix
    ./components/fleet.nix
  ];

  # One target → its flat, name-sorted manifest list [{ name; content; }].
  manifestsFor =
    target:
    let
      all = lib.concatMap (
        c:
        (import c {
          inherit
            lib
            k8sLib
            constants
            target
            imageTags
            ;
        }).manifests
      ) components;
    in
    lib.sort (a: b: a.name < b.name) all;

  # A store file per manifest, named by its slash-free path so writeText is happy.
  manifestFile = m: pkgs.writeText (lib.replaceStrings [ "/" ] [ "_" ] m.name) m.content;

  # The whole rendered tree as one derivation: $out/<target>/<name> for every
  # manifest of every target. The drift check and the render app both consume this.
  tree = pkgs.runCommand "k8s-rendered" { } (
    lib.concatStrings (
      lib.mapAttrsToList (
        tname: target:
        lib.concatMapStrings (m: ''
          mkdir -p "$out/${tname}/$(dirname ${lib.escapeShellArg m.name})"
          cp ${manifestFile m} "$out/${tname}/${m.name}"
        '') (manifestsFor target)
      ) targets
    )
  );

  # Managed target dirs, so the app/check touch only what this renderer owns and
  # leave hand-maintained files (e.g. rendered/k3s/apps/README.md) alone.
  targetNames = lib.attrNames targets;

  render-manifests = pkgs.writeShellApplication {
    name = "k8s-render-manifests";
    runtimeInputs = [ pkgs.diffutils ];
    text = ''
      tree=${tree}
      targets=(${lib.concatStringsSep " " targetNames})

      if [ "''${1:-}" = "--check" ]; then
        rc=0
        for t in "''${targets[@]}"; do
          # Every generated file must exist committed and match.
          while IFS= read -r -d "" f; do
            rel=''${f#"$tree"/}
            if ! diff -u "rendered/$rel" "$f" >/dev/null 2>&1; then
              echo "drift: rendered/$rel differs from a fresh render" >&2
              rc=1
            fi
          done < <(find "$tree/$t" -type f -print0)
          # No committed *.yaml may linger that the renderer no longer emits.
          if [ -d "rendered/$t" ]; then
            while IFS= read -r -d "" c; do
              rel=''${c#rendered/}
              if [ ! -f "$tree/$rel" ]; then
                echo "drift: rendered/$rel is not produced by the renderer" >&2
                rc=1
              fi
            done < <(find "rendered/$t" -type f -name '*.yaml' -print0)
          fi
        done
        if [ "$rc" -ne 0 ]; then
          echo "run 'nix run .#k8s-render-manifests' to update rendered/" >&2
          exit 1
        fi
        echo "rendered/ is up to date"
        exit 0
      fi

      # Rewrite: drop the generated *.yaml (keeping README.md etc.), then copy fresh.
      for t in "''${targets[@]}"; do
        if [ -d "rendered/$t" ]; then
          find "rendered/$t" -type f -name '*.yaml' -delete
        fi
      done
      cp -rT "$tree" rendered
      chmod -R u+w rendered
      echo "wrote rendered/ for: ''${targets[*]}"
    '';
  };
in
{
  inherit tree render-manifests targetNames;
  # Exposed so a check can rebuild the exact same tree.
  inherit manifestsFor targets;
}
