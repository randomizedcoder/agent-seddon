# gRPC seams & distributed tracing

How agent-seddon components talk to each other **across processes and machines** —
the protobuf wire contracts, the per-seam gRPC transport pattern, and the
OpenTelemetry tracing that follows a request across every hop into the ClickStack
collector.

> **Status.** All **shipped**: the schemas + generated stubs + conversions
> ([`agent-proto`](../crates/agent-proto)), the OTLP tracing layer
> ([`agent-telemetry`](../crates/agent-telemetry)), and the per-seam gRPC
> **servers + clients over TCP and unix domain sockets**, the `= "grpc"` config
> selection, and the `agent --serve-<seam>` binaries ([`agent-grpc`](../crates/agent-grpc)).
> Nothing here changes the agent loop.

## Why

Today every seam runs in-process as `Arc<dyn Trait>` (see
[`architecture.md`](architecture.md)); the only cross-process boundary is MCP. To
scale the harness onto a cluster — a central **model gateway**, a shared **memory
service**, sandboxed **tool workers**, all as separate containers in k8s — we need
two things:

1. **Explicit, versioned contracts** between components. Protobuf/gRPC makes the
   wire shape unambiguous and language-agnostic, and gives us streaming + status
   codes for free.
2. **Distributed tracing.** OpenTelemetry spans that follow one request across
   component boundaries and land as a single end-to-end trace in ClickStack, so a
   slow turn can be attributed to the gateway, a tool worker, or the memory
   service.

The design leans entirely on the existing seam architecture: a *remote* provider,
tool, memory, context, or policy is **just another impl of the same `agent-core`
trait**, selected by config. The loop (`agent-runtime/src/agent.rs`) only ever
talks to traits, so it is untouched.

## The wire contract — `agent-proto`

[`crates/agent-proto`](../crates/agent-proto) is the language-agnostic mirror of the
`agent-core` "message currency". Layout (package `agent.v1`):

| File | Contents |
|------|----------|
| `proto/agent/v1/common.proto` | The shared types: `Role`, `Message`, `ToolCall`, `ToolSchema`, `Observation`, `ToolContext`, `ModelCapabilities`, `Usage`, `CompletionRequest/Response/Chunk`, `MemoryItem`, `RecallQuery`, `MemoryEvent`, `ContextBlock`, `ContextInput`, `WorkingSet`, `TokenBudget`, `Decision`, plus `JsonValue` (binary arbitrary-JSON). |
| `provider.proto` | `service Provider` — `Capabilities`, `Complete`, `Stream` (server-streaming). |
| `tool.proto` | `service ToolService` — `DescribeAll`, `Execute`. |
| `memory.proto` | `service Memory` (facade) + `service Episodic` + `service Semantic`. |
| `context.proto` | `service ContextService` — `Assemble`, `Compact`. |
| `policy.proto` | `service Policy` — `Authorize`. |
| `search.proto` | `service SearchService` — `Status`, `Capabilities`, `Reindex` (server-streaming), `Search`. A `backend` selector routes to a named backend (empty ⇒ default). |
| `repo.proto` | `service RepoService` — object reads (`Resolve`, `ReadFile`, `ListTree`, `Diff`, `Grep`, `Log`, `Branches`) + lifecycle (`Status`, `Fetch`, `WorktreeAdd/List/Remove`, `CreateCheckpoint`, `Push`). Oids/revisions ride as strings. |

`tonic-build` (invoked from `build.rs`, needs `protoc` — pinned in `nix/`) generates
client + server stubs into `OUT_DIR`, re-exported as `agent_proto::pb`.

### Mapping decisions (fixed)

- **Arbitrary JSON** (`serde_json::Value` in `ToolCall.arguments`,
  `ToolSchema.parameters`) travels as `JsonValue` — a **fully-binary** recursive
  message (a `oneof` over null/bool/int64/uint64/double/string/array/object). It is
  deliberately **not** `google.protobuf.Struct`, whose spec forces every number to
  `double` and so loses 64-bit integer range: dedicated `int_value`/`uint_value`
  arms keep integers exact, and a `big_number` decimal-string arm is the
  arbitrary-precision escape hatch. Binary on the wire *and* lossless — no JSON text
  anywhere in the transport. An unset value decodes to `Value::Null`.
- **Optionals** (`tool_call_id`, `usage`, `iter`, `finish_reason`) use proto3
  `optional`. Singular message fields are `Option<T>` in prost, so a required one
  that's absent converts to `ConvertError::MissingField`.
