# Retries & backoff — the `agent-retry` library

**There is exactly one retry/backoff implementation in this workspace:
[`agent-retry`](../../crates/agent-retry). Never hand-roll a retry loop, a
`sleep`-and-loop, or a bespoke backoff.** Any code that makes a call which can fail
*transiently* — the LLM HTTP providers, the gRPC seam clients, MCP transports, the
telemetry writer, git fetch — routes through this crate. Adding a new retryable
transport means adding a *classifier* here, not a new loop somewhere else.

`agent-retry` is a leaf crate (it depends on nothing else in the workspace), so
every layer can use it without a dependency cycle.

## Why one library

Retry logic is subtle and easy to get subtly wrong: fixed delays cause thundering
herds, un-jittered exponential backoff synchronises retriers, retrying a
non-idempotent or non-transient failure wastes time (or corrupts state), and
ignoring a server's own backoff hint fights the server instead of cooperating. Get
it right **once**, test it thoroughly, and reuse it everywhere — divergent copies
are how these bugs creep back in.

## What it does (best practices, baked in)

- **Exponential backoff with jitter.** `RetryPolicy` computes
  `min(base · 2^attempt, max_delay)` and applies a [`Jitter`] strategy. The default
  is **full jitter** (`rand[0, ceiling)`), the AWS-recommended choice that
  de-synchronises many clients retrying at once. `Equal` (a floor + half-jitter) and
  `None` (deterministic, for tests) are also available. Defaults: 500 ms base, 20 s
  cap.
- **Server backoff hints win.** A server that tells us how long to wait is obeyed
  (clamped to `max_delay` so a hostile/misconfigured server can't pin us):
  - **HTTP** `Retry-After` — `agent_retry::http::parse_retry_after`.
  - **gRPC** `grpc-retry-pushback-ms` — `agent_retry::grpc::parse_pushback`, which
    also understands the `-1` "do not retry" sentinel.
- **Correct classification** — only genuinely transient failures retry:
  - **HTTP** (`agent_retry::http`): **429** + **5xx** retry; every other 4xx and all
    2xx/3xx do not.
  - **gRPC** (`agent_retry::grpc`): the "overloaded" codes **`UNAVAILABLE (14)`** and
    **`RESOURCE_EXHAUSTED (8)`** retry; `INVALID_ARGUMENT`, `NOT_FOUND`,
    `PERMISSION_DENIED`, `UNAUTHENTICATED`, `INTERNAL`, `DEADLINE_EXCEEDED`, … fail
    fast. (Codes are taken as their numeric wire values, so the crate needs no
    `tonic` dependency; a caller maps `tonic::Status::code() as i32`.)
  - **Transport errors** (connection refused / reset / timeout) retry; the tiny
    reqwest-specific `is_timeout()`/`is_connect()` check lives at the HTTP call site.
- **Bounded.** A hard `max_retries` cap and a per-delay cap; the driver returns the
  last error once exhausted.
- **Streams are not retried.** Only the initial request/response is retried; a
  broken mid-stream SSE/gRPC stream is left fatal, since a partially-consumed stream
  can't be safely replayed.

## The API

Two entry points share one backoff core (`backoff_wait`):

- **`run(policy, op)`** — the transport-agnostic driver. `op` returns an
  [`Attempt`]: `Done(T)` (success), `Retry { err, after }` (transient — retry, with
  an optional server hint), or `Fail(E)` (permanent — stop). This uniformly covers
  both "the HTTP response *arrived* but its status says slow down" and "the call
  returned a retryable error", so it fits every transport.
- **`retry(policy, classify, op)`** — a convenience for a plain
  `Result`-returning call: `classify(&err)` returns `Some(after)` to retry or `None`
  to fail fast.

```rust
// HTTP provider (see crates/agent-providers): a 5xx/429 response is Retry (with
// Retry-After), a connect/timeout error is Retry, other errors Fail, success Done.
let resp = agent_retry::run(&self.retry, || async {
    match self.client.post(&url).json(body).send().await {
        Ok(r) if agent_retry::http::retryable_status(r.status().as_u16()) => {
            let after = r.headers().get(reqwest::header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok()).and_then(agent_retry::http::parse_retry_after);
            Attempt::Retry { err: to_err(&r).await, after }
        }
        Ok(r) => Attempt::Done(r),
        Err(e) if e.is_timeout() || e.is_connect() => Attempt::Retry { err: e.into(), after: None },
        Err(e) => Attempt::Fail(e.into()),
    }
}).await?;

// gRPC seam client — the adoption pattern: retry UNAVAILABLE/RESOURCE_EXHAUSTED,
// honour grpc-retry-pushback-ms, clone the request per attempt.
let resp = agent_retry::retry(&self.retry,
    |s: &tonic::Status| grpc_retry_decision(s),        // uses agent_retry::grpc
    || { let mut c = self.client.clone(); let r = req.clone(); async move { c.method(r).await } },
).await?;
```

## Configuration

Retry counts come from config; the schedule/jitter are the canonical defaults.

- Providers: `[provider] max_retries` (default `3`; `0` disables). Threaded into
  both the HTTP providers and the `= "grpc"` provider client.

## Adoption status

- ✅ **HTTP providers** (`agent-providers`): `complete` + `stream` route through
  `agent_retry::run`, honouring `Retry-After`. `[provider] max_retries` (default 3).
- ⬜ **gRPC seam clients** (`agent-grpc`: provider/memory/context/policy/search/
  repo/tool), **MCP** reconnect, **telemetry** writer, **git** fetch: adopt the
  `agent_retry` helpers (the gRPC clients via the `grpc_retry_decision` pattern
  above, retrying `UNAVAILABLE`/`RESOURCE_EXHAUSTED` and honouring pushback). This
  is a mechanical sweep tracked as a follow-up — **the classifier and driver are
  ready and fully tested**, so adoption is call-site wiring plus a fault-injection
  test per transport, not new retry logic.

The rule holds regardless of adoption status: **new code that retries MUST use
`agent-retry`.** The follow-up only migrates existing call sites.

## Tests

`agent-retry` is unit-tested exhaustively (policy ceiling/jitter bounds with pinned
randomness, `run`/`retry` attempt counting, HTTP status + `Retry-After`, gRPC codes
+ pushback). `agent-providers` adds a raw-TCP mock-server integration proving
`503→200` retries and `400` does not. A gRPC fault-injection test lands with the
gRPC-client wiring follow-up.
