# Cargo libnix evaluation protocol

This file defines how to compare normal Cargo, Crane, and Cargo libnix for this
repo. The goal is to produce evidence that is useful locally and useful upstream.

## What We Measure

Measure three things:

- Wall-clock time for representative commands.
- Nix-store and local `target/` space used.
- Failure mode quality: what failed, how clear the error was, and what fix was
  needed.

## Required Run Metadata

Every result row must record:

| Field | Example |
|---|---|
| Date | `2026-10-14` |
| Host | `l2` |
| Repo commit | `git rev-parse HEAD` |
| Dirty state | `clean`, `dirty-docs-only`, or summary |
| nixpkgs rev | from `flake.lock` |
| Rust version | `rustc --version` |
| Cargo version | `cargo --version` |
| libnix version | release tag/hash, or `none` |
| Command | exact shell command |
| Cache state | cold, warm, or after source edit |
| Elapsed | `/usr/bin/time -p` real time |
| Store delta | before/after Nix store size if available |
| target delta | before/after `target/` size |
| Result | pass/fail and short note |

## Command Matrix

Start small, then widen.

| ID | Command | Why |
|---|---|---|
| M1 | `cargo check -p agent-proto` | Exercises `tonic-build`, `prost-build`, and `PROTOC` |
| M2 | `cargo test -p agent-core` | Small pure Rust baseline |
| M3 | `cargo check -p agent-runtime --no-default-features` | Lean feature path used by the gate |
| M4 | `cargo test -p agent-config-store --features config-store-sqlite` | Bundled SQLite/build-script path |
| M5 | `cargo clippy --all-targets --all-features -- -D warnings` | Heavy all-feature static-analysis path |
| M6 | `nix build .#agent` | Current packaged build baseline |
| M7 | `nix flake check` | Full current gate, only after smoke success |

## Cache Scenarios

Run each selected command under these scenarios:

| Scenario | Setup |
|---|---|
| Cold | No relevant local `target/`, empty/irrelevant `sccache`, no libnix outputs built |
| Warm no-op | Repeat the exact same command |
| First-party edit | Touch or minimally edit one leaf crate source file |
| Build-script input edit | Touch one `agent-proto/proto/**/*.proto` file, then revert |
| Feature change | Switch between default, no-default, and feature-scoped commands |

Do not delete unrelated Nix-store paths for cold runs unless that is explicitly
called out. A realistic "cold for this project" run is more useful than an
unusual host-wide purge.

## Timing Wrapper

Use a small wrapper so results are consistent:

```sh
run_measure() {
  label="$1"
  shift
  before_target="$(du -sBG target 2>/dev/null | cut -f1 || true)"
  before_store="$(nix path-info -S .#agent 2>/dev/null || true)"
  /usr/bin/time -p "$@"
  status="$?"
  after_target="$(du -sBG target 2>/dev/null | cut -f1 || true)"
  after_store="$(nix path-info -S .#agent 2>/dev/null || true)"
  printf 'label=%s status=%s target_before=%s target_after=%s store_before=%s store_after=%s\n' \
    "$label" "$status" "$before_target" "$after_target" "$before_store" "$after_store"
  return "$status"
}
```

For libnix-specific commands, run inside `nix develop .#libnix` and record
`CARGO_BACKEND`. The pinned libnix v4 release does not implement `cargo check`,
so use `cargo build` for the build-script and lean-feature smoke probes. Record
that command difference when comparing with normal Cargo or Crane; do not claim
like-for-like timing for `check` versus `build`.

The LN-04 smoke run passed on 2026-10-14. Its first invocation fetched 147
paths and built the initial dependency graph; `agent-core` tests passed (681
unit tests plus doc tests), and the lean runtime build passed. These are
correctness probes only, not performance results. Populate the result table
with separate exact commands and cache states before drawing timing conclusions.

## Result Table Template

| Date | ID | Backend | Scenario | Command | Elapsed | target delta | store delta | Result | Notes |
|---|---|---|---|---|---:|---:|---:|---|---|
| 2026-10-14 | M1 | crane | cold | `nix build .#checks.x86_64-linux...` | TBD | TBD | TBD | TBD | TBD |
| 2026-10-14 | M1 | cargo+sccache | cold | `cargo check -p agent-proto` | TBD | TBD | TBD | TBD | TBD |
| 2026-10-14 | M1 | cargo-libnix | cold | `cargo check -p agent-proto` | TBD | TBD | TBD | TBD | TBD |

## Initial Results

### M2: `cargo test -p agent-core`

Host: `l2`; date: `2026-10-14`; repo commit: `fafe69f3`; working tree: dirty
(178 paths reported by `git status --porcelain`, including unrelated ongoing
work); nixpkgs rev: `753cc8a3a87467296ddd1fa93f0cc3e81120ee46`.