- **`Decision`** → `{ bool allowed; optional string deny_reason; }` (message form,
  not a bare bool, to leave room for structured reasons).
- **`ContextService`** is named with the `Service` suffix so the generated type
  doesn't collide with `std::task::Context` in tonic's Tower impls. (`ToolService`
  likewise.)

### Conversions — direction matters

`agent-core` is the source of truth and **never depends on proto**; all bridging
lives in [`agent-proto/src/convert.rs`](../crates/agent-proto/src/convert.rs),
preserving the acyclic seam graph:

- Outbound `core → proto` is infallible: `impl From<agent_core::T> for pb::T`.
- Inbound `proto → core` is fallible: `impl TryFrom<pb::T> for agent_core::T`,
  because the wire can carry an unset enum, an absent required message, or
  malformed JSON — see `ConvertError`.
- `status_from_error(&agent_core::Error) -> tonic::Status` maps a failed local call
  onto a gRPC status (see the table below).

Round-trip tests (`core → proto → core → proto`, asserting proto equality) cover
every shared type.

## The transport — `agent-grpc`

Each seam gets **two** thin pieces in [`agent-grpc`](../crates/agent-grpc)
(`src/server.rs`, `src/client.rs`), following the MCP blueprint (`agent-mcp` +
`--serve-mcp`):

**Client** — a `Grpc<Seam>` type (`GrpcProvider`, `GrpcMemory`, `GrpcContext`,
`GrpcPolicy`, `GrpcSearch`, and `grpc_tools()` for a remote tool worker) that implements the
`agent-core` trait by calling a remote server, converting via `agent-proto` and
mapping `tonic::Status` → `agent_core::Error`. Channels are built **lazily**
(`Endpoint::connect_lazy`) so the runtime's *synchronous* seam factories can
construct a client without `await`.

**Server** — a `<Seam>Service` (`ProviderService`, `ToolWorker`, `MemoryService`
(+ `EpisodicService`/`SemanticService`), `ContextSvc`, `PolicySvc`, `SearchServiceSvc`, `RepoServiceSvc`) that wraps a
locally-built `Arc<dyn Trait>` and implements the generated tonic service, mapping
errors via `status_from_error`. The `*_router` helpers return a ready-to-serve
`Router`.

### TCP and unix domain sockets

`transport::Endpoint` covers both, parsed from a string: `unix:/path` ⇒ UDS,
otherwise TCP (`host:port` or `http://…`). UDS is the fast path when components
share a host — it bypasses the TCP/IP stack on a known socket path. The same
`Endpoint` dials (client, lazily) and binds (`serve_with_incoming` over a
`TcpListenerStream` / `UnixListenerStream`; a `SocketGuard` unlinks the socket on
shutdown).

**UDS security.** `bind` creates the parent dir `0o700` and sets the socket
`0o600`, so only the owner UID can connect (on Linux, connecting to a UDS requires
write permission on the socket) — an unauthenticated local peer can't invoke, say,
`tools.Execute`. On a multi-user host, prefer a per-user runtime dir over shared
`/tmp` (`listen = "unix:$XDG_RUNTIME_DIR/agent-seddon/<seam>.sock"`). For isolation
*across* UIDs, add a `SO_PEERCRED` check or mTLS on the TCP transport (a follow-up).

### Default ports & sockets (generated)

`nix/constants.nix` is the single source of truth; `nix run .#gen-constants`
renders it into the committed `crates/agent-grpc/src/constants.rs`, and the
`constants-sync` flake check fails on drift.

| Seam | TCP port | UDS path |
|------|----------|----------|
| provider | 50051 | `/tmp/agent-seddon/provider.sock` |
| memory | 50052 | `/tmp/agent-seddon/memory.sock` |
| tools | 50053 | `/tmp/agent-seddon/tools.sock` |
| context | 50054 | `/tmp/agent-seddon/context.sock` |
| policy | 50055 | `/tmp/agent-seddon/policy.sock` |
| search | 50056 | `/tmp/agent-seddon/search.sock` |
| repo | 50057 | `/tmp/agent-seddon/repo.sock` |

### Selection is config, exactly like every other seam

