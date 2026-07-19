# CLAUDE.md

Guidance for Claude Code (and other agents) working in this repository.

## Environment: always use `nix develop`

This repo is built with a **pinned, hermetic toolchain** defined in `flake.nix`
(Rust toolchain via `rust-overlay`, `protoc` for the gRPC codegen, and the dev
tools). **Do not rely on an ambient `cargo`/`rustc`/`clippy` on `PATH`** — the
versions there may not match what the project needs (e.g. `tantivy` requires a
recent rustc, and `tonic-build` needs `protoc`), and a mismatched `clippy` will
fail to resolve dependencies.

Enter the dev shell first, or prefix commands with `nix develop -c`:

```sh
nix develop                     # interactive dev shell (correct toolchain + tools)

# …or run a single command in the pinned environment:
nix develop -c cargo build
nix develop -c cargo test
nix develop -c cargo clippy --all-targets -- -D warnings
```

The single source of truth for versions/ports/pins is `nix/versions.nix` (+
`nix/constants.nix`, which generates `crates/agent-grpc/src/constants.rs`).

## Common commands (run inside `nix develop`)

```sh
cargo build                     # default features: both providers, all tools, search, memory, grpc
cargo test                      # workspace tests (table-driven, rstest)
cargo test -p agent-search      # one crate
nix flake check                 # clippy -D warnings + rustfmt + tests + cargo-audit + nix-fmt + constants-sync
nix fmt                         # format all .nix files
nix build .#agent               # build the `agent` binary -> ./result/bin/agent
nix run   .#gen-constants       # regenerate constants.rs after editing nix/constants.nix
```

`nix flake check` is the gate: it runs `clippy` with `-D warnings`, so the tree
must be warning-clean. When in doubt, run it before considering work done.

## Architecture (see DESIGN.md + docs/)

- Every replaceable component is an `async` trait in `agent-core` (a **seam**):
  `LlmProvider`, `Tool`, `MemoryStore`, `ContextStrategy`, `Policy`,
  `SearchBackend`.
- Concrete impls live in sibling crates, gated by **cargo features**, and are
  wired at runtime by the **plugin registry** (`agent-runtime/src/registry.rs`,
  `register_builtins`) which maps a config string → factory. Config selects impls
  (`config/agent.toml`); no code edits to swap a backend.
- Any seam can run as its own gRPC service (`agent --serve-<seam>`) and be dialed
  by a `= "grpc"` client. Ports/sockets come from `nix/constants.nix`.
- Cross-cutting: Prometheus metrics (`agent-metrics` + `metered.rs` decorators)
  and OpenTelemetry tracing (`agent-telemetry`).

Per-component docs live in `docs/components/*.md`; add/adjust the matching doc
when you change a component.

## Conventions

- **Tests are table-driven** with `rstest` `#[case]`; shared test doubles +
  `tempdir()` live in `agent-testkit`. Match the existing style (see
  `crates/agent-grpc/tests/roundtrip.rs`, `crates/agent-tools/src/search.rs`).
- **Adding a seam impl**: implement the trait → add a feature to the owning
  crate → register a factory line in `register_builtins` (guarded by the feature)
  → document it. See `docs/extending.md`.
- Keep the tree **warning-clean** (`clippy -D warnings`) and formatted
  (`cargo fmt`, `nix fmt`).
- Only commit or push when asked. Never commit secrets/API keys.
