# Cargo libnix experiment

This track evaluates the `nixcloud/cargo` `master-libnix` branch as an opt-in
Cargo backend for `agent-seddon`.

Goal: learn whether Cargo's libnix backend can reduce duplicate Rust build work,
improve Nix-store reuse, or make local and CI builds faster than the current
Crane + `sccache` setup, without destabilizing the existing `nix build .#agent`
and `nix flake check` path.

External reference: <https://github.com/nixcloud/cargo/tree/master-libnix>.

## Current build shape

The repo already has a strong Nix/Rust build:

- `flake.nix` is a thin orchestrator and imports `./nix`.
- `nix/default.nix` binds Crane to the pinned rust-overlay toolchain.
- `cleanedSrc` preserves the non-Rust files Cargo needs: proto, SQL, test
  fixtures, generated OpenAPI fixtures, graph assets, and deny config.
- `commonArgs` provides `pkg-config`, `protobuf`, and `PROTOC`.
- `cargoArtifacts = craneLib.buildDepsOnly commonArgs` is reused by `agent`,
  clippy, tests, and feature-scoped checks.
- `nix/devshell.nix` uses `sccache` and disables incremental compilation to keep
  `target/` bounded.

That means libnix is not being evaluated as a rescue from a bad build. It is
being evaluated as a possible improvement over an already deliberate setup.

## Hypothesis

Libnix may help most where this repo currently repeats first-party Cargo work
across many feature and command variants:

- `cargo test` for default features
- `cargo clippy --all-targets --all-features`
- many feature-scoped `cargo test -p ... --features ...` checks
- `cargo check` rows in the lean feature matrix
- local edit/build loops that currently rely on `sccache` outside Nix-store
  reuse

The likely win is finer-grained artifact reuse across Cargo units and feature
sets. The likely risk is toolchain drift or rough edges around build scripts.

## Integration Principles

1. Keep the Crane path authoritative until libnix has repeated measurements.
2. Add libnix as an opt-in development and evaluation surface.
3. Do not change `packages.agent`, `checks.*`, or the default dev shell first.
4. Pin libnix by tag or release artifact, not by a moving branch.
5. Keep all libnix-specific dependency and environment injection explicit.
6. Capture failures as feedback, not as local folklore.

## Proposed Opt-In Surface

Initial files:

- `nix/libnix-cargo.nix`: imports the pinned libnix Cargo release.
- `Cargo.dependencies.nix`: per-crate native inputs and env vars needed by
  libnix builds.
- `devShells.libnix`: same base developer tools as `devShells.default`, but with
  libnix Cargo earlier on `PATH` and `CARGO_BACKEND=nix`.
- `apps.libnix-smoke`: runs the smallest useful build/test probe.
- `checks.libnix-smoke`: optional, non-default at first if evaluation cost is
  high; promoted only after it proves stable.

The default `nix develop` remains unchanged until the experiment shows a clear
benefit and a low maintenance cost.

## Expected First Wiring

The first shell should make this possible:

```sh
nix develop .#libnix
which cargo
echo "$CARGO_BACKEND"
cargo check -p agent-proto
cargo test -p agent-core
cargo check -p agent-runtime --no-default-features
```

The pinned libnix v4 release currently rejects `cargo check`. For the libnix
shell, use `cargo build -p agent-proto` and
`cargo build -p agent-runtime --no-default-features`; normal Cargo and Crane
measurements can continue to use the `check` commands above.

`agent-proto` is intentionally first because it exercises the important build
script path: `tonic-build` must find `protoc` via `PROTOC`, just like the current
Crane build.

## Dependency Injection Notes

The workspace is intentionally light on native system dependencies:

- TLS is Rustls, not OpenSSL.
- Postgres uses `sqlx` with Rustls and runtime-checked queries, not `libpq`.
- SQLite uses `rusqlite` with bundled SQLite.
- `agent-proto` needs `protoc`.

The initial `Cargo.dependencies.nix` should probably be small:

```nix
{ pkgs }:
{
  deps = {
    "agent-proto" = [ pkgs.protobuf ];
  };
  envs = {
    "agent-proto" = {
      PROTOC = "${pkgs.protobuf}/bin/protoc";
    };
  };
}
```

This is a starting point, not a design contract. Update it only in response to
observed libnix failures.

## Risks

| Risk | Why it matters | Mitigation |
|---|---|---|
| Toolchain mismatch | `agent-seddon` tracks `stable.latest`; libnix releases may lag a Rust release | Pin both explicitly for the experiment and record versions in every result |
| Build-script behavior | `agent-proto`, `ring`, `libsqlite3-sys`, and `zstd-sys` exercise build scripts | Start with targeted probes and record every env/dependency fix |
| Store growth | Running Crane and libnix side by side may duplicate artifacts | Measure closure/store deltas before promotion |
| Check semantics drift | The gate has many feature-specific checks with deliberate inputs | Keep Crane checks authoritative until libnix reproduces a subset exactly |
| Dirty-tree noise | This repo often has active local changes | Record commit, dirty state, and source filter assumptions for each run |

## Promotion Gates

Libnix can move from experiment to supported opt-in when all of this is true:

- `nix develop .#libnix` enters cleanly.
- `cargo check -p agent-proto` works with hermetic `PROTOC`.
- `cargo test -p agent-core` works.
- `cargo check -p agent-runtime --no-default-features` works.
- At least one feature-scoped test check works.
- Results show either faster warm builds, lower duplicated store usage, or a
  clearly better developer workflow.
- Failures and required `Cargo.dependencies.nix` entries are documented well
  enough to send upstream.

## Track Files

- [`STATUS.md`](STATUS.md) records implementation progress and decisions.
- [`EVALUATION.md`](EVALUATION.md) defines the performance and store-use
  comparison protocol.
- [`FEEDBACK.md`](FEEDBACK.md) collects concise upstream feedback for the libnix
  author.
