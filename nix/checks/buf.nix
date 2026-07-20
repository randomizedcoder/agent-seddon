# nix/checks/buf.nix
#
# Proto governance gate: `buf lint` (style/consistency, per buf.yaml) + `buf
# breaking` (wire-compat) over the `.proto` contracts. Codegen stays on
# `tonic-build`; buf only lints + guards compatibility.
#
# `buf breaking` needs a baseline, but `nix flake check` is stateless and can't
# reach git history — so we gate against the committed image
# `crates/agent-proto/buf.image.binpb` (the accepted wire contract). Additive
# changes (a new RPC/field) pass without touching it; a wire-incompatible edit
# fails until the baseline is deliberately moved with `nix run .#buf-image`.
{ pkgs, versions }:

pkgs.runCommand "buf-check"
  {
    nativeBuildInputs = [ versions.buf ];
    src = ../..;
  }
  ''
    cp -r $src/. ./work && chmod -R +w ./work
    cd ./work
    export HOME=$TMPDIR # buf writes a cache under $HOME

    echo "buf lint…"
    buf lint

    if [ ! -f crates/agent-proto/buf.image.binpb ]; then
      echo "missing crates/agent-proto/buf.image.binpb — run: nix run .#buf-image" >&2
      exit 1
    fi
    echo "buf breaking (vs committed image)…"
    buf breaking --against crates/agent-proto/buf.image.binpb

    echo "buf: lint + breaking clean" > $out
  ''
