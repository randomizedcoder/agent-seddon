# REST + OpenAPI surface — implementation status

The living tracker for the [rest-openapi](README.md) design. One gated PR per increment, based off
`main` — do not stack. Each must pass `nix develop -c nix flake check`. Update the matching row (and
the as-built log) in the PR that lands the increment.

## Increments

| # | Increment | Proto | Nix | Envoy | Tests | Bench | Status |
|---|---|:--:|:--:|:--:|:--:|:--:|:--:|
| 01 | [Design directory](README.md) (design-of-record + STATUS + index) | — | — | — | — | — | ✅ merged (#510) |
| 02 | Groundwork: vendor `google/api/{annotations,http}.proto`, wire `tonic-build` + buf-lint exemption, annotate one RPC, coverage-test skeleton | ✅ | — | — | ✅ | — | 🟡 in flight |
| 03 | Annotate the full transcodable surface (batched per proto file); streaming RPCs excluded + documented | ✅ | — | — | ✅ | — | ⬜ |
| 04 | OpenAPI doc: pin `protoc-gen-openapiv2`, generate + commit, `gen-openapi`/`openapi-sync` drift gate, OpenAPI-parity test | — | ✅ | — | ✅ | — | ⬜ |
| 05 | Envoy `grpc_json_transcoder`: descriptor derivation, loopback REST listener (port via `nix/constants.nix`), filter before `router`, descriptor mount, `authorization` in CORS | — | ✅ | ✅ | — | — | ⬜ |
| 06 | `nix run .#rest-integration` (boot → Envoy → curl → assert → teardown; adversarial cases); folded into `nix/integration.nix` | — | ✅ | ✅ | ✅ | — | ⬜ |
| 07 | `nix run .#rest-bench` (REST-vs-gRPC via `ghz` + HTTP load; descriptor/config-size note) | — | ✅ | ✅ | — | ✅ | ⬜ |

Legend: ✅ built · 🟡 partial / in flight · ⬜ not started.

## Build order = dependency order

- **02** must land before **03**: vendoring + tonic-build wiring + the buf-lint exemption is the
  prerequisite that makes a `google.api.http` option parse and survive into the descriptor; one
  proven RPC de-risks the 157-RPC sweep.
- **03** produces the annotations that both **04** (OpenAPI) and **05** (the Envoy descriptor) read.
- **04** and **05** both consume the descriptor but are independent of each other; **04** is the
  committed contract + drift gate, **05** is the live path.
- **06** needs **05** (a running transcoder to curl); **07** needs **05** + **06** (a live REST path
  and the harness shape to load-test).

## Streaming exclusions (RPCs deliberately left un-annotated)

**Finding (02):** the surface has **no client-streaming or bidi-streaming RPCs** — all five streaming
RPCs (`Provider.Stream`, `AstService.Reindex`, `SearchService.Reindex`, `AgentSessionService.Subscribe`,
`AgentSessionService.Send`) are **server-streaming**, which the transcoder *does* support (it emits a
JSON array / chunked body). So there is **no hard REST-exclusion list** — the whole surface is
transcodable in principle. Increment 03 still decides, per RPC, whether a live-event / token-stream RPC
is *worth* a REST binding (UX), but nothing is excluded for being untranscodable. A whole-set invariant
test (`boundary_surface_has_no_client_or_bidi_streaming_rpcs`) guards this — if a client/bidi RPC is ever
added, it fires as a reminder to list it here as gRPC-only.

## Implementation log (as-built deviations)

- **01 (this directory).** Runtime REST engine chosen = **Envoy `grpc_json_transcoder`** over the
  Rust-native crates (`grpc-gw`/`tonic-rest`) and the Go grpc-gateway binary — decision + rationale in
  [README](README.md#architecture--envoy-grpc_json_transcoder). Annotation scope = the full surface.
- **02 (groundwork).** Vendored `google/api/{annotations,http}.proto` under
  `crates/agent-proto/proto/google/api/` (Apache-2.0, unmodified; `google/protobuf/descriptor.proto`
  is a protoc well-known type, *not* vendored). Exempted the vendored tree from `buf lint` **and**
  `buf breaking` (`buf.yaml` `ignore:`). `build.rs`: the `["proto"]` include path already resolves the
  imports, so the google files are *not* added to the compile list (no Rust generated for them) — only
  a `rerun-if-changed` entry each. Annotated one proof RPC, `ReviewFleetService.Get` →
  `GET /v1/fleet/sessions/{id}`. Coverage test (`crates/agent-proto/tests/http_annotations.rs`) decodes
  `FILE_DESCRIPTOR_SET` via a **minimal descriptor mirror** declaring the `google.api.http` extension
  (field 72295728) — `prost` drops unknown fields and `prost_types::MethodOptions` has no field for the
  custom extension, so this reads the option with **no new dependency** (no `prost-reflect`). This proves
  `(google.api.http)` survives `tonic-build` codegen before increment 03 annotates the rest.
