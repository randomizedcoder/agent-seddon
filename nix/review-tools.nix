# nix/review-tools.nix
#
# The static-analysis tool suite the code-review pipeline shells out to, bundled as
# ONE `symlinkJoin` so a single `nix flake update nixpkgs` (floating the pins in
# nix/versions.nix) bumps every tool together, reproducibly. This bundle is wired
# onto the packaged agent's runtime PATH (nix/default.nix, `agentRuntimePath`) so a
# `--serve-fleet` service / container carries the whole suite without relying on an
# ambient environment, and is exposed as `packages.review-toolbox` for inspection
# and dev/CI parity.
#
# Versions come from nix/versions.nix (the SSOT) — never `pkgs.<tool>` directly —
# so there is exactly one place a version is chosen. `go` provides `go`, `gofmt`,
# and `go vet`; `golangci-lint` needs a Go toolchain on PATH to run, which this
# bundle now guarantees for the fleet (previously only ambient). `cargo-audit`
# (RustSec advisories) and `cargo-deny` (banned/duplicate deps + sources) are the
# Rust supply-chain half of the analyzer's suite (review-analysis-depth Inc 5a),
# run offline against the pinned advisory-db (see nix/default.nix, AGENT_ADVISORY_DB).
{
  pkgs,
  versions,
}:
pkgs.symlinkJoin {
  name = "agent-review-toolbox";
  paths = [
    versions.go # `go` + `gofmt` + `go vet`
    versions.golangci-lint
    versions.gosec
    versions.cargo-audit
    versions.cargo-deny
  ];
}
