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
*across* UIDs, use mTLS on the TCP transport (below) or a `SO_PEERCRED` check (a
follow-up).

### TLS and mTLS

The TCP transport speaks TLS 1.2/1.3 (tonic's rustls, `ring` provider) configured
by `[grpc.tls]` (security-hardening S4,
[design](design/security-hardening/07-transport-tls-and-pki.md)).

- **Which dials use TLS is the address's call.** `https://host:port` ⇒ TLS;
  `http://host:port` and bare `host:port` ⇒ plaintext (back-compat); `unix:` ⇒
  never TLS. Before S4 an `https://` was stripped and dialed plaintext — a silent
  downgrade that is now gone.
- **Server** — `[grpc.tls] cert` + `key` (PEM, leaf first): every `--serve-*`
  process listening on TCP serves TLS, including `--serve-all`, `--serve-fleet` and
  `--serve-sessions`. `client_ca` makes it **mutual**: a client without a
  certificate chaining to that CA is refused in the handshake. A unix-socket listener
  stays plaintext (its boundary is the `0600` socket). A `listen = "https://…"` with
  no cert is a startup error, not a plaintext listener. Startup logs
  `transport = plaintext | tls | mtls` per listener.
- **Client** — `[grpc.tls.client]`: `ca` is the **only** trust anchor when set (the
  public web roots are not mixed in, so no public CA can mint a certificate for an
  internal seam); empty ⇒ the webpki roots. `cert` + `key` present a client
  certificate for mTLS. `domain` overrides the name the server certificate must
  carry (e.g. dialing an IP whose certificate only names `agent`). It is installed
  process-wide at startup, so every `= "grpc"` seam client picks it up.
- **Files** are read at startup, capped at 1 MiB, must be PEM, and are parsed then —
  a bad file fails the start, not the first handshake. A private key readable by
  group/other logs a warning. Unknown keys under `[grpc.tls]` are errors (a
  misspelt `client_ca` would otherwise silently turn mTLS off).

**Certificates.** `nix run .#pki-dev` mints an offline development PKI with
smallstep's `step certificate create` (no `step-ca` daemon, no network) into
`$XDG_RUNTIME_DIR/agent-seddon/pki` (or `--out`): a P-256 root CA, a
`token-signer` key for the agent-token service (S5), and one leaf per `--service`
(default `agent`, `cli`) with SANs `localhost`, `127.0.0.1`, `::1`, the name and
`spiffe://agent.<deployment>/svc/<name>`, EKU server + client auth. It prints the
matching `[grpc.tls]` block; `--verify` checks every leaf chains to the root;
re-runs keep what exists, `--force` regenerates. Production brings its own CA —
the agent only needs PEM files.

```sh
nix run .#pki-dev                                   # mint + print the config block
grpcurl -cacert "$XDG_RUNTIME_DIR/agent-seddon/pki/ca/root.crt" \
  -cert "$XDG_RUNTIME_DIR/agent-seddon/pki/cli/cert.pem" \
  -key  "$XDG_RUNTIME_DIR/agent-seddon/pki/cli/key.pem" \
  127.0.0.1:50100 grpc.health.v1.Health/Check
```

**Tested by** the `crates/agent-grpc/tests/tls.rs` wire matrix (in-memory CA from
`agent_testkit::pki`: TLS, mTLS, bring-your-own-CA, expired / not-yet-valid /
other-CA / wrong-name server certs, a client cert from another CA, no client cert,
plaintext against TLS, bare `host:port` staying plaintext, UDS unaffected), the
`pki-dev-tests` check (real step-cli, offline, with check-the-checks), and the
`tls` / `mtls` rows of `nix run .#serve-smoke`. Mapping the peer certificate to a
service principal is covered below under **Service identity**.

**Plaintext refusal (S10).** With `[auth] mode = "oidc"`, a TCP listener that is
not loopback and has no `[grpc.tls]` refuses to start, because bearer tokens would
cross the network in clear. `[auth] allow_insecure_listen = true` overrides it, with
a warning at every start.

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
agent --serve-web ; agent --serve-web-search
agent --serve-sandbox ; agent --serve-pty      # see the warning below
agent --serve-forge ; agent --serve-tasks     # forge writes to the platform
agent --serve-lsp
agent --serve-episodic ; agent --serve-semantic   # the memory layers, individually
agent --serve-provider-registry   # the model-router fleet control plane (:50084)
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

The **`nix run .#serve-smoke`** app is the real-wire breadth probe of this surface:
it boots `--serve-all` over **TCP and UDS** and, via server reflection, asserts the
gateway reports `grpc.health.v1` SERVING, that every advertised seam can be
`grpcurl describe`d, that a CPU-only critical subset is present, and that two seams
(`Memory/Recall`, `TokenizerService/Count`) round-trip. Like `e2e-live` /
`loadtest-wire` it needs a running server + a socket, so it is an opt-in app, not a
hermetic `nix flake check` gate (it needs no model, though). See
[`nix/serve-smoke.nix`](../nix/serve-smoke.nix).

### Health checking

Every seam process serves the standard **`grpc.health.v1.Health`** service, so a
k8s `grpc` probe, `grpcurl grpc.health.v1.Health/Check`, or any off-the-shelf
balancer works with no agent-specific knowledge:

```sh
grpcurl -plaintext localhost:50055 grpc.health.v1.Health/Check
grpcurl -plaintext -d '{"service":"agent.v1.Policy"}' localhost:50055 grpc.health.v1.Health/Check
# over a unix socket: use the unix:// scheme (see Introspection below), not -unix
grpcurl -plaintext unix:///tmp/agent-seddon/gateway.sock grpc.health.v1.Health/Check
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
> service the project defines — including ones this process does not host.
> Calling one of those returns `UNIMPLEMENTED`. To ask what is actually being
> served, use the health service, not reflection.
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

## Session/user identity on the wire

Multi-session identity rides the **same two choke-points** as trace context, as gRPC
metadata (not a `.proto` field, so it is additive and `buf breaking` never sees it):

| Metadata key | Carries |
|---|---|
| `x-agent-session-id` | the session id (a server-minted UUID in the multi-user flow) |
| `x-agent-user-id` | the user id (`local` for the single-user CLI/REPL) |
| `authorization` | `Bearer <agent token>`: the caller's, forwarded; else this process's service token |
| `x-agent-hops` | how many agent services the request has already passed through |

- [`agent-proto::identity`](../crates/agent-proto/src/identity.rs) defines the key
  constants and `inject_identity` / `extract_identity` over tonic metadata.
- The ambient `(user, session)` for the current task is a dedicated
  [`tokio::task_local`](../crates/agent-grpc/src/identity.rs) (`AGENT_IDENTITY`) —
  **not** OpenTelemetry baggage, so it flows whether or not telemetry is configured
  (a security boundary must not depend on OTLP being on). `outbound()` injects it
  alongside trace context; `server::span()` extracts it, validates each segment with
  `agent_core::safe_segment`, and attributes the handler span (`session_id` /
  `user_id`). A server-as-client (`--serve-all`) forwards the caller's identity via
  the same task-local.
- **Credentials follow the call** (security-hardening S9). A served request's verified
  agent token is kept in the `AGENT_BEARER` task-local, and `outbound()` sends it on every
  downstream seam call, so the next seam authorizes the *caller*, not the service in the
  middle. With no caller token in scope (a scheduler job, background upkeep, a host with
  auth off dialling a seam that has it on), `outbound()` sends the process's own token
  from the installed `BearerSource` instead, and nothing when there is none. A caller's
  token is never swapped for the service's.
- **Hop count.** Every server computes its hop as the inbound `x-agent-hops` plus one
  (absent = a client, so the first server is hop 1) and `outbound()` stamps that count on
  the next call, replacing whatever was there. A value that is not a small ASCII number is
  `INVALID_ARGUMENT`; more than 4 is `FAILED_PRECONDITION`, so a forwarding loop stops.
  The count applies with auth on or off. A caller can hide hops made before it reached
  us, never the ones after.
- **Spawned work keeps its caller.** `tokio::spawn` inherits no task-local, so request
  work handed to a task runs under `agent_core::scope_request(RequestScope::current(), …)`.
  A test ([`no_unscoped_spawn.rs`](../crates/agent-grpc/tests/no_unscoped_spawn.rs)) fails
  the build on a spawn in the served crates that does neither that nor carry an
  `// unscoped-spawn: <reason>` comment.

> **Trust boundary (important).** Without `[auth] mode = "oidc"` these values are
> **attacker-controllable**. They are trusted only as routing/namespacing labels, and
> only as far as the transport (UDS file perms / loopback) already trusts the peer;
> `mode = "none"` on a routable address refuses to start unless
> `allow_insecure_listen = true`. Isolation is enforced *structurally* (per-tenant
> paths guarded by `safe_segment` + `confine`), so even a spoofed identity cannot
> escape the namespace it names. `safe_segment` also **bounds each segment's length**
> (`MAX_SEGMENT_LEN`), so an over-long id can't blow up a metric label or path. Under
> `mode = "oidc"` the auth layer overwrites `x-agent-user-id` with the token's verified
> tenant, and `agent_core::current_tenant()` prefers that verified tenant everywhere.

**Which calls must carry identity.** Each service has an identity class
([`identity_policy.rs`](../crates/agent-grpc/src/server/identity_policy.rs), kept equal to
the [mt-audit](components/mt-audit.md) manifest). While identity is enforced — always for a
call with a verified token, and for token-less calls when `[auth] require_identity` is on
(default: on for a routable listener, off for loopback and unix sockets) — a call to a
`scoped` or `single-store` service without a valid `x-agent-session-id` (and, without a
token, `x-agent-user-id`) gets `UNAUTHENTICATED("identity required")`, and a call to a
service with no class gets `PERMISSION_DENIED`. `field-scoped` services
(`SessionRegistryService.Open` is how a client gets a session), `stateless` and
`operator-global` services proceed on the tenant alone. Health and reflection are exempt.

**Who may sign tokens.** `[auth]` accepts tokens from any number of OIDC issuers
(`[[auth.issuers]]`, beside the original single-issuer `issuer` / `audience` / `jwks_url`
form, which acts as one issuer named `default`). Each issuer has a profile
([`auth/issuer.rs`](../crates/agent-grpc/src/server/auth/issuer.rs)): `google` takes the
tenant from the Workspace domain (`hd`, which must be in `allowed_domains`) and requires a
verified email; `entra` takes it from the directory id (`tid`, which must be in
`allowed_tenants`); `generic` takes it from a configured claim. A token is routed to its issuer
by `iss` before verification, so an unknown issuer costs no key fetch, and each issuer keeps its
own key cache, so one issuer's key never verifies a token that claims another. A `generic`
issuer without `jwks_url` finds its keys by OIDC discovery, and the discovery document must
name the same issuer. Roles are read from a token only with `trust_roles_claim = true` (the
single-issuer form keeps trusting its `roles_claim`, as before).

**Agent tokens.** With `[auth.token]` configured
([`auth/token.rs`](../crates/agent-grpc/src/server/auth/token.rs)), an IdP token is good for
one call only: `agent.v1.AuthService/Exchange`, which verifies it with the issuer rules above
and returns an agent-signed token (`ES256`, header `typ = at+jwt`, `kid` = the signing key's
RFC 7638 thumbprint). Its claims carry `tenant`, `sub = user:<issuer name>/<IdP subject>`,
`roles`, `amr`, and a `perms` snapshot (`"read:prompt"`, …; left out with `perms_ref = true`
past 40 entries). It expires at the earlier of `ttl_secs` (default 900) and the login token's
own `exp`. Every other RPC, on every seam, accepts only agent tokens: an IdP token presented
to a seam, or an agent token presented to `Exchange`, is `UNAUTHENTICATED`. `AuthService` is
served by any listener with `[auth.token]`; `Exchange` and `Jwks` (the public key set, for
Envoy or another process) are exempt from the bearer check, `WhoAmI` is not. After key
rotation, set `previous_key` to the old key for one `ttl_secs` so tokens it signed keep
verifying. The verified bearer is kept in the request scope (`agent_core::AGENT_BEARER`), and
`agent_core::scope_request` carries identity, principal and bearer across a `spawn`.

**Sessions.** `Exchange` also opens a sign-in session
([`auth/session.rs`](../crates/agent-grpc/src/server/auth/session.rs)). The agent token carries
its `sid`, and the response carries a `refresh_handle` plus the session's absolute expiry
(`session_ttl_secs`, default 12 h).

- `Refresh{refresh_handle}` needs no bearer. It returns a new token and a new handle; the old
  handle is retired, and presenting it again revokes the session.
- `Logout` revokes the caller's session.
- `ListMySessions` and `RevokeMySession` act on the caller's own sessions.
- `ListSessions` and `RevokeSession` need `read:binding` and `write:binding`, respectively.

**Role bindings** (S8, [`auth/binding.rs`](../crates/agent-grpc/src/server/auth/binding.rs)).
A binding grants roles in one tenant to a login subject (`sub`: `<issuer name>/<IdP sub>`), a
verified `email`, every verified email in a `domain`, or an `mtls_san` (a service's
certificate SAN). Roles are
resolved at `Exchange` and at every `Refresh`. They are the union of the trusted claim roles, the
tenant's active bindings that match, and `operator` for `[auth] operator_subjects`.

- `ListBindings` and `GetBinding` need `read:binding`.
- `PutBinding` needs `write:binding`, and `DeleteBinding` needs `delete:binding`.
- A tenant other than the caller's needs a host-global grant.
- Writes may not grant beyond the caller's own permissions, bind the caller, or remove the
  tenant's last binding that can manage bindings (`FAILED_PRECONDITION`).
- A delete, or a put that narrows a binding, revokes the sessions the old binding named
  (`revoke_reason = "binding"`) unless `keep_sessions` is set.
- Role-card writes on `RoleService` are host-global.

Revocation stops refresh straight away, and it stops sensitive RPCs (approve, exec, and role,
binding or config writes) within about 5 seconds. Other RPCs keep working until the token
expires. `[auth.token] session_store` picks where sessions persist: `memory`, `file` or
`postgres`.

```sh
RESP=$(grpcurl -d "{\"id_token\":\"$ID_TOKEN\"}" "$ADDR" agent.v1.AuthService/Exchange)
TOKEN=$(jq -r .accessToken <<<"$RESP"); HANDLE=$(jq -r .refreshHandle <<<"$RESP")
grpcurl -H "authorization: Bearer $TOKEN" "$ADDR" agent.v1.AuthService/WhoAmI
grpcurl -d "{\"refresh_handle\":\"$HANDLE\"}" "$ADDR" agent.v1.AuthService/Refresh
grpcurl -H "authorization: Bearer $TOKEN" -d '{"binding":{"id":"bob","subject_kind":"email",
  "subject":"bob@example.com","roles":["reviewer"]}}' "$ADDR" agent.v1.AuthService/PutBinding
grpcurl -H "authorization: Bearer $TOKEN" "$ADDR" agent.v1.AuthService/Logout
```

**Service identity** (S10, [`auth/mtls.rs`](../crates/agent-grpc/src/server/auth/mtls.rs)).
A service proves itself with its client certificate instead of a login.

- `[[auth.mtls.bindings]]` maps a certificate's URI SAN to `{service, tenant, roles}`.
- `Exchange{use_client_cert: true}` over an mTLS connection returns a token with:
  - `sub = svc:<service>` and `amr = ["mtls"]`;
  - a `cnf` claim holding the certificate's SHA-256 thumbprint (`x5t#S256`);
  - the binding's roles plus any matching `mtls_san` role bindings.
- There is no refresh handle: the service exchanges again before expiry.
- With `[auth.mtls] token_endpoint` set, the process does that itself: its service
  token becomes the S9 fallback bearer.
- A token with `cnf` is accepted only from a connection that presents that
  certificate, or from another bound service relaying it. Over plaintext, from Envoy
  or from an unbound client certificate it is `UNAUTHENTICATED`.
- The bound SAN of the connection is recorded as `peer_san` on the `grpc.server` span.

```sh
PKI="$XDG_RUNTIME_DIR/agent-seddon/pki"
grpcurl -cacert "$PKI/ca/root.crt" -cert "$PKI/fleet/cert.pem" -key "$PKI/fleet/key.pem" \
  -d '{"use_client_cert": true}' "$ADDR" agent.v1.AuthService/Exchange
```

**A person at a terminal** (S12, [`client/login.rs`](../crates/agent-grpc/src/client/login.rs),
[`agent-runtime/src/login.rs`](../crates/agent-runtime/src/login.rs)).

- `agent login [--issuer NAME] [--endpoint ADDR]` runs the OAuth device flow (RFC 8628) at the
  login issuer. It prints a URL and a code. Once the code is approved, it trades the ID token at
  `AuthService.Exchange` (`client_kind = "cli"`).
  - The issuer comes from `[[auth.issuers]]`: its `audience` is the OAuth client id, and
    `client_secret` is an `env:`/`file:` reference for IdPs that want one from a device client
    (Google).
  - The agent is `[grpc.client] auth_endpoint` or `--endpoint`. It must be `https://`, a
    loopback IP or `unix:`, because the ID token travels on it.
- The agent token and its refresh handle are kept in
  `$XDG_CONFIG_HOME/agent-seddon/tokens/<issuer>.json`:
  - The file is mode `0600` in a `0700` directory, written atomically.
  - A file others can read, a symlink or an oversized file is refused, not used.
- `agent whoami` prints tenant, subject, roles, permissions and session id; `agent logout`
  revokes the session and deletes the file.
- `[grpc.client] bearer = "login"` (or `"login:<issuer>"`) makes the stored login the process's
  outbound bearer (the S9 fallback). It is refreshed at two thirds of each lifetime.
  - The file is locked across a refresh: two `agent` processes sharing one login never both spend
    the rotating handle, which the server would take as theft.
  - `bearer = "env:VAR"` / `"file:/path"` sends a token issued elsewhere instead.
  - A process has one credential of its own: `bearer` and `[auth.mtls] token_endpoint` together
    are refused at load.

```sh
agent login --issuer google       # prints: Open https://www.google.com/device … code ABCD-EFGH
agent whoami
agent logout
```

**A person in a browser** (S13a, [`server/auth/code_flow.rs`](../crates/agent-grpc/src/server/auth/code_flow.rs)).
The portal signs in with the OAuth authorization-code flow and PKCE (RFC 7636). The agent
redeems the code, so a client secret (Google asks for one even with PKCE) never reaches the
browser.

- `[auth] redirect_uris` lists the exact URIs an IdP may send the browser back to (the portal's
  own address). Setting it turns browser sign-in on. Each must be `https`, or plain `http` to a
  loopback IP, with no fragment. Register the same URIs with the IdP.
- `AuthService.Issuers` lists the login issuers a browser can use (`name`, `profile`). An issuer
  that accepts several `iss` URLs (an Entra issuer spanning directories) is left out, because
  which one to discover is ambiguous.
- `AuthService.Begin{issuer, redirect_uri, code_challenge}` returns the IdP's authorization URL
  and a `state`.
  - The client keeps the PKCE verifier and sends only its S256 challenge.
  - The `state` is random, single use and good for 10 minutes. It binds the issuer, the redirect
    URI, the challenge and an OIDC `nonce`.
  - At most 1024 sign-ins can be in flight; beyond that `Begin` answers `RESOURCE_EXHAUSTED`.
- The IdP redirects to `redirect_uri?code&state`. The client then calls
  `AuthService.Exchange{code, state, code_verifier}` (`client_kind` defaults to `portal`). The
  agent:
  - spends the `state` (a second use is refused, whatever the first did);
  - checks the verifier against the challenge before contacting the IdP;
  - redeems the code at the token endpoint with the redirect URI, the verifier and the issuer's
    `client_secret`;
  - verifies the returned ID token like any other, and requires it to come from the issuer
    `Begin` named and to carry that sign-in's `nonce`.
- Every refusal is the usual opaque `UNAUTHENTICATED`, audited with a reason:
  `unknown_state`, `pkce_mismatch`, `code_refused`, `idp_unavailable`, `issuer_mismatch`,
  `nonce_mismatch`, `malformed_code`, `code_flow_off`. `Issuers` and `Begin` need no bearer.
- With browser sign-in on, the serve path resolves each issuer's `client_secret` at startup. A
  reference that does not resolve refuses to start.

**Audit trail** (S11, [`server/audit.rs`](../crates/agent-grpc/src/server/audit.rs)). Every
auth event is a row in ClickHouse `agent.agent_auth_events` when `[telemetry]` is on.

- Rows cover: `login` and `refresh` (`Exchange`, `Refresh`); `logout` and `revoke`; every
  refused credential (`verify_fail`, with a reason such as `no_token`, `invalid_token`,
  `login_invalid`, `refresh_reused`, `session_not_live`, `cert_not_presented`); every
  permission denial; allows for anything other than reads and `(use, agent)`; and role
  and binding changes.
- `user` is the tenant. A refusal that proved no identity has an empty tenant, so only
  operators see it: the `tenant_iso_auth_events` policy never matches `''`.
- `rpc` is only ever a served method (never a path the caller made up); `trace_id` joins
  the row to its OTLP trace; no column holds token material.

```sql
-- as agent_reader: this tenant's denials in the last day
SELECT ts, subject, action, resource_type, rpc, reason FROM agent.agent_auth_events
WHERE event = 'authz_deny' AND ts > now() - INTERVAL 1 DAY
SETTINGS SQL_tenant_id = 'example.com';
```

### Isolation is not containment: `bash` and the exec seams

Per-tenant paths isolate the *confined* file tools (`edit`/`read`/`write`/`search`).
They do **not** contain `bash`, which is the deliberate *unconfined* escape hatch
([`CLAUDE.md`](../CLAUDE.md)): in a multi-user deployment, `bash` under one session can
read another user's files or the host. The same applies with more force to
`--serve-sandbox` / `--serve-pty` / `--serve-forge`, which host arbitrary execution and
whose `Policy` gate stays **client-side**
([above](#--serve-sandbox-and---serve-pty-are-a-different-class-of-grant)). Until the auth
follow-up lands, harden exec-capable multi-user deployments at the transport:

- **One UDS socket per user**, with OS file permissions (`0o600` inside a `0o700` dir),
  so the socket itself enforces the user boundary and `x-agent-user-id` becomes advisory.
  Session namespacing still isolates that user's own sessions within the socket.
- Or **disable `bash` per-user by `Policy`** in multi-user configs.

These are deployment choices, not defaults — the single-process CLI/REPL (one `local`
user) needs none of it.

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

> **Poking a seam served over a unix socket?** Address it with the `unix://<path>`
> **scheme**, *not* grpcurl's `-unix` flag — the flag expects a `host:port` and
> fails with `missing port in address` on a bare path. The same `unix://<path>`
> form works for `ghz` and any gRPC-Go-based tool, so prefer it everywhere:
>
> ```sh
> agent --serve-all --listen unix:/tmp/agent-seddon/gateway.sock &
> grpcurl -plaintext unix:///tmp/agent-seddon/gateway.sock list
> ```

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
  without making the trait async — see
  [Three seams are deliberately not distributed](#three-seams-are-deliberately-not-distributed).
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

## `--serve-sandbox` and `--serve-pty` are a different class of grant

Every other seam server exposes a *capability with a shape*: fetch this URL,
count these tokens, scan this text. These two expose **arbitrary code
execution**. They accept a command string and run it, so anyone who can reach
the socket can execute code on that host as the serving user.

The transport is unauthenticated **by design** — a unix socket 0o600 in a 0o700
dir, or loopback TCP — so the socket's file permissions *are* the access control.
Binding either to a routable address is equivalent to running an unauthenticated
remote shell.

Note also what does **not** move: the `Policy` gate lives on the agent side, in
front of the tool. A seam server hosts the raw capability, so a compromised or
careless client is not screened by the server.

The upside is real, which is why they are here: an agent process can stay thin
and unprivileged while execution happens on a host built for it — one with the
toolchain, or one deliberately isolated from anything the agent should not reach.
That is a *better* posture than executing in-process, provided the socket is
where you think it is.

**`--serve-forge` carries the same caveat with a different blast radius.** It
performs *authenticated writes to the hosting platform* — opening pull requests,
commenting, submitting reviews — on behalf of whoever reaches it. The same upside
applies, and is arguably stronger: the platform token lives in one process, so an
agent can open a pull request without ever holding a credential that could also
delete a repository.

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
- Peer-certificate → service-principal mapping and refusing plaintext on routable
  listeners (security-hardening S10).

## Three seams are deliberately not distributed

Twenty seam traits have a gRPC service. Three do not, and will not:

| Seam | Primary operation |
|---|---|
| `Prices` | `fn get(&self, model: &str) -> Option<ModelPrices>` |
| `OutputSchema` | `fn validate(&self, schema: &Value, value: &Value) -> Verdict` |
| `CacheStrategy` | `fn place(&self, prompt: &PromptShape, caps: &CacheCapabilities) -> CacheMarks` |

All three are `fn`, not `async fn`. A gRPC client physically cannot implement
them: there is nowhere to await the response. Distributing them therefore is not
a wiring exercise — it requires making those traits `async`, which ripples
through every implementation and call site in the workspace.

**That refactor was considered and declined.** The reasoning, recorded here so it
does not have to be rediscovered:

### It would make them slower, not faster

The intuition that `async` improves performance does not hold for these. Async
buys throughput when there is **I/O to overlap** — the await lets other work
proceed while you wait on a socket or a disk. None of these three wait on
anything:

- `Prices::get` is a map lookup, falling back to a longest-prefix scan of the
  table when the model id is not an exact row (a dated id matching its family).
- `OutputSchema::validate` is a pure, CPU-bound JSON-schema check.
- `CacheStrategy::place` is a pure function over the assembled message list.

Every one is CPU-bound and returns immediately. There is no point at which the
thread would otherwise be parked.

Meanwhile `#[async_trait]` — which every *async* seam trait here uses, and which
these would have to adopt — desugars each method to return
`Pin<Box<dyn Future + Send>>`. That is a **heap allocation on every call**, plus
a future state machine and an extra indirection. On the exact-hit path of
`Prices::get` that allocation plausibly costs as much as the lookup it wraps; on
the heavier paths it is proportionally smaller but still strictly additive. In
no case does anything get faster, because nothing was ever waiting.

> This is an argument from what `async_trait` provably generates, not from a
> benchmark. If the decision is ever revisited, measure it rather than re-arguing
> it: the `iai-callgrind` benches give deterministic instruction counts, and a
> before/after on a `Prices::get` bench would settle it in minutes. See
> [`components/benchmarking.md`](components/benchmarking.md).

### And there is nothing to gain by remoting them

Even setting the local cost aside, a network hop is *far* more expensive than the
work being done. A remote price lookup costs a round trip to save a hashmap
probe. There is no credential to isolate (the price table is public data), no
hardware to exploit (no GPU, no warm index), and no shared state worth
centralising that a config file does not already handle.

Contrast the seams that *are* distributed, each of which had a concrete reason:
credentials (`Forge`, `WebSearch`), specialised hardware (`Embedder`), a warm
process worth sharing (`LspBackend`), a host with the right reach or isolation
(`WebBackend`, `Sandbox`, `Pty`), or genuinely shared state (`SessionStore`,
`TaskTracker`). These three have none of those.

### What this costs, honestly

"Every capability is an inspectable, distributed seam" is now *nearly* true
rather than exactly true, and `--serve-all` hosts twenty services rather than
twenty-three. That is a real loss of uniformity, and it is the price of not
paying a per-call allocation across the whole workspace to make three pure
functions remotable for no benefit.

If a future backend changes the premise — a licensed schema validator behind an
API, or a pricing service with live rates — the calculus changes with it, and the
trait should go async *then*, for that reason.
