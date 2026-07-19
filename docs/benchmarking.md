# Performance & leak checks — design & motivation

This is the *why* behind agent-seddon's performance and heap gate. For the
practical "how do I add one" reference — commands, templates, ceilings — see
[`components/benchmarking.md`](components/benchmarking.md). This document explains
the choices so a future contributor can extend or change the harness without
re-deriving the reasoning.

## The problem

An agent harness is a hot loop wrapped around expensive I/O: assemble context →
call the model → dispatch tools → record → compact, every turn. The seams that do
this are exactly the code most likely to grow an accidental `O(n²)` scan, an
allocation in a tight loop, or a slow drift that no one notices until a session
feels sluggish. Two failure modes matter:

- **Performance regressions** — a change makes a hot path do materially more work.
- **Leaks / allocation blow-ups** — a change holds memory it shouldn't, or starts
  allocating per-item where it used to allocate once.

Neither is caught by the existing gate (`clippy`, `rustfmt`, `cargo test`). Both
are the kind of thing you want to catch *at merge time*, like a failing test — not
discover in production. So the design goal is: **make "it got slower / it leaks" a
build failure**, with the same determinism and low friction as a unit test.

## Why instruction counts, not wall-clock time

The obvious approach — time the code with something like `criterion` — is a poor
fit for a **gate**. Wall-clock time depends on the machine, its thermal state,
neighbouring load, and CPU frequency scaling; the same code varies run to run.
That forces statistical baselines and generous thresholds, which are flaky in CI
and useless in a *stateless* check that has no memory of previous runs.

Instead we count **instructions** with [iai-callgrind](https://github.com/iai-callgrind/iai-callgrind),
which runs each benchmark under valgrind's callgrind. The instruction count for a
given binary is **deterministic** — identical run to run, independent of the
machine's speed or load. That single property is what makes everything else work:

- A regression is a *fact*, not a *probability* — no warm-up, no noise, no retries.
- The gate needs **no stored baseline or history**, so it works inside the
  hermetic, stateless `nix flake check` sandbox.
- Because valgrind and the Rust toolchain are **pinned in Nix**, the same binary
  and therefore the same count is reproduced on any machine — a contributor's
  laptop, CI, a reviewer's checkout.

The tradeoff is honest: instruction count measures *work done*, not *time*. It
won't see a change that trades instructions for cache locality or parallelism, and
callgrind runs the code ~10–50× slower than native. We accept that: for a **gate**
we want a stable "did this hot path start doing a lot more work?" signal, and
instruction count is the most reproducible proxy for it. Wall-clock profiling is
still the right tool for tuning — it's just not what should block a merge.

## Why absolute ceilings, not relative baselines

iai-callgrind's default workflow compares against a saved baseline from a previous
run. That needs somewhere to *store* the baseline, which a stateless check doesn't
have. Committing baseline files works but is fiddly and machine-coupled.

We instead set an **absolute instruction ceiling per benchmark, in the bench file**:

```rust
#[library_benchmark(config = LibraryBenchmarkConfig::default()
    .tool(Callgrind::default().hard_limits([(EventKind::Ir, 800_000u64)])))]
fn record_and_encode() -> String { … }
```

Exceeding the ceiling makes `cargo bench` exit non-zero, which fails the `bench`
check. Ceilings are set ~40% above the observed count. This is deliberately
**coarse**: the goal is to catch "major low-hanging fruit" — a hot path that
suddenly costs 2× — not to police 1% micro-movements. When a legitimate change
moves a bench, you bump the ceiling in the same PR, and the diff is the permanent
record of "this change cost N more instructions, on purpose." The budget lives
next to the code it guards, versioned with it.

## Why dhat for leaks and allocations

Rust's ownership model prevents use-after-free, but it does **not** prevent leaks
(reference cycles, an unbounded cache, a `Box::leak`, a channel that never drains)
or *allocation* regressions (allocating per-item in a loop that should allocate
once). Those are real and invisible to the type system.

[dhat](https://docs.rs/dhat) is a heap profiler that, with its global allocator
installed, tracks every allocation. A leak test runs a hot path in a scope and
asserts two things:

- **No leak** — live blocks return to the pre-scope count once the work drops.
- **Allocation budget** — total blocks stay under a ceiling, catching an
  accidental per-call allocation blow-up.

dhat's allocator is gated behind a per-crate **`dhat-heap` cargo feature**, so it
is compiled *only* for the leak test and is never present in a normal build or
release binary — zero cost to everything else. Unlike the perf benches, leak
assertions need no valgrind and are trivially sandbox-safe.

## How it's wired

```
Cargo.toml            iai-callgrind + dhat as workspace dev-deps
   │
   ├─ crates/*/benches/*.rs   iai benches, absolute Ir ceilings in-code
   │        └── nix/checks/bench.nix   run under valgrind → gate
   ├─ crates/*/tests/leak.rs  dhat leak + allocation-budget asserts (feature-gated)
   │        └── nix/checks/leak.nix    run with --features dhat-heap → gate
   │
   ├─ agent-testkit::bench     deterministic input fixtures (shared by benches+tests)
   ├─ agent-testkit::observe   MetricsProbe + captured_spans (assert observability)
   │
   └─ nix/versions.nix   pins valgrind + a version-matched iai-callgrind-runner
            nix/default.nix   `nix run .#bench` app + the bench/leak checks
```

A few implementation choices worth knowing:

- **The runner is pinned and version-locked.** iai-callgrind needs a companion
  `iai-callgrind-runner` binary whose version must *equal* the `iai-callgrind`
  dev-dependency. Both are pinned in `nix/versions.nix` (the runner built via
  `buildRustPackage`), so the dev shell, `nix run .#bench`, and the CI gate all use
  the same runner — bump the dep, the version, and the two hashes together.
- **Determinism discipline in fixtures.** Bench inputs come from
  `agent-testkit::bench` (e.g. `write_tree`, `message_history`, `fact_corpus`) and
  are **pure functions of their arguments** — no clocks, no randomness — because a
  varying input would vary the instruction count and defeat the whole approach.
- **valgrind runs in the Nix sandbox.** The `bench` check executes callgrind inside
  the hermetic build sandbox; this was verified to work, so the perf gate is a real
  `nix flake check` target, not a manual step.

## How it relates to observability

The metrics and tracing stack ([`observability.md`](observability.md)) measures a
*running* agent — latencies, token counts, span trees. This harness guards the
*code* that produces those numbers, at merge time, before anything runs in
production. The two meet in `agent-testkit::observe`: `MetricsProbe` lets a unit
test assert an operation actually moved a Prometheus metric (via the public text
exposition), and `captured_spans` asserts a code path emitted the span it should.
So a feature can prove it is **correct, fast, leak-free, and observable** in the
same test suite — and all of it gates the build.

## The per-feature pattern

This harness is Phase 0 of implementing the [top-10 fundamentals](parity/) test
plans. Every feature PR that follows adds, alongside its `#[rstest]` tables: an iai
bench with a ceiling over its hot path, a dhat leak test, and (where a metric or
span exists) a `MetricsProbe`/`captured_spans` assertion. The harness is the fixed
cost that makes each of those a few lines instead of a project.

## Non-goals

- **Not** a wall-clock profiler or a load/throughput test — instruction count is a
  merge-gate proxy, not a latency measurement. Use native profiling for tuning.
- **Not** a micro-optimisation police — ceilings are coarse by design; small,
  intentional movements are handled by bumping the ceiling in the PR.
- **Not** a substitute for real memory profiling under production workloads — the
  leak tests assert bounded, deterministic hot paths, not whole-system behaviour.
