# Design: Overload/backpressure conformance + seam load harness

Status: **implementation in progress.** The design of record for making every seam
service **signal overload the gRPC way** and verifying clients respond correctly,
plus quantifying seam performance under load. Tracker: [`STATUS.md`](STATUS.md).

## The problem

Each seam can run as a gRPC service, but the README is honest that a hop costs
something — and that cost, plus two deliberately-designed stress behaviours, was
**unquantified and partly unimplemented**:

- **No service told a client to slow down.** `status_from_error` only ever emitted
  `INTERNAL` / `INVALID_ARGUMENT` / `UNAVAILABLE` — **never `RESOURCE_EXHAUSTED`**.
  Overload (`Error::Provider`) → `INTERNAL` → *not* retryable → the client failed
  fast, the opposite of backpressure. No seam had admission control (zero
  `concurrency_limit`/`load_shed` layers). The pool's saturation was *invisible on
  the wire* (`LlmPoolService.Complete` returns an empty `OK` batch).
- **The client was already correct + ready.** `agent-retry` does bounded exponential
  backoff + full jitter, honors and **clamps** server pushback
  (`grpc-retry-pushback-ms`, incl. the `-1` veto), blocks retry-amplification on
  permanent codes, and **already treats `RESOURCE_EXHAUSTED` as retryable**. It just
  never received the signal.

## The contract

> **Server, under overload:** shed immediately with `Status::RESOURCE_EXHAUSTED`
> carrying a sane `grpc-retry-pushback-ms` hint — never `INTERNAL`, never a hang,
> never unbounded memory.
>
> **Client, on `RESOURCE_EXHAUSTED`:** back off (honoring + clamping the pushback),
> retry up to the cap with de-correlated jitter (no stampede), then succeed once load
> drops or fail cleanly. Streaming RPCs aren't retried — a partial stream can't replay.

## The mechanism — a uniform admission layer

One `tower::Layer` ([`agent-grpc/src/server/admission.rs`](../../../crates/agent-grpc/src/server/admission.rs))
applied inside the shared `base_router()`, so **every seam and `--serve-all` shed the
same way** with no per-seam code. It bounds in-flight requests with a semaphore
(permit held RAII for the call); over the cap a new request is shed *without touching
the inner service* as a `RESOURCE_EXHAUSTED` response carrying the pushback hint. The
cap is `[grpc] max_in_flight` (absent ⇒ a protective **1024**; `0` ⇒ unbounded).
`Error::Overloaded` → `RESOURCE_EXHAUSTED` covers a seam impl that sheds internally
(e.g. the pool).

Because applying a tower layer changes the router's type, the serve path threads a
`ServeRouter` alias, and `Endpoint::serve` / the test `spawn` are generic over the
layer so they accept both the bare router (per-seam tests) and the layered one.

## The verifier — a load/conformance harness

An **opt-in** `nix run .#loadtest` (never gated — throughput is machine-dependent;
the gate stays deterministic iai-callgrind) that drives each service into overload
and **asserts the contract** (shed is `RESOURCE_EXHAUSTED`, not `INTERNAL`/hang/OOM;
pushback sane; the retry client recovers once load drops; no stampede; bounded
memory), and separately reports **throughput/latency** under a concurrency ramp. A
model-free `loadtest-smoke` check in the gate keeps the harness alive. It reuses the
`spawn()` harness + agent-testkit doubles, so it drives all 26 seams hermetically.

## Scope (both correctness + perf)

Backpressure contract + fixes, **and** the full perf harness: per-seam
throughput/latency ramp (TCP vs UDS), pool saturation, streaming concurrency,
full-loop e2e, plus a `ghz` wire load against a real `--serve-all` with server-side
`/metrics` correlation. Delivered across the increments in [`STATUS.md`](STATUS.md).

## The opt-in apps

Three harnesses, none gated (a model-free `loadtest-smoke` check keeps them
compiling + behaving in `nix flake check`):

| App | Tier | What it does |
|---|---|---|
| `nix run .#loadtest` | in-process, wire seams | `--scenario {ramp,overload,saturation,streaming}` — hermetic hand-rolled clients over `spawn()` + testkit doubles; asserts the overload contract, reports ramp throughput, drives pool saturation + concurrent streams |
| `nix run .#loadtest-loop` | in-process, full loop | N concurrent `agent.run()` on one real `Agent`; client latency vs the loop's own server-side metrics (`MetricsProbe`), no network |
| `nix run .#loadtest-wire` | real wire | starts `agent --serve-all` and drives it with **`ghz`** via reflection; correlates ghz's client numbers with the scraped `/metrics` histogram, and bursts past the admission cap to see `RESOURCE_EXHAUSTED` on the wire |
