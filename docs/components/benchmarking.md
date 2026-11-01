# Benchmarking & leak checks

> **This is the how-to reference** (commands, templates, ceilings). For the *why*
> — why instruction counts over wall-clock, why absolute ceilings, why dhat, and
> how the pieces fit — see the design doc [`../benchmarking.md`](../benchmarking.md).

Performance and heap behaviour are treated like correctness: both are **gated by
`nix flake check`**, so a regression fails the build. Two mechanisms, both
deterministic (no wall-clock noise, no flaky thresholds):

| Concern | Tool | Where | Gate |
|---|---|---|---|
| CPU / instruction cost | [iai-callgrind](https://github.com/iai-callgrind/iai-callgrind) under `valgrind` | `crates/*/benches/*.rs` | `nix/checks/bench.nix` |
| Heap leaks + allocation budget | [dhat](https://docs.rs/dhat) | `crates/*/tests/leak.rs` | `nix/checks/leak.nix` |

Why these two: callgrind counts **instructions**, which are identical run-to-run
for a given binary + valgrind, so an **absolute ceiling** is a stable gate a
stateless check can enforce — unlike wall-clock benchmarks, which need a warm
machine and statistical baselines. dhat tracks every allocation, so "did this hot
path leak / suddenly allocate 10× more?" becomes a hard assertion.

## Running locally

```sh
nix run .#bench                        # every bench (valgrind + runner on PATH)
nix run .#bench -- -p agent-metrics    # one crate
nix run .#bench -- --bench metrics -- new_registry   # one bench case

# leak tests (dhat installs its global allocator only under the feature)
nix develop -c cargo test -p agent-metrics --features dhat-heap --test leak
```

`cargo bench` outside the dev shell fails: iai-callgrind needs `valgrind` and the
matching `iai-callgrind-runner` on `PATH`, both provided by `nix develop` /
`nix run .#bench`.

## Writing a benchmark

Add `benches/<feature>.rs` and register it in the crate's `Cargo.toml`:

```toml
[dev-dependencies]
iai-callgrind.workspace = true

[[bench]]
name = "<feature>"
harness = false
```

The bench (see `crates/agent-metrics/benches/metrics.rs` for the exemplar):

```rust
use std::hint::black_box;
use iai_callgrind::{
    library_benchmark, library_benchmark_group, main, Callgrind, EventKind,
    LibraryBenchmarkConfig,
};

// NB: iai-callgrind's macro rejects `///` docs on the benched fn — use `//`.
#[library_benchmark(config = LibraryBenchmarkConfig::default()
    .tool(Callgrind::default().hard_limits([(EventKind::Ir, 1_400_000u64)])))]
fn hot_path() -> Output {
    black_box(do_the_thing(black_box(input())))
}

library_benchmark_group!(name = mygroup; benchmarks = hot_path);
main!(library_benchmark_groups = mygroup);
```

Use `agent-testkit`'s `bench` module for **deterministic inputs** (no clocks / no
randomness — instruction counts must be reproducible): `write_tree`,
`message_history`, `fact_corpus`.

The `hard_limits` value is the bench's **absolute ceiling**: exceeding it makes
`cargo bench` exit non-zero. Start with a placeholder and run it once
(`nix run .#bench -- -p <crate>`); a run over the placeholder fails but still prints the
observed `Instructions:` count. Set the ceiling ~40% above that count. When a legitimate change
moves the count, bump the ceiling in the same PR — the diff is the record of the perf
change.

Then add the bench to the `runs` table in `nix/checks/bench.nix`, under its crate:

```nix
agent-foo = [ "--features <feature> --bench <feature>" ];
```

Each crate in the table is its own `bench-<crate>` check on the crate's source slice
(see [testing.md](testing.md#per-crate-test-slices-the-gate)), so an edit re-runs only
the benches that can see it; `bench` builds them all.

## Writing a leak test

Add `tests/leak.rs` and a per-crate `dhat-heap` feature:

```toml
[dependencies]
dhat = { workspace = true, optional = true }

[features]
dhat-heap = ["dep:dhat"]
```

```rust
#![cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

#[test]
fn hot_path_does_not_leak() {
    let _p = dhat::Profiler::builder().testing().build();
    let before = dhat::HeapStats::get().curr_blocks;
    { /* run the hot path in a scope so its allocations drop */ }
    let stats = dhat::HeapStats::get();
    dhat::assert_eq!(stats.curr_blocks, before);   // no leak
    dhat::assert!(stats.total_blocks < CAP);       // allocation budget
}
```

Add the crate to the `runs` table in `nix/checks/leak.nix`
(`agent-foo = [ "--features dhat-heap --test leak" ];`); it becomes its own
`leak-<crate>` check on the crate's slice, and `leak` builds them all. The
`dhat-heap` feature keeps the global allocator out of every normal build.

The `leak-bench-sync` check fails while a `tests/*leak*.rs` or `benches/*.rs` file
has no row in its table.

## Observability assertions

`agent-testkit`'s `observe` module lets a unit test prove a path is observable,
not just correct:

- `MetricsProbe::new(&metrics)` then `.delta(&metrics, "agent_tool_exec_seconds_count",
  Some("tool=\"edit\""))` — assert an op moved a metric (reads the public text
  exposition, no registry internals).
- `captured_spans(|| …)` — returns the names of spans created in the closure, to
  assert a code path emitted e.g. `skill.load`.

## Versions

`valgrind` and `iai-callgrind-runner` are pinned in
[`nix/versions.nix`](../../nix/versions.nix). The runner's version **must equal**
the `iai-callgrind` dev-dependency in the root `Cargo.toml`. To bump: change the
dep, `iaiCallgrindVersion`, and recompute the two hashes (set each to `""` and read
nix's `got:` line).
