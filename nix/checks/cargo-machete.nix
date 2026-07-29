# nix/checks/cargo-machete.nix
#
# `cargo-machete` — flags dependencies declared in a Cargo.toml but never used in
# that crate's source. It only parses manifests + greps source (no compile, no
# network), so this is a cheap hermetic `runCommand` (the buf.nix idiom), not a
# crane build.
#
# cargo-machete exits non-zero when it finds an unused dependency, so a regression
# fails the gate. Known false positives (a dep used only through a macro, a `cfg`,
# or an optional `dep:` feature that machete can't see) are suppressed per-crate via
# `[package.metadata.cargo-machete] ignored = [...]` — e.g. the `dhat-heap`-gated
# `dhat` in agent-embed / agent-web.
#
{
  pkgs,
  versions,
}:

pkgs.runCommand "cargo-machete-check"
  {
    nativeBuildInputs = [ versions.cargo-machete ];
    src = ../..;
  }
  ''
    cp -r $src/. ./work && chmod -R +w ./work
    cd ./work
    export HOME=$TMPDIR
    # Invoke the binary directly (no cargo needed); exits non-zero on any finding.
    cargo-machete
    echo "cargo-machete: no unused dependencies" > $out
  ''
