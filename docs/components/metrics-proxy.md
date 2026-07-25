# Metrics proxy — the `MetricsProxy` seam

A generic **PromQL-over-gRPC** proxy to a Prometheus HTTP API, so a **gRPC-only**
client (the [portal](../design/portal/README.md)) can read the same series Grafana
does — reusing any panel query verbatim, with no getter minted per number. The loop
does not consume it; it is a portal-facing read surface.

- **Trait:** `agent_core::MetricsProxy` ([`agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs))
- **Impl crate:** [`agent-metrics-proxy`](../../crates/agent-metrics-proxy) (`HttpMetricsProxy`)
- **Cargo feature:** `metrics-proxy` (default on)
- **Service:** `agent --serve-metrics-proxy` (`agent.v1.MetricsProxyService`) — port
  `50079` (`nix/constants.nix`); reflection- + health-introspectable.

## The trait

```rust
#[async_trait]
pub trait MetricsProxy: Send + Sync {
    async fn query(&self, q: &PromQuery) -> Result<PromResult>;            // instant
    async fn query_range(&self, q: &PromRangeQuery) -> Result<PromResult>; // [start,end,step]
}
```

`PromResult` mirrors Prometheus' JSON envelope — a `result_type`
(`vector`/`matrix`/`scalar`), a set of labelled `PromSeries` (each a `{labels,
samples}`), and a class-only `error`. The client sends **PromQL strings**; the
portal keeps a small canned library, e.g.:

```promql
histogram_quantile(0.99, sum(rate(agent_provider_request_seconds_bucket[5m])) by (le))
agent_context_tokens
```

## Fail-soft, read-only, capped

The seam **never surfaces a transport `Err`** and never panics: an oversized query,
an unreachable/slow upstream, or a malformed response all fold into an empty
`PromResult` with a class-only `error`. It only ever *reads* Prometheus.

The PromQL string is untrusted, so `HttpMetricsProxy` **fails closed** on size:
query length (`MAX_QUERY_LEN`), series count (`MAX_SERIES`), and per-series sample
count (`MAX_SAMPLES`) are all capped before buffering (excess dropped + logged), the
upstream request is time-bounded (`UPSTREAM_TIMEOUT_SECS`), and a raw upstream error
body is never forwarded — the `error` carries only Prometheus' `errorType` class.
The JSON→`PromResult` translation (`parse_prom_response`) is a pure function,
unit-tested against canned envelopes including error/oversized/malformed cases; the
wire round-trip is asserted on TCP + UDS.

## Config

```toml
[metrics_proxy]
prometheus_url = "http://127.0.0.1:9090"   # Prometheus HTTP API base
```

## Adding your own

Implement `MetricsProxy` (e.g. against a different TSDB's query API) and select it.
`HttpMetricsProxy` is the reference Prometheus impl. See
[`extending.md`](../extending.md) and [`design/portal/`](../design/portal/README.md).
