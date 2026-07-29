# nix/coverage.nix
#
# `nix run .#coverage` — source-based test-coverage report for the workspace,
# generated on demand against the *working tree* (so it reflects local edits and
# drops its artifacts in your `target/`). Reporting only: no threshold, no gate —
# the `coverage` check (nix/checks/coverage.nix) keeps the instrumented path
# building in `nix flake check`, and a `--fail-under` ratchet can come later.
#
# Compiles-on-run like `nix run .#bench` (cached after the first instrumented
# build). Runs the default-feature test set — NOT `--all-features`: the
# `dhat-heap` feature installs a `#[global_allocator]` that conflicts with
# coverage instrumentation, and the non-default backends aren't the runnable set.
# Generated code is excluded from the report (proto stubs live in OUT_DIR and are
# auto-excluded; the committed @generated constants.rs, the benches, and the
# dev-only agent-testkit are regex-ignored).
#
# Examples:
#   nix run .#coverage                 # tests once → lcov.info + HTML + summary
#   nix run .#coverage -- -p agent-tools   # scope to one crate (passed through)
{
  pkgs,
  versions,
}:
pkgs.writeShellApplication {
  name = "coverage";
  runtimeInputs = [
    versions.rustToolchain
    versions.cargo-llvm-cov
    versions.protobuf
    # The test suite shells out to these, exactly as the `test` check supplies
    # them: `git` for agent-git's fixture repos, `rg` for the grep tool's fast path.
    pkgs.git
    pkgs.ripgrep
  ];
  text = ''
    export PROTOC="${versions.protobuf}/bin/protoc"

    # Files excluded from the coverage numbers: generated + non-product code.
    ignore='(/constants\.rs$|/benches/|/agent-testkit/)'

    # Collect coverage once (default features), then render three views from the
    # same run so tests aren't executed three times.
    cargo llvm-cov --workspace --no-report "$@"
    cargo llvm-cov report --ignore-filename-regex "$ignore" --lcov --output-path lcov.info
    cargo llvm-cov report --ignore-filename-regex "$ignore" --html
    echo
    cargo llvm-cov report --ignore-filename-regex "$ignore" --summary-only

    echo
    echo "coverage: lcov  -> $(pwd)/lcov.info"
    echo "coverage: html  -> $(pwd)/target/llvm-cov/html/index.html"
  '';
}
