# Cargo libnix upstream feedback

This file collects feedback for the `nixcloud/cargo` libnix author. Keep it
short, concrete, and reproducible.

## Project Profile

`agent-seddon` is a large Rust workspace with many crates, feature-scoped checks,
and a Nix flake. The current Nix build uses Crane and a pinned rust-overlay
toolchain. Local development uses normal Cargo with `sccache` and incremental
compilation disabled.

Notable build traits:

- gRPC/protobuf code generation through `tonic-build` and `prost-build`.
- `PROTOC` must point at Nix's `protobuf`.
- Rustls TLS; no OpenSSL expected for the agent.
- `rusqlite` uses bundled SQLite.
- Postgres support uses `sqlx` with Rustls and runtime-checked queries.
- Many checks run the same workspace with different features.

## Feedback Items

No upstream feedback yet. Add rows as implementation proceeds.

| ID | Type | Summary | Repro | Status |
|---|---|---|---|---|
| FB-001 | Unsupported command | libnix-1.87.0+v4 refuses `cargo check` with `cargo 'check' is not supported yet`; build and test probes work. | `CARGO_BACKEND=nix cargo check -p agent-proto` in this workspace; use `cargo build -p agent-proto` as a workaround. | Reproduced; workaround documented |
| FB-002 | Repeat build reuse | Three unchanged `cargo test -p agent-core` runs each re-evaluated the 95-unit graph and reported four derivations built; elapsed times stayed near 48s. | In `nix develop .#libnix`, run `/usr/bin/time -p cargo test -p agent-core` three times; output reports `Finished 4 built` each time. | Reproduced; investigate before performance claims |

## Candidate Questions

- What is the recommended way to pin libnix Cargo when the project uses
  rust-overlay rather than fenix?
- Should a project keep using its existing `rustc` and only replace `cargo`, or
  should it adopt the libnix release's matching Rust toolchain?
- What is the intended shape for workspaces with many feature-scoped test
  commands?
- Is generated `write-nix-buildsystem` output expected to be committed for an
  application workspace, or generated only for releases?
- Are build-script env vars like `PROTOC` best expressed in
  `Cargo.dependencies.nix`, shell env, or both?

## Report Template

```text
Project:
  agent-seddon, Rust workspace, Crane baseline, rust-overlay toolchain

libnix:
  release/tag:
  cargo version:
  rustc version:

Command:
  ...

Expected:
  ...

Actual:
  ...

Normal Cargo result:
  ...

Fix or workaround:
  ...

Performance/store notes:
  ...
```
