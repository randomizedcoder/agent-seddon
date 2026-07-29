# nix/clean.nix
#
# `nix run .#clean` — reclaim disk from the local `target/` build tree (the
# non-nix cargo world: `cargo`/`nextest`/rust-analyzer output, which crane's
# /nix/store cache does NOT share). Two modes:
#
#   nix run .#clean            # SOFT: prune target/*/incremental (the balloon)
#   nix run .#clean -- --hard  # HARD: cargo clean (full target/ reset)
#
# The dev shell exposes this as the `clean` helper and, on entry, nudges when
# `target/` grows past a threshold — so it can't silently regrow to hundreds of GB
# again. The soft prune drops only the incremental-compilation cache (the biggest
# and least valuable chunk) and keeps `deps/`, so the next build is still fast.
{
  pkgs,
  versions,
}:
pkgs.writeShellApplication {
  name = "clean";
  runtimeInputs = [
    versions.rustToolchain # `cargo clean` for --hard
    pkgs.coreutils # du
  ];
  text = ''
    hard=0
    for a in "$@"; do
      case "$a" in
        --hard) hard=1 ;;
        -h | --help)
          echo "usage: clean [--hard]"
          echo "  (default) prune target/*/incremental — keeps deps/, fast rebuild"
          echo "  --hard    cargo clean — full target/ reset"
          exit 0
          ;;
        *)
          echo "clean: unknown arg '$a' (try --hard or --help)" >&2
          exit 2
          ;;
      esac
    done

    before=$(du -sh target 2>/dev/null | cut -f1 || echo "?")

    if [ "$hard" = 1 ]; then
      echo "clean: cargo clean (full target/ reset)"
      cargo clean
      echo "clean: target/ was $before, now removed"
    else
      echo "clean: pruning incremental-compilation cache (keeping deps/)"
      rm -rf target/debug/incremental target/release/incremental 2>/dev/null || true
      after=$(du -sh target 2>/dev/null | cut -f1 || echo "?")
      echo "clean: target/ $before -> $after (run 'clean --hard' for a full reset)"
    fi
  '';
}
