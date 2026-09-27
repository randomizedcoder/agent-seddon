# REST + OpenAPI surface over the gRPC API

**Status:** design-of-record. Increment 1 (this directory) in flight.

## Why

The whole API is **gRPC only**. A repo-wide grep for `openapi`, `grpc_json_transcoder`,
`google.api.http`, `swagger`, `grpc-gateway` and `tonic-web` returns zero hits
([gap-analysis §4](../../gap-analysis/README.md)); the only HTTP surfaces are the Envoy grpc-web
bridge (portal-only, binary/base64 framing — not REST), the Prometheus `/metrics` scrape endpoint,
the egress CONNECT proxy, and `grpcurl` + reflection (a human tool, not a stable contract).

gRPC is and remains the **primary, recommended** interface. But a REST + JSON surface with a
published **OpenAPI** contract is worth having: it lets a curl/browser/third-party client call the
system without a protobuf toolchain, and the OpenAPI document is a machine-readable description
other tools can consume. We expect REST to be **low-traffic** — a compatibility surface, not the hot
path — and the docs say so plainly.

The groundwork is favourable and nothing in the Rust services has to change for the first steps:

- 34 proto files / 39 services / **157 RPCs** built by `tonic-build`
  ([build.rs](../../../crates/agent-proto/build.rs)).
- A `FileDescriptorSet` is already emitted for reflection (`build.rs:47-55`,
  `FILE_DESCRIPTOR_SET` in [lib.rs](../../../crates/agent-proto/src/lib.rs)).
- buf v2 with `WIRE_JSON` breaking rules ([buf.yaml](../../../buf.yaml)) — JSON field names are
  already protected against wire-incompatible edits.
- A hermetic `local:` buf plugin pattern already proven for Dart
  ([buf.gen.yaml](../../../buf.gen.yaml)); `protoc`, `buf`, `grpcurl` and the Envoy image are pinned
  ([nix/versions.nix](../../../nix/versions.nix)).

## Non-goals

- **REST is not a first-class or preferred interface.** The docs and the OpenAPI `info` recommend
  gRPC. No feature is REST-only.
- **No new long-running process and no new Rust HTTP server** in the default build (see "Why Envoy").
- **No client-streaming / bidirectional-streaming over REST** — the transcoder cannot do it; those
  RPCs stay gRPC-only (see "Streaming").
- **Not an external-exposure change on its own.** The REST listener defaults to loopback; exposing it
  publicly is gated on the auth work in [`security-hardening/`](../security-hardening/README.md)
  (see "Auth & exposure").

## Architecture — Envoy `grpc_json_transcoder`

The live REST↔gRPC translation is done by **Envoy's `grpc_json_transcoder` HTTP filter** — the same
Envoy we already run for grpc-web (`envoyproxy/envoy:v1.31-latest`,
[nix/versions.nix](../../../nix/versions.nix); config in
[nix/portal/default.nix](../../../nix/portal/default.nix)). The filter reads a proto
`FileDescriptorSet` plus the `google.api.http` annotations and translates JSON/HTTP into a gRPC call
to the backend.

```
   client ──HTTP/JSON──▶ Envoy [ grpc_json_transcoder ▶ cors ▶ router ] ──gRPC──▶ agent --serve-all
                                                                                    (same AuthLayer)
```

