# Load/overload harness — implementation status

The living tracker for the [overload/backpressure + load harness](README.md) design.
One gated PR per increment, off `main`, checkpoint after each.

## Increments

| # | Increment | Server fix | Tests | Nix | Status |
|---|---|:--:|:--:|:--:|:--:|
| 01 | Uniform admission layer + `RESOURCE_EXHAUSTED` mapping | ✅ | ✅ | — | **merged** (#133) |
| 02 | Pool saturation visible on the wire | ✅ | ✅ | — | **merged** (#134) |
| 03 | Conformance harness core + per-seam ramp | — | ✅ | ✅ | **merged** (#135) |
| 04 | Stress: pool saturation + streaming scenarios | — | ✅ | ✅ | **merged** (#136) |
| 05 | Full-loop e2e concurrency | — | ✅ | ✅ | **in review** |
| 06 | `ghz` wire load + `/metrics` correlation | — | ☐ | ☐ | pending |

## 01 — what shipped

- **`Error::Overloaded(String)`** (agent-core) → **`RESOURCE_EXHAUSTED`** in
  `status_from_error` — the "please slow down" error (client already retries code 8).
- **`AdmissionLayer`** (`agent-grpc/src/server/admission.rs`): a `tower::Layer` +
  `Service` holding an `Arc<Semaphore>`; over the cap it sheds *without touching the
  inner service* with a `RESOURCE_EXHAUSTED` response carrying
  `grpc-retry-pushback-ms`. Permit held RAII for the call. `0` ⇒ pass-through. A
  shared shed counter is exposed for the harness.
- **Wired uniformly** into `base_router(max_in_flight)` (`server/health.rs`) via
  `Server::builder().layer(...)`, so every seam + `--serve-all` gets it. Threaded the
  new `ServeRouter` type through `with_reflection` / `add_seam_service`; made
  `Endpoint::serve` + the test `spawn` **generic over the layer** so bare and layered
  routers both serve.
- **Config:** `[grpc] max_in_flight` (`Option<usize>`; absent ⇒ 1024 protective
  default, `0` ⇒ off) → `Settings.grpc_max_in_flight` → `Agent::grpc_max_in_flight()`,
  read by the serve path.
- **Tests** (`agent-grpc/tests/admission.rs`): a flood past the cap sheds
  `RESOURCE_EXHAUSTED` + a pushback trailer (raw client, no retry); `cap = 0` never
  sheds. All 45 existing round-trips still pass under the generic `spawn`.

## 02 — what shipped

- **Pool saturation is now visible on the wire.** `LlmPoolService.Complete` returned a
  fail-soft empty `OK` batch on saturation, so a remote client couldn't tell it was
  overloaded. It now disambiguates via `health()`: an empty batch **with an alive,
  saturated member** sheds with `RESOURCE_EXHAUSTED` + a `grpc-retry-pushback-ms` hint
  (via the shared `overloaded_status` helper, extracted from the admission layer). A
  genuinely empty/dead pool still returns the empty batch (not overload).
- **No client change needed** — `GrpcLlmPool::complete_all` already routes through
  `call_retry`, which retries `RESOURCE_EXHAUSTED` with bounded jittered backoff and
  honors+clamps the pushback; a sustained shed still fails soft to an empty batch, but
  now *after* backing off rather than hammering.
- **Tests** (`agent-grpc/tests/pool_backpressure.rs`): a saturated pool → `RESOURCE_EXHAUSTED`
  + pushback (raw client); an empty/dead pool → an empty `OK` batch (not overload).
- **Deferred (documented):** teaching `parse_retry_after` the **HTTP-date** form was
  scoped out — `agent-retry` is intentionally dep-free *and clock-free* (its own doc +
  the `http_date → None` test say so), and the date form needs both a date lib and
  `SystemTime::now()`; the fallback (jittered backoff) is already safe, so the cost
  outweighs the value.

## 03 — what shipped

- **The harness** (`crates/agent-grpc/examples/loadtest.rs`, opt-in `nix run .#loadtest`):
  a concurrency-driver (`run_load`) + **dep-free percentiles** (unit-tested:
  empty/single/monotonic/unsorted), reusing a generic `spawn` + agent-testkit doubles.
  - `--scenario ramp` — each seam × {TCP, UDS} × concurrency → a table (`req/s · p50 ·
    p99 · p999 · shed · err`) + `--json`. Confirmed **UDS beats TCP** and surfaced a
    periodic ~42 ms TCP loopback tail (Nagle/delayed-ack) that UDS avoids.
  - `--scenario overload` — floods a seam past the admission cap through the **raw**
    client and **asserts the contract**: every non-success must be `RESOURCE_EXHAUSTED`
    (a non-shed error exits **2**), some succeed, no hang. Verified: cap 4 / conc 64 →
    420 shed, 0 non-shed errors.
  - Seams covered: provider, tokenizer, prompt, memory (the diverse light patterns;
    more are mechanical registry additions).
- **Gate:** `nix/checks/loadtest-smoke.nix` — a tiny model-free overload+ramp run
  (`--require-shed`) asserting the harness compiles, sheds, and runs clean; **no perf
  numbers asserted**. `nix run .#loadtest` app registered.

## 04 — what shipped

- **`--scenario saturation`** — a self-contained `LoadPool` (tracks in-flight per
  member, RAII slot + a slow "generation", returns an empty batch at capacity) driven
  past its cap through the raw `LlmPoolService` client. Exercises the inc-02 wire
  signal under **real concurrent load**: verified cap 4 / conc 64 → 300 shed
  (`RESOURCE_EXHAUSTED`), 0 non-shed errors. No `agent-providers` dep needed (the real
  `PoolProvider`'s saturation math is unit-tested in `pool.rs`; the harness proves the
  *wire contract* under load).
- **`--scenario streaming`** — M concurrent `Provider.Stream` server-streams
  (`ScriptedProvider` chunked); each fully drained, a mid-stream error or stall is the
  failure. Verified conc 32 → 224 streams drained, 0 errors — the producer never stalls.
- **Gate:** `loadtest-smoke` now also runs tiny saturation (`--require-shed`) +
  streaming micro-runs, so both stress paths are kept honest without perf assertions.
- The contract check generalized: overload/saturation must shed only
  `RESOURCE_EXHAUSTED`; streaming must not error (exit 2 on a violation).

## 05 — what shipped

- **`crates/agent-runtime/examples/loadtest_loop.rs`** (opt-in `nix run .#loadtest-loop`):
  drives the **whole agent loop** in-process under concurrency, not one seam over the
  wire. Builds **one** production `Agent` (real `registry → builder → metered seams →
  loop`, `auto-approve`, temp dirs) with a tool-capable scripted model, then fires **N
  concurrent `agent.run()`**. Each `run` opens its own `Session`/`WorkingSet`, so the
  runs are independent and the only shared mutable state is what a real deployment
  shares — the memory store, the metrics registry, the seam `Arc`s.
- **Client-vs-server correlation, no network.** Alongside client-side run latency
  (p50/p99/p999/max) + throughput, it reads the loop's *own* instrumentation via
  `agent_testkit::observe::MetricsProbe` (a snapshot diff over `Metrics`):
  `agent_runs_total`, `agent_run_duration_seconds`, `agent_provider_request_seconds`.
  Verified conc 16 / 256 runs → 0 err, ~5k runs/s, client p50 2.77 ms vs server mean
  2.53 ms, and **exactly 2.0 provider calls/run** (tool turn + final) — the two-turn
  loop genuinely iterated under load.
- **Concurrency-safe model without a shared cursor.** The scripted turn is computed
  from the *request* (answer once a tool observation is present, else ask to read the
  seed file), so it's correct under any interleaving — a `ScriptedProvider`'s shared
  `AtomicUsize` cursor would race across concurrent runs.
- **Contract:** any run error exits 1; a client success the server didn't record
  (`agent_runs_total` delta < client oks) exits 2.
- **Gate:** `loadtest-smoke` now also runs a tiny in-process loop (conc 4 / 20 runs),
  so the full-loop path + the metric correlation stay honest — still no perf assertion.

## Notes / decisions of record

- **Not gated on numbers.** Load/throughput is machine-dependent; the perf gate stays
  deterministic (iai-callgrind). The harness is opt-in; only a model-free smoke check
  (harness runs, sheds > 0) enters `nix flake check`.
- **One process-wide cap**, not per-seam — matches "all the services" and is the
  smallest uniform change. Per-seam caps are a documented follow-up.
- **Streaming** (`Provider.Stream`, `AgentSessionService.Subscribe`) isn't retried and
  leans on HTTP/2 flow control + the bounded broadcast's lag-and-drop; the admission
  layer bounds handler *setup* concurrency, not stream duration (acceptable).
