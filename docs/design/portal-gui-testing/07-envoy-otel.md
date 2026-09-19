# 07 — Fully instrument the Envoy bridge (OTLP → ClickHouse)

The grpc-web **Envoy bridge is the one hop with zero telemetry today** — its config has
no `access_log` and no `tracing` (`nix/portal/default.nix:151-325`). So when a browser
call is slow we cannot tell whether the time is in the proxy/HTTP layer or the backend.
This increment instruments Envoy fully, streaming to the **existing ClickStack OTEL
collector** (OTLP `:4317`, already wired — `crates/agent-telemetry/src/otel.rs:20-33`,
[`docs/deployment-l2.md`](../../deployment-l2.md)), so every hop of `browser → envoy → gateway → seam` is a
single trace, and any slow call is traceable via SQL. It is **dual-purpose**: the same
instrumentation gives production observability of the bridge, not just tests.

## 1. Envoy access logs → ClickHouse (proxy-vs-backend latency)

Add an `envoy.access_loggers.open_telemetry` access log to each of the three HCM
listeners (gateway/sessions/fleet), shipping **structured OTLP log records** to the
collector with the fields that separate proxy time from upstream time:

| field | Envoy command operator | why |
|---|---|---|
| total | `%DURATION%` | request start → last response byte |
| response | `%RESPONSE_DURATION%` | first upstream byte time |
| request rx | `%REQUEST_DURATION%` | request body received |
| upstream handshake | `%UPSTREAM_HANDSHAKE_DURATION%` | connect/TLS to backend |
| method | `%REQ(:PATH)%` | the gRPC method |
| status | `%GRPC_STATUS%` / `%RESPONSE_CODE%` | outcome |
| flags | `%RESPONSE_FLAGS%` | e.g. `UF`/`UT` upstream failure/timeout |
| upstream | `%UPSTREAM_HOST%` | which backend |
| ids | `%REQ(X-REQUEST-ID)%`, trace id | correlation |

`DURATION − RESPONSE_DURATION` (and the upstream duration) localizes a slowdown to the
proxy vs the backend. These land in ClickStack's OTLP **logs** table with
`service.name="envoy-portal-bridge"`. Optionally a ClickHouse **materialized view
`envoy_access_latency`** projects the flat latency columns for the simple "envoy proxy
logs" SQL:

```sql
SELECT req_path, quantile(0.95)(duration_ms)          AS p95_total,
       quantile(0.95)(duration_ms - response_ms)      AS p95_proxy_overhead
FROM envoy_access_latency
WHERE ts > now() - INTERVAL 1 HOUR
GROUP BY req_path ORDER BY p95_total DESC;
```

## 2. Envoy OTel tracing (spans)

Enable the `envoy.tracers.opentelemetry` tracer per listener, exporting spans over
OTLP/gRPC to the same collector, decorating each span with the gRPC method as the
operation name. Envoy already speaks **W3C trace-context**, matching the propagator the
agent installs (`crates/agent-telemetry/src/otel.rs:115`), so the Envoy span stitches
the browser action to the server `grpc.server` span
(`crates/agent-grpc/src/server/mod.rs:131-145`) under **one `trace_id`**. Sample at
**100 %** for this test/dev bridge (trace every call); a production deployment can lower
it.

## 3. Latency headers the harness reads directly

Envoy adds `x-envoy-upstream-service-time` (backend ms) on responses. Exposing it lets
Layer B read it and record `grpc_upstream_ms` vs its own client-perceived
`grpc_client_ms` → proxy+network overhead = `client − upstream`, attributed **without
even querying ClickHouse** ([`06`](06-performance.md)). The access-log table remains the
authoritative breakdown.

## 4. CORS must allow trace context through (concrete config change)

The three CORS policies in the envoy config must be widened or the browser can't
propagate/read trace context:

- **`allow_headers`** (`nix/portal/default.nix:175,212,249`) — add
  `traceparent,tracestate,x-request-id` so the browser/harness can send trace context.
- **`expose_headers`** (`nix/portal/default.nix:177,214,251`) — add
  `x-envoy-upstream-service-time` (and the trace headers) so the client can read them.

Optionally instrument the Dart grpc-web client to inject `traceparent` so the **browser
is the trace root**; otherwise Envoy starts the trace at the bridge.

## 5. The harness emits its own OTLP spans

A `portal-gui-test` tracer wraps each test/step in a span that is the **parent** of the
Envoy → gateway → seam spans, and stores the `trace_id` in the `portal_gui_perf` row and
the [report](05-report.md) record. Result: **any slow sample is one SQL join from its
full cross-hop trace** — `SELECT … WHERE duration_ms > N` on the traces table, or join
`portal_gui_perf.trace_id` → the ClickStack traces table:

```sql
-- every span of the slowest interaction this run, in order
SELECT span_name, service_name, duration_ms
FROM otel_traces
WHERE trace_id = (
  SELECT argMax(trace_id, value_ms) FROM portal_gui_perf
  WHERE run_id = {run_id:String} AND metric = 'interaction_ms')
ORDER BY start_time;
```

## Scope / verification

The change is confined to the envoy config generated in
[`nix/portal/default.nix`](../../../nix/portal/default.nix) (access_log + tracing stanzas
+ the CORS header additions), plus the optional Dart-client `traceparent` injection and
the `envoy_access_latency` materialized view. The collector runs on host loopback and
envoy runs `--network host`, so `localhost:4317` is reachable. Verify by driving one
call through the bridge and confirming (a) an `envoy-portal-bridge` access-log row and
(b) an Envoy span sharing the gateway span's `trace_id` both land in ClickHouse.

Increments [04](04-layer-b-e2e.md) (curated span assertion, trace linking) and
[06](06-performance.md) (the proxy-vs-backend split) lean on this.
