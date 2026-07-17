# nix/checks/nix-fmt.nix
#
# `nixfmt --check` over every .nix file — fails on any unformatted file.
#
{ pkgs, versions }:

pkgs.runCommand "nix-fmt-check"
  {
    nativeBuildInputs = [ versions.nixfmt ];
    src = ../..;
  }
  ''
    cp -r $src/. ./work && chmod -R +w ./work
    cd ./work
    unformatted=$(find . -name '*.nix' -print0 | xargs -0 nixfmt --check 2>&1 || true)
    if [ -n "$unformatted" ]; then
      echo "nixfmt: the following files are not formatted:" >&2
      echo "$unformatted" >&2
      exit 1
    fi
    echo "nixfmt: all .nix files formatted" > $out
  ''