A remote seam is the `"grpc"` factory in `register_builtins` (feature `grpc`),
reading its endpoint from `[grpc]` — the same string-selected registry described in
[`extending.md`](extending.md). Empty endpoint ⇒ `127.0.0.1:<default port>`; set
`unix:/path` for the socket:

```toml
[agent]
provider = "grpc"                    # -> GrpcProvider

[grpc.provider]
endpoint = "unix:/tmp/agent-seddon/provider.sock"   # same-host, TCP-bypassing
# endpoint = "http://model-gateway:50051"           # cross-host
```

…and likewise `context = "grpc"`, `policy = "grpc"`, `[memory] backend = "grpc"`,
`[search] backends = ["grpc"]`, and `[grpc.tools] endpoint` for a remote tool
worker. No loop changes.

### Serve binaries

Counterparts to `--serve-mcp` (`agent-cli/src/grpc_server.rs`), one per seam, each
hosting the config-selected concrete impl over gRPC (config picks e.g.
`provider = "anthropic"`; the serve process exposes it as a gateway):

```
agent --serve-provider --config gateway.toml        # binds [grpc.provider] listen
agent --serve-memory   --listen 0.0.0.0:50052       # or override the address
agent --serve-tools ; agent --serve-context ; agent --serve-policy ; agent --serve-search
agent --serve-repo ; agent --serve-session ; agent --serve-scanner
agent --serve-reference ; agent --serve-scheduler
agent --serve-tokenizer ; agent --serve-embed
```

### One process, every seam — `--serve-all`

Distributing every seam as its own process means one process, one port and one
scrape target *per seam*. That is the right shape across hosts and the wrong one
on a single box, so `--serve-all` hosts **every enabled seam's service on one
endpoint** (default `127.0.0.1:50058`, `[grpc.gateway] listen` to override):

```
agent --serve-all --listen unix:/tmp/agent-seddon/gateway.sock
```

Clients are unchanged: a `= "grpc"` seam dials its own service by name, and
several seams pointed at the same endpoint just work. Seams whose impl is
disabled in this build/config are **skipped with a warning** rather than failing
the process — a gateway that refuses to start because one optional seam is off
would be useless.

Internally this is the same code path as `--serve-<seam>`: both fold seam
services onto the router returned by `server::base_router()`, so the one-seam and
all-seams paths cannot drift.

### Health checking

Every seam process serves the standard **`grpc.health.v1.Health`** service, so a
k8s `grpc` probe, `grpcurl grpc.health.v1.Health/Check`, or any off-the-shelf
balancer works with no agent-specific knowledge:

```sh
grpcurl -plaintext localhost:50055 grpc.health.v1.Health/Check
grpcurl -plaintext -d '{"service":"agent.v1.Policy"}' localhost:50055 grpc.health.v1.Health/Check
```

