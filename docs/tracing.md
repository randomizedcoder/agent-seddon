# Distributed tracing → ClickStack (HyperDX)

A runbook for exporting the agent's OpenTelemetry traces to a **ClickStack /
HyperDX** collector and seeing the agent interacting with its components — including
a **distributed trace that spans two processes** (the loop + a `--serve-provider`
model gateway).

## What you get

The loop is instrumented (`agent-runtime/src/agent.rs`) so one run is a span tree:

```
agent.turn
├─ memory.recall
├─ context.assemble
├─ provider.stream ──▶ grpc.server        (agent-loop → agent-provider-gateway)
├─ policy.authorize
├─ tool.execute
├─ context.compact
└─ provider.stream ──▶ grpc.server        (next iteration)
```

With `provider = "grpc"`, the `provider.*` calls cross a process boundary and the
gateway's `grpc.server` span is a **child** of the loop's `provider.stream` span —
one trace, two services (`agent-loop`, `agent-provider-gateway`).

## Run it

Prereqs: Docker running; a working provider (the example uses the GLM endpoint from
`config/agent.toml`).

**1. Start ClickStack** (HyperDX all-in-one: OTLP collector + ClickHouse + UI):

```sh
nix run .#clickstack-up          # UI :8080, OTLP gRPC :4317, OTLP HTTP :4318
```

**2. Onboard + get the ingestion key.** Open <http://localhost:8080>, create a
local account. HyperDX then activates the collector's OTLP receiver (until a team
exists it runs a no-op pipeline and `:4317` isn't even bound) and authenticates
OTLP with an **Ingestion API Key**. Copy it from **Team Settings** and set it in
both demo configs:

```toml
# config/otel-demo/gateway.toml  and  loop.toml, under [telemetry]:
otlp_headers = "authorization=<ingestion-key>"
```

**3. Start the model gateway** (serves the real provider over gRPC on `:50051`):

```sh
nix run .#agent -- --serve-provider --config config/otel-demo/gateway.toml
```

**4. Run the loop** (in another shell; `provider = "grpc"` dials the gateway):

```sh
nix run .#agent -- --config config/otel-demo/loop.toml \
  "list the crates directory then read Cargo.toml and name the workspace members"
```

**5. View it** — open <http://localhost:8080> → Search → Traces, and open the
`agent-loop` trace to see the waterfall (loop spans + the gateway `grpc.server`
spans nested under `provider.stream`).

## Verify from ClickHouse (no UI)

```sh
# spans by service + name
nix run .#clickstack-client -- -q "SELECT ServiceName, SpanName, count() n \
  FROM default.otel_traces GROUP BY ServiceName, SpanName ORDER BY 1,2 FORMAT PrettyCompact"

# one trace spanning BOTH processes (the distributed proof)
nix run .#clickstack-client -- -q "SELECT TraceId, groupUniqArray(ServiceName) services, count() spans \
  FROM default.otel_traces GROUP BY TraceId HAVING length(services) > 1 \
  ORDER BY max(Timestamp) DESC LIMIT 1 FORMAT PrettyCompact"
```

Verified output looks like (a single `TraceId`, both services):

```
TraceId                            services                                   spans
039c65a4da7ca72d822c63367f4c05b9   ['agent-loop','agent-provider-gateway']    12
```

`nix run .#clickstack-down` tears it down (data discarded).

## Gotchas (learned the hard way)

- **Use `127.0.0.1`, not `localhost`, in `otlp_endpoint`.** The container maps
  `4317` on IPv4 only; `localhost` may resolve to IPv6 `::1` and fail to connect.
- **The collector needs onboarding.** Before you create a HyperDX account, the
  bundled collector runs a `nop` OTLP pipeline and doesn't bind `:4317`. Creating an
  account activates it.
- **OTLP requires the ingestion key** (`authorization` header) once onboarded.
- **Traces are independent of `RUST_LOG`.** The OTLP layer has its own `INFO` filter
  (`agent-cli/src/main.rs`), so `RUST_LOG=warn` quiets the console without silently
  disabling tracing — the spans are `info_span!` and would otherwise be filtered out
  before any layer saw them.
- Only the **provider** is remote here (per the demo). Serve `--serve-memory`,
  `--serve-tools`, etc. the same way for a fuller distributed picture.
