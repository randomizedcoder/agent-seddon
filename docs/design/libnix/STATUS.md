# Cargo libnix experiment - status

Legend: done / in progress / next / blocked / dropped

Design: [`README.md`](README.md) · evaluation: [`EVALUATION.md`](EVALUATION.md) · feedback:
[`FEEDBACK.md`](FEEDBACK.md).

| # | Increment | State | Notes |
|---|---|---|---|
| LN-00 | Create experiment docs and evaluation protocol | done | This document set |
| LN-01 | Pin libnix Cargo release in Nix | done | `nix/libnix-cargo.nix`, release `libnix-1.87.0+v4` |
| LN-02 | Add opt-in `devShells.libnix` | done | Default `nix develop` unchanged |
| LN-03 | Add minimal `Cargo.dependencies.nix` | done | `agent-proto`/`PROTOC` only |
| LN-04 | Run smoke builds | done | `nix run .#libnix-smoke`; v4 `check` limitation documented |
| LN-05 | Compare default Cargo, Crane, and libnix timings | in progress | M2 and M4 recorded; toolchain/cache states differ |
| LN-06 | Decide whether to add `apps.libnix-smoke` / `checks.libnix-smoke` | next | Promote only after repeated stable runs |
| LN-07 | Package upstream feedback | next | Summarize failures, wins, missing docs, measurements |

## As-Built Log

- **2026-10-14 - LN-00.** Created the design/evaluation track. Current assessment:
  integrate libnix as an opt-in path first. The existing Crane setup is already
  optimized enough that replacing it immediately would hide signal behind churn.
  The first useful proof is whether libnix can build the workspace's build-script
  crates and reuse work across feature-scoped commands better than Crane plus
  `sccache`.
- **2026-10-14 - LN-01..LN-04 wiring.** Added `nix/libnix-cargo.nix` pinned to
  upstream release `libnix-1.87.0+v4` (Rust 1.87.0 / Cargo 0.88.0), a root
  `Cargo.dependencies.nix` with only `agent-proto`'s `PROTOC`, an opt-in
  `devShells.libnix`, `packages.cargo-libnix`, and `nix run .#libnix-smoke`.
  The libnix shell disables `sccache` so measurements are about libnix/Nix-store
  reuse, not the local compiler cache.

- **2026-10-14 - LN-04 first run.** The smoke runner reached libnix Cargo and
  confirmed its pinned versions (`cargo 1.87.0 +libnix`, Rust 1.87.0), then
  failed immediately because this release refuses `cargo check` as unsupported.
  Updated the smoke probes to use `cargo build` for those two commands and
  documented this comparison limitation. The corrected runner passed all three
  probes: `agent-proto` build (including build-script/protoc), `agent-core` test
  (681 passed plus doc tests), and `agent-runtime --no-default-features` build.
  The first complete run took about 4 minutes for the first graph realization;
  the core test graph took 47.8s and the lean runtime graph took 203s to build.
  No native dependency injection beyond `agent-proto`'s `PROTOC` was needed.
- **2026-10-14 - LN-05 M2.** Recorded three runs for `cargo test -p agent-core`
  per backend in `EVALUATION.md`. Warm local Cargo+sccache took 1.12s/0.95s;
  libnix took 47.98s/48.26s and rebuilt four derivations each time. Crane's
  first targeted derivation took 139.02s; subsequent Nix-store no-ops took
  0.23s/0.24s without rerunning tests. Toolchain and cache differences mean
  this is diagnostic only. Investigate libnix's repeated rebuild before
  widening the comparison matrix.
- **2026-10-14 - LN-05 M4.** `cargo test -p agent-config-store --features
  config-store-sqlite` passed with all three backends (94 tests). The first
  libnix run took 241.23s including 163.5s graph evaluation; Cargo+sccache took
  26.08s, and Crane took 59.30s. These are single runs with different toolchain
  and cache states. Bundled SQLite required no extra libnix native inputs.

## Decisions

- **D1: Opt-in first.** `packages.agent`, `checks.*`, and `devShells.default`
  stay on Crane and rust-overlay until libnix has measured wins.
- **D2: Keep measurements comparable.** Every benchmark row records toolchain,
  libnix release, nixpkgs rev, command, commit, dirty state, and cache state.
- **D3: Treat failures as evidence.** A libnix failure should become either a
  local `Cargo.dependencies.nix` entry or an upstream feedback item.
- **D4: Do not chase native deps preemptively.** Add per-crate deps/env only
  after a concrete failure or warning.

## Open Questions

- Which libnix release should be pinned for the first pass?
- Does the libnix Cargo release need to match this repo's current Rust toolchain
  exactly, or can it safely drive the workspace with a nearby Cargo version?
- Does libnix provide enough benefit for CI checks, or is the main win local
  development?
- Can generated libnix build-system output be committed without becoming noisy
  whenever `Cargo.lock` or workspace membership changes?