Both the **empty** service name (the protocol's "server as a whole", and what
k8s' optional `grpcService` field defaults to) and each **fully-qualified** seam
service name are reported, because probes disagree about which to ask for and
answering only one makes the other silently fail.

> **What SERVING claims, precisely.** That the process is up and *that seam's
> adapter is wired* — bound transport, built `Arc<dyn Trait>`, service added to
> the router. It does **not** claim the backing impl is healthy: no seam trait has
> a readiness method, so a `--serve-search` with a corrupt index still reports
> SERVING. The narrow claim is deliberate. Health that quietly means less than a
> reader assumes is worse than none, because it gets wired into failover.
> Widening it needs a readiness method on the seam traits; `HealthHandle` is where
> that signal would be flipped.

A seam that was *not* added never reports SERVING — that is what makes
`--serve-all`'s skip path safe, and it has a regression test.

> **Reflection lists the schema; health lists what is running.** Every process
> registers the *whole* descriptor set, so `grpcurl … list` shows every seam
> service the project defines — including ones this process does not host (and
> `agent.v1.Episodic` / `agent.v1.Semantic`, which nothing hosts yet). Calling
> one of those returns `UNIMPLEMENTED`. To ask what is actually being served, use
> the health service, not reflection.
>
> `grpc.health.v1`'s own descriptor is registered alongside the agent's for
> exactly this reason: a reflection-based client resolves a method through
> reflection *before* calling it, so a service absent from the descriptor set is
> invisible to `grpcurl` even while it answers generated clients perfectly well.

### Streaming & errors

- **Streaming.** `Provider::Stream` is server-streaming; the client maps the tonic
  stream item-by-item through `TryFrom<pb::CompletionChunk>` into agent-core's
  `ChunkStream`. Backpressure is tonic/HTTP-2 flow control.
- **Error mapping** (`status_from_error`):

  | `agent_core::Error` | gRPC `Code` |
  |---|---|
  | `Provider` / `Tool` / `Memory` / `Search` | `Internal` |
  | `Config` | `InvalidArgument` |
  | `Io` | `Unavailable` |
  | `Json` (and any `ConvertError`) | `InvalidArgument` |

## Distributed tracing → ClickStack

OTLP tracing is **shipped** and additive to the ClickHouse-native sink. Enable it
with a non-empty `[telemetry] otlp_endpoint` (see [`config/agent.toml`](../config/agent.toml)).
For a runnable end-to-end demo (ClickStack container + a two-process distributed
trace) see **[`tracing.md`](tracing.md)**.

- [`agent-telemetry::otlp_layer`](../crates/agent-telemetry/src/otel.rs) builds a
  batch `TracerProvider` that exports spans over OTLP/gRPC to the ClickStack OTEL
  collector, returned as a `tracing` layer composed alongside `ClickHouseLayer` in
  `agent-cli/src/main.rs`. It also installs the global **W3C trace-context**
  propagator.
- [`agent-proto::trace`](../crates/agent-proto/src/trace.rs) provides
  `inject_context` / `extract_context` over tonic metadata (the `MetadataInjector` /
  `MetadataExtractor` adapters).

**Where the transports wire it in:** each gRPC **client** injects the current
context into request metadata (`client.rs`'s `outbound()`); each gRPC **server**
extracts it and `set_parent`s the handler's span on it (`server.rs`'s `span()`). The
collector then stitches gateway → tool-worker → memory-service spans into one trace.

## Deployment sketch (k8s)

```
             ┌──────────────┐        ┌────────────────────┐
   goal ───▶ │  agent (loop)│──gRPC─▶│  model-gateway     │──▶ LLM API
             │  Deployment  │        │  (--serve-provider)│
             └──────┬───────┘        └────────────────────┘
                    │ gRPC                    │ OTLP
       ┌────────────┼───────────────┐         ▼
       ▼            ▼               ▼   ┌──────────────────────────┐
 ┌───────────┐ ┌──────────┐ ┌──────────┐│ ClickStack OTEL collector│──▶ ClickHouse
 │tool-worker│ │  memory  │ │  policy  ││  (OTLP :4317)            │
 │--serve-…  │ │--serve-… │ │--serve-… │└──────────────────────────┘
 └───────────┘ └──────────┘ └──────────┘        ▲ every component exports here
```

Each seam is an independently-scalable `Deployment` + `Service`; tool workers can be
sandboxed and horizontally scaled; the memory service is shared cluster-wide. This
lifts the "multi-user serving / distributed subagents" non-goal in
[`DESIGN.md`](../DESIGN.md) §1 — the seam boundaries were always the plan; gRPC just
makes them network-addressable.

## Introspection — reflection + `grpcurl`

Every `--serve-<seam>` process enables **gRPC server reflection** (both the `v1`
and `v1alpha` services, for broad client compatibility), so you can list, describe,
and *call* a seam with human-readable JSON using [`grpcurl`](https://github.com/fullstorydev/grpcurl)
(pinned in the dev shell) — no `.proto` files on hand:

```sh
agent --serve-search &                      # search seam on 127.0.0.1:50056

grpcurl -plaintext 127.0.0.1:50056 list                        # → agent.v1.SearchService, …
grpcurl -plaintext 127.0.0.1:50056 describe agent.v1.SearchService
grpcurl -plaintext -d '{"globs":["**/*.rs"]}' \
        127.0.0.1:50056 agent.v1.SearchService/ListFiles       # JSON in → binary → JSON out
```

The reflection descriptor is a `FileDescriptorSet` emitted by `agent-proto`'s
`build.rs` and exposed as `agent_proto::FILE_DESCRIPTOR_SET`; `agent_grpc::server::with_reflection`
registers it on a seam's `Router` (a unit test asserts it carries every seam
service). The JSON you send maps onto the typed proto fields — note that tool
arguments ride the custom `JsonValue` message (which preserves i64/u64 exactly and
escapes arbitrary-precision numbers via its `big_number` field), *not*
`google.protobuf.Struct`.

## Testing

`crates/agent-grpc/tests/roundtrip.rs` exercises **every seam over both TCP and
UDS** (a table-driven `#[case::tcp]` / `#[case::uds]`): each test binds a real
server on an ephemeral port or a temp-dir socket, connects the client, and asserts
the round-trip (including Provider server-streaming). `transport.rs` unit-tests
the `unix:`/TCP endpoint parsing.

`tests/common/mod.rs` holds the harness both test files share — `Transport`,
`spawn(transport, router)`, and a `TestServer` that shuts down and unlinks its
socket on drop. It lives there rather than in `agent-testkit` so testkit's other
consumers don't all pull in `agent-grpc`; a new seam's test file gets a real
server on both transports for one `use`.

`tests/gateway.rs` covers the transport-level properties rather than any one
seam: health reporting (serving, draining, and the NOT_FOUND an unhosted service
must give) and one router hosting several seams at once.

## Adding a seam to the wire

The per-seam work is mechanical; the shared pieces — `transport.rs`, reflection,
health, trace propagation, the retry classifier, `status_from_error`, and the
test harness — are written once and are not touched again.

1. `proto/agent/v1/<seam>.proto`, then one line in `agent-proto/build.rs` and one
   in the descriptor-set test in its `lib.rs`.
2. `From`/`TryFrom` pairs in `agent-proto/src/convert.rs` (usually the largest
   part; enums get a saturating `*_from_i32` helper rather than a fallible
   conversion, so an unknown wire value degrades instead of erroring).
3. `agent-grpc/src/server/<seam>.rs` — the service impl and its `*_router`.
4. `agent-grpc/src/client/<seam>.rs` — the core trait implemented over the wire.
5. A row in `nix/constants.nix` and a line in `nix/gen-constants.nix`, then
   `nix run .#gen-constants`.
6. A `GrpcSeamCfg` field in `config.rs`, a `"grpc"` factory in `registry.rs`, and
   a `SEAMS` row in `agent-cli/src/grpc_server.rs`.
7. Round-trip tests over both transports.

Additive proto changes (a new service, RPC, or field) pass `buf breaking`
untouched, so a new seam needs **no** `buf.image.binpb` bump.

Three things are judgement, not mechanics:

- **Sync trait methods can't round-trip.** Metadata accessors (`capabilities()`,
  `name()`) are answered from a config-derived value cached at connect time — see
  `GrpcProvider`. A seam whose *primary* operation is sync can't be distributed
  without making the trait async.
