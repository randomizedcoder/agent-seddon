# nix/libnix-cargo.nix
#
# Opt-in cargo(libnix) toolchain experiment. This deliberately does NOT replace
# the repo's normal rust-overlay latest toolchain or Crane package/check path.
# cargo(libnix) currently requires cargo and rustc from the same Rust release; the
# upstream v4 release is based on Rust 1.87.0 / Cargo 0.88.0.
{ pkgs, lib }:

let
  version = "1.87.0+v4";

  # Generated libnix build system for the cargo fork.
  buildsystem = builtins.fetchTarball {
    url = "https://github.com/nixcloud/cargo/releases/download/libnix-1.87.0%2Bv4/libnix-1.87.0+v4.tar.bz2";
    sha256 = "sha256:0zhj8sl05g861c7pkwgd1ij26mf68qj1flbad2ivwj113vac9chq";
  };

  # Cargo source tree that the generated build system compiles.
  cargoSrc = builtins.fetchTarball {
    url = "https://github.com/nixcloud/cargo/archive/refs/tags/libnix-1.87.0%2Bv4.tar.gz";
    sha256 = "sha256:0lm951801chcdnjni4068jjwlzyf94byjgjk5qzlr9vihsp5i3ym";
  };

  rustToolchain = pkgs.rust-bin.stable."1.87.0".default.override {
    extensions = [
      "clippy"
      "rustfmt"
      "rust-src"
    ];
  };

  cargo =
    (import (buildsystem + "/derivations/default.nix") {
      inherit pkgs;
      project_root = cargoSrc;
      external_crate_dependencies = import (buildsystem + "/Cargo.dependencies.nix") { inherit pkgs; };
      rustc = rustToolchain;
      cargo = rustToolchain;
    }).cargo-0_88_0-bin-8cad88ffd54a16db;
in
{
  inherit
    version
    buildsystem
    cargoSrc
    rustToolchain
    cargo
    ;

  nix-prefetch = pkgs.nix-prefetch-scripts or pkgs.nix-prefetch-git;

  meta = {
    description = "Experimental cargo(libnix) backend pinned for agent-seddon evaluation";
    upstream = "https://github.com/nixcloud/cargo/tree/master-libnix";
    platforms = lib.platforms.linux;
  };
}
