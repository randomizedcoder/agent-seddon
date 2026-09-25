# nix/checks/sandbox-bwrap.nix
#
# Executes the `BwrapSandbox` backend's tests, which sit behind the non-default
# `sandbox-bwrap` cargo feature (Tier-1 rootless-namespace isolation, multi-tenancy
# C23). The main `test` check runs *default* features (it must not enable the opt-in
# backend), and clippy `--all-features` compiles + lints this code but does not run it.
# This dedicated, feature-scoped check is what actually EXECUTES the bwrap tests in the
# gate — the pure `bwrap_argv` flag-assembly (network / read-only overlay C23-3a /
# seccomp C23-3b), the `systemd_scope_argv` resource-cap assembly (C23-2), the
# fail-closed setup-error classifier, and the seccomp-BPF profile compiler + memfd
# staging (C23-3b, `crates/agent-sandbox/src/seccomp.rs`).
#
# The `#[tokio::test]` LIVE pillar tests (real bwrap exec) skip cleanly here: the
# hermetic builder permits neither unprivileged user namespaces nor `bwrap` on PATH,
# so `bwrap_usable()` returns false and they return early. `seccompiler` + `libc` are
# pure Rust; crane vendors them from Cargo.lock — no system libseccomp, no extra input.
{
  craneLib,
  commonArgs,
  cargoArtifacts,
}:

craneLib.cargoTest (
  commonArgs
  // {
    inherit cargoArtifacts;
    cargoTestExtraArgs = "-p agent-sandbox --features sandbox-bwrap";
  }
)