- **Streams bypass retry.** A partial stream can't replay, so `call_retry` wraps
  unary calls only.
- **The failure semantic is per-seam and deliberate.** `Policy` fails *safe*
  (deny), `Tool` fails *soft* (an error observation), `Search`/`Repo`/`Session`
  fail *hard* (`Err`), and `Scanner` fails **open** — the one place "fail closed"
  is wrong, because its trait has no error channel and a scanner that denied
  every call when its backend blinked would be an availability weapon. Copying
  the wrong one from a neighbouring seam silently changes behaviour under
  partition — pick it, don't inherit it.

  Fail-open needs a compensating control, or the failure is invisible: the
  scanner client emits a `WARN` (`scanner.transport_failed`) on every transport
  failure, and that log is the only signal that scanning has stopped happening.
  `ReferenceResolver` degrades the same way — an outage becomes a warning and an
  unexpanded prompt, and deliberately does **not** set `blocked`, which means
  "refused on purpose".

## Serving a seam is not always the same as *using* one remotely

`--serve-scheduler` hosts the job registry, so a remote client can schedule,
list, cancel and inspect history. But there is deliberately **no**
`[scheduler] backend = "grpc"`: firing a job needs `tick_with`, which takes the
executor closure and is not on the `Scheduler` trait, because a job's executor
*is* the agent. A remote registry can therefore be **managed** remotely but only
**driven** by the process that owns it.

Wiring a config backend anyway would produce a scheduler that accepts jobs and
silently never fires them — precisely the failure mode the scheduler's design
goes out of its way to prevent. Distributed *driving* needs a richer protocol
(claim a due job, run it, report the outcome), which is a feature rather than a
wiring line, and is deferred as such.

## Possible follow-ups

- An example / compose file running the loop against a separate `--serve-provider`
  gateway process end-to-end.
- Distributing the memory layers independently via the shipped
  `EpisodicService` / `SemanticService` (the CLI currently serves the `Memory`
  facade).
- TLS / mTLS on the TCP transport for cross-host trust.
