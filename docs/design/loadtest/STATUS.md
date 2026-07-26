# Load/overload harness — implementation status

The living tracker for the [overload/backpressure + load harness](README.md) design.
One gated PR per increment, off `main`, checkpoint after each.

## Increments

| # | Increment | Server fix | Tests | Nix | Status |
|---|---|:--:|:--:|:--:|:--:|
| 01 | Uniform admission layer + `RESOURCE_EXHAUSTED` mapping | ✅ | ✅ | — | **in review** |
| 02 | Pool saturation visible on the wire (+ HTTP-date `Retry-After`) | ☐ | ☐ | — | pending |
| 03 | Conformance harness core + per-seam ramp | — | ☐ | ☐ | pending |
| 04 | Stress: pool saturation + streaming scenarios | — | ☐ | ☐ | pending |
| 05 | Full-loop e2e concurrency | — | ☐ | ☐ | pending |
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

## Notes / decisions of record

- **Not gated on numbers.** Load/throughput is machine-dependent; the perf gate stays
  deterministic (iai-callgrind). The harness is opt-in; only a model-free smoke check
  (harness runs, sheds > 0) enters `nix flake check`.
- **One process-wide cap**, not per-seam — matches "all the services" and is the
  smallest uniform change. Per-seam caps are a documented follow-up.
- **Streaming** (`Provider.Stream`, `AgentSessionService.Subscribe`) isn't retried and
  leans on HTTP/2 flow control + the bounded broadcast's lag-and-drop; the admission
  layer bounds handler *setup* concurrency, not stream duration (acceptable).