**Why Envoy, and not a Rust gateway or the Go binary.** Rust-native gRPC→REST gateways now exist
([`grpc-gw`](https://github.com/youyuanwu/grpc-gw), the [`tonic-rest`](https://docs.rs/tonic-rest/)
family, [`abada`](https://github.com/angolardevops/abada)) and the mature Go
[grpc-gateway](https://github.com/grpc-ecosystem/grpc-gateway) binary is always an option. For *this*
repo Envoy transcoding is strictly lower-risk:

- **Zero Rust code and zero new crate dependency** for `cargo-deny` / `cargo-audit` / `cargo-machete`
  to vet — the security posture ("the model is untrusted; fail closed", hermetic pins) has less new
  surface.
- **Zero new process** — reuses the Envoy already deployed for the portal; a Rust in-process gateway
  would add a second HTTP listener to the agent binary with its own auth plumbing, and the Go binary
  would add a Go toolchain and a separate proxy.
- **Reuses the descriptor set** we already produce; **mature** (Google Cloud Endpoints / ESP use the
  same transcoder).
- **Bypasses no authz.** A transcoded request is an ordinary gRPC call into the same server, so the
  existing gRPC `AuthLayer` applies unchanged.

The Rust-native in-process gateway stays documented as the **deferred, Envoy-free** option below.

## Annotation convention

Each RPC is mapped with a `google.api.http` option. To keep a 157-RPC sweep mechanical and
reviewable, the mapping follows one convention (documented here, applied uniformly):

- **Read-only RPCs** (`Get` / `List` / `Describe` / `Check` / `Status` / `Preflight` / `Diff` /
  `Recall`, and similar) → `get:` with path params for identifiers and query-param field expansion:

  ```proto
  rpc Get(GetRequest) returns (Review) {
    option (google.api.http) = { get: "/v1/fleet/reviews/{id}" };
  }
  ```

- **Mutating / action RPCs** → `post:` with `body: "*"`:

  ```proto
  rpc Approve(ApproveRequest) returns (ApproveResponse) {
    option (google.api.http) = { post: "/v1/fleet/reviews/{id}/approve" body: "*" };
  }
  ```

- **`Delete` RPCs** → `delete:` with the id as a path param (no body):

  ```proto
  rpc Delete(DeleteRequest) returns (DeleteReply) {
    option (google.api.http) = { delete: "/v1/fleet/sessions/{id}" };
  }
  ```

- **Server-streaming RPCs** → annotated; the transcoder emits a JSON array / chunked stream.

Two refinements the sweep applies (both first exercised by the `03b` control plane):

- **Composite keys** — when an entity's identity is more than one field (e.g. a prompt is keyed by
  the `(kind, id)` pair), every key field is a path param: `get: "/v1/prompts/{kind}/{id}"`.
- **Reads that carry a request body** — a read whose request is only scalar / repeated-scalar
  *filters* stays `get:` (repeated fields expand to repeated query params, e.g.
  `GET /v1/prompts/select?tags=mode:review&tags=language:rust`). A read whose request carries a
  **nested message** (or a free-text field that would need brittle dotted query expansion) uses
  `post: … body:"*"` instead — still side-effect-free, but the filter travels as a JSON body
  (e.g. `PromptService.PreviewAssembled`, `ConfigService.Validate`).

Paths are versioned under `/v1/` and grouped by area (`/v1/fleet/…`, `/v1/session/…`, …). URL
templates and field paths must be unique across the whole surface (enforced by a unit test, below).

### Streaming (the excluded set)

`grpc_json_transcoder` supports **unary** and **server-streaming** RPCs. **Client-streaming and
bidirectional-streaming RPCs are not transcodable** and would be deliberately left un-annotated (each
carrying a `// REST: gRPC-only (streaming)` comment and listed in `STATUS.md`).

**As surveyed in increment 02, this surface has none of them.** All five streaming RPCs
(`Provider.Stream`, `AstService.Reindex`, `SearchService.Reindex`, `AgentSessionService.Subscribe`,
`AgentSessionService.Send`) are **server-streaming**, so the whole surface is transcodable in principle
and the hard-exclusion set is **empty**. Increment 03 still decides per RPC whether a live-event /
token-stream RPC is *worth* a REST binding, but nothing is excluded for being untranscodable. A
whole-set invariant test (`boundary_surface_has_no_client_or_bidi_streaming_rpcs`) fires if a
client/bidi RPC is ever added, as a reminder to list it here.

## OpenAPI generation + drift gate

The OpenAPI document is generated from the annotated protos by **`protoc-gen-openapiv2`** (the
grpc-gateway OpenAPI plugin; emits **Swagger 2.0**), pinned in `nix/versions.nix` and run as a
hermetic `local:` buf plugin — never a BSR/network plugin, matching the Dart codegen posture.

The generated document is **committed** (`crates/agent-proto/openapi/agent.swagger.json`) and
**gated against drift** with the exact three-piece pattern that already keeps
`crates/agent-grpc/src/constants.rs` honest:

| Piece | New file | Model |
|---|---|---|
| Shared generator derivation → the formatted doc as `$out` | `nix/gen-openapi.nix` | [`nix/gen-constants.nix`](../../../nix/gen-constants.nix) |
| `nix run .#gen-openapi` app that `cp`s it into the repo | app in `nix/default.nix` | `gen-constants` / `buf-image` apps |
| `nix flake check` drift check (`diff -u` committed vs regenerated) | `nix/checks/openapi-sync.nix` | [`nix/checks/constants-sync.nix`](../../../nix/checks/constants-sync.nix) |

*If OpenAPI 3.0 is preferred over Swagger 2.0, the swap is gnostic's `protoc-gen-openapi`; this track
follows gap-analysis §4 and uses `protoc-gen-openapiv2`.*

## Auth & exposure

A transcoded REST call becomes an ordinary gRPC call into the same server, so the existing gRPC
`AuthLayer` (when the `auth` feature is built and `[auth] mode` is set) applies unchanged — **REST
adds no bypass**. But the REST surface must not *widen* the exposure the gap analysis already flags
([§2.8](../../gap-analysis/README.md)): Envoy binds `0.0.0.0`, CORS is `*`, and `allow_headers` omits
`authorization`.

This track therefore:

- Defaults the REST listener to **loopback**.
- Adds `authorization` to the listener's CORS `allow_headers` so a bearer token can be forwarded.
- Documents that **external exposure is gated** on the auth work in
  [`security-hardening/`](../security-hardening/README.md) (agent-issued JWT, one `AuthLayer` for
  gRPC / grpc-web / REST) and/or an Envoy `jwt_authn` / `ext_authz` filter in front of the transcoder.

## Testing

**Unit — annotation coverage** (`crates/agent-proto`, reads `FILE_DESCRIPTOR_SET`). A table-driven
`rstest` (four classes + adversarial, `description` + typed `expected` columns, modelled on
[`reach.rs`](../../../crates/agent-providers/src/reach.rs)) walks the descriptor set and asserts, per
method and its streaming kind, that a `google.api.http` rule is present (unary / server-streaming) or
absent (client/bidi), that read RPCs map to `GET` and actions to `POST body:*`, and that no two RPCs
declare an overlapping URL template. A companion **OpenAPI-parity** test asserts the committed
`agent.swagger.json` has exactly one path per annotated RPC (catches gen/annotation drift).

**Integration — `nix run .#rest-integration`** (modelled on
[`serve-smoke`](../../../nix/serve-smoke.nix) + the portal-e2e Envoy bring-up, `harness.contract`
0/1/2 exit): boot `agent --serve-all` on loopback → bring up Envoy with the transcoder listener →
`curl` REST endpoints → assert → teardown. **REST request bodies are untrusted input**, so
adversarial cases are mandatory: malformed JSON → 400 (never 5xx), oversized body → capped (no OOM),
unknown field → handled per config, path param containing `../` → confined. Self-skips green when no
container runtime is reachable, and folds into [`nix/integration.nix`](../../../nix/integration.nix).

## Benchmarking

Transcoding runs **in Envoy (out-of-process)**, so there is no Rust transcode hot path for
`iai-callgrind`. The meaningful, honest artifact is a **REST-vs-gRPC comparison** that also
substantiates the "recommend gRPC" guidance: a gRPC baseline via **`ghz`** (already pinned) against a
representative unary RPC, and REST latency/throughput against the same RPC through the transcoder,
reporting p50/p95 and the overhead delta — plus an operational check on descriptor/config size and
Envoy load time for 157 RPCs (`nix run .#rest-bench`, opt-in like `loadtest`).

## Increments (each a gated PR off `main`, never stacked)

1. **This directory** — design-of-record + `STATUS.md` + `docs/README.md` index entry. Docs only.
2. **Groundwork** — vendor `google/api/{annotations,http}.proto`; wire `tonic-build` + a buf-lint
   exemption for the vendored tree; annotate **one** RPC as proof; land the annotation-coverage unit
   test asserting its rule survives into `FILE_DESCRIPTOR_SET`.
3. **Annotate the full surface** — `google.api.http` on every transcodable RPC, in reviewable
   per-proto batches; each batch extends the coverage-test manifest. `buf breaking` stays green
   (adding options is additive).
4. **OpenAPI + drift gate** — pin `protoc-gen-openapiv2`; generate + commit the doc; the
   `gen-openapi` / `openapi-sync` trio; the OpenAPI-parity test.
5. **Transcoder** — a `buf build --as-file-descriptor-set` descriptor derivation; a new loopback REST
   listener (port via [`nix/constants.nix`](../../../nix/constants.nix)); the `grpc_json_transcoder`
   filter before `router`; descriptor mount; `authorization` added to CORS `allow_headers`.
6. **`nix run .#rest-integration`** — the live REST harness, folded into `nix/integration.nix`.
7. **`nix run .#rest-bench`** — the REST-vs-gRPC perf artifact.

## Deferred: the Rust-native, Envoy-free path

For deployments that do not front the agent with Envoy, a Rust-native in-process gateway
([`grpc-gw`](https://github.com/youyuanwu/grpc-gw) as a `tower::Service`, or
[`tonic-rest`](https://docs.rs/tonic-rest/) generating Axum handlers + OpenAPI 3.1) could serve the
same `google.api.http` annotations directly from the agent binary. It is deferred because it adds a
dependency to a hermetic, strict-audit tree and a second HTTP listener needing its own auth wiring —
weight that a low-traffic compatibility surface does not justify while Envoy is already in the
deployment.