| Backend | Toolchain | Exact command | Cache state | Elapsed | Result / notes |
|---|---|---|---|---:|---|
| Cargo + sccache | Cargo/rustc 1.97.1; sccache 0.16.0 | `nix develop .#default --command /usr/bin/time -p cargo test -p agent-core` | Existing 201 GB `target/`; first measured run | 14.46s | Pass; 681 tests |
| Cargo + sccache | Same | Same | Warm repeat 1 | 1.12s | Pass; 681 tests |
| Cargo + sccache | Same | Same | Warm repeat 2 | 0.95s | Pass; 681 tests |
| Cargo + sccache | Same | Same | Warm follow-up | 0.97s | Pass; 681 tests rerun |
| Crane | Cargo/rustc 1.97.1 | `/usr/bin/time -p nix build .#checks.x86_64-linux.agent-core-eval` | Cold derivation realization | 139.02s | Pass; temporary `craneLib.cargoTest` target with `cargoTestExtraArgs = "-p agent-core"`; 56 derivations realized |
| Crane | Same | Same | Warm Nix-store no-op 1 | 0.23s | Cached derivation; tests did not rerun |
| Crane | Same | Same | Warm Nix-store no-op 2 | 0.24s | Cached derivation; tests did not rerun |
| Cargo libnix | Cargo 1.87.0+v4 / rustc 1.87.0 | `nix develop .#libnix --command /usr/bin/time -p cargo test -p agent-core` | First run after LN-04 smoke | 48.53s | Pass; 681 tests; evaluated graph in 33.2s; 4 derivations built |
| Cargo libnix | Same | Same | Repeat 1 | 47.98s | Pass; 681 tests; 4 derivations built again |
| Cargo libnix | Same | Same | Repeat 2 | 48.26s | Pass; 681 tests; 4 derivations built again |
| Cargo libnix | Same | Same | Warm follow-up after M4 | 49.34s | Pass; 681 tests; 4 derivations built again |

These rows are diagnostic, not a performance verdict. The Cargo and Crane runs
used the repository toolchain (1.97.1), while libnix is pinned to 1.87.0; the
local Cargo target was already populated, the Crane first run realized a new
derivation graph, and Crane's repeat rows are Nix-store no-ops that did not
rerun tests. In contrast, each libnix invocation re-evaluated the 95-unit graph
and reported four built derivations despite no source edits. Store and target
size deltas were not captured for these runs. The temporary Crane target was
removed after measurement; it used `craneLib.cargoTest` with the same
`commonArgs`, shared `cargoArtifacts`, and `cargoTestExtraArgs = "-p agent-core"`.

Observed warm behavior: local Cargo with sccache reran the test binary in a
median 0.97s across its three warm runs, Crane returned its already-built
derivation in a median 0.235s without rerunning tests, and libnix took a median
48.26s across its three repeated runs while rebuilding four derivations each
time. This is a warm-to-warm comparison of the same test target, with the
important distinction that Crane's cached derivation skips test execution. The
toolchain/cache differences prevent treating these as a
controlled winner comparison; the repeated libnix rebuild is a concrete item to
investigate before widening the matrix.

### M4: SQLite feature test

Command: `cargo test -p agent-config-store --features config-store-sqlite`.
All three backends passed the same 94 tests (plus doc tests):

| Backend | Toolchain | Exact command | Cache state | Elapsed | Notes |
|---|---|---|---|---:|---|
| Cargo + sccache | Cargo/rustc 1.97.1 | `nix develop .#default --command /usr/bin/time -p cargo test -p agent-config-store --features config-store-sqlite` | First local feature build | 26.08s | Pass; compiler phase 22.69s |
| Crane | Cargo/rustc 1.97.1 | `/usr/bin/time -p nix build .#checks.x86_64-linux.agent-config-store-sqlite-eval` | First targeted derivation | 59.30s | Pass; temporary `craneLib.cargoTest` target with `cargoTestExtraArgs = "-p agent-config-store --features config-store-sqlite"` |
| Cargo libnix | Cargo 1.87.0+v4 / rustc 1.87.0 | `nix develop .#libnix --command /usr/bin/time -p cargo test -p agent-config-store --features config-store-sqlite` | First feature-graph realization | 241.23s | Pass; 163.5s graph evaluation, 148 derivations built |

No native inputs or environment entries were needed for bundled SQLite;
`libsqlite3-sys` built with the existing minimal dependency file. This is a
single run per backend and uses different toolchains and cache states, so it
confirms feature support but does not establish relative performance. The
temporary Crane target has been removed; its derivation used `commonArgs`, the
shared `cargoArtifacts`, and the arguments shown above.

## Evaluation Notes For Upstream

When a libnix run fails, capture:

- exact command
- exact crate and version
- missing native input or env var, if known
- whether a `Cargo.dependencies.nix` entry fixed it
- whether the error message led to the fix
- whether the same command works with normal Cargo

When a libnix run wins, capture:

- cold vs warm difference
- whether the win survived a first-party edit
- whether the win survived a feature-set change
- whether store growth was lower, equal, or higher

## Interpretation Rules

- A single fast run is not enough. Require at least three repeated warm runs for
  any performance claim.
- Separate "faster because less work" from "faster because the cache was already
  warm".
- Do not compare `nix flake check` directly to one Cargo command. Compare like
  with like first, then evaluate whole-gate impact.
- Prefer practical developer time over synthetic purity. If libnix makes the
  everyday edit/check loop meaningfully better, that matters even if full-gate
  results are neutral.
