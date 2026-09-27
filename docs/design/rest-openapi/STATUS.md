# REST + OpenAPI surface — implementation status

The living tracker for the [rest-openapi](README.md) design. One gated PR per increment, based off
`main` — do not stack. Each must pass `nix develop -c nix flake check`. Update the matching row (and
the as-built log) in the PR that lands the increment.

## Increments

| # | Increment | Proto | Nix | Envoy | Tests | Bench | Status |
|---|---|:--:|:--:|:--:|:--:|:--:|:--:|
| 01 | [Design directory](README.md) (design-of-record + STATUS + index) | — | — | — | — | — | ✅ merged (#510) |
| 02 | Groundwork: vendor `google/api/{annotations,http}.proto`, wire `tonic-build` + buf-lint exemption, annotate one RPC, coverage-test skeleton | ✅ | — | — | ✅ | — | ✅ merged (#512) |
| 03 | Annotate the full surface, batched per proto group (reads→GET, deletes→DELETE, else POST body:*); server-streaming annotated too — **nothing excluded** | 🟡 | — | — | 🟡 | — | 🟡 in flight |
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
transcodable. **Decision (03):** annotate everything, server-streaming included — no RPC is left
gRPC-only, keeping the sweep uniform and the "full surface" commitment literal. A whole-set invariant
test (`boundary_surface_has_no_client_or_bidi_streaming_rpcs`) guards this — if a client/bidi RPC is ever
added, it fires as a reminder to list it here as gRPC-only.

## Increment 03 batches (one gated PR each, per proto group)

| Batch | Proto group | Status |
|---|---|:--:|
| 03a | `review_fleet.proto` (ReviewFleetService, 11 RPCs) | ✅ merged (#515) |
| 03b | `prompt.proto`, `role.proto`, `config.proto` (control plane) | 🟡 in flight |
| 03c | `forge_registry.proto`, `transport_registry.proto`, `upstream.proto` (registries) | ⬜ |
| 03d | `repo.proto`, `search.proto`, `ast.proto` (code intelligence) | ⬜ |
| 03e | `session.proto`, `session_registry.proto`, `agent_session.proto`, `scheduler.proto` | ⬜ |
| 03f | `tool.proto`, `exec.proto`, `web.proto`, `forge.proto` (TaskService) | ⬜ |
| 03g | `provider.proto`, `llm_pool.proto`, `embed.proto`, `tokenizer.proto`, `memory.proto`, `context.proto`, `dimension.proto`, `mode.proto`, `graph.proto`, `digest.proto`, `reference.proto`, `scanner.proto`, `lsp.proto`, `metrics_proxy.proto`, `policy.proto`, `review.proto` (remaining seams) | ⬜ |

(Batch boundaries may shift as the sweep proceeds; the tracker is updated per PR.)

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
- **03a (fleet control plane).** Annotated all 11 `ReviewFleetService` RPCs under `/v1/fleet/`:
  reads → GET (`List`→`/sessions`, `Get`→`/sessions/{id}`, `ListReviews`→`/reviews`,
  `GetReview`→`/reviews/{review_id}`, `Preflight`→`/preflight`), `Delete` → DELETE `/sessions/{id}`,
  every other mutation/action → POST `body:"*"` (`Put`→`/sessions`, `SetEnabled`→`/sessions/{id}/enabled`,
  `ReviewNow`→`/sessions/{session_id}/review-now`, `Approve`→`/reviews/{review_id}/approve`,
  `UpdateReview`→`/reviews/{review_id}`). Added `DELETE` to the documented convention. Expanded the
  coverage table to exercise every verb + the param/no-param/param+body shapes; the negative
  `Unmapped` row now points at `Policy.Authorize` (a later batch) and flips when `policy.proto` lands.
- **03b (control plane: role / prompt / config).** Annotated `RoleService` (4 RPCs, `/v1/roles/` —
  a straight CRUD mirror of the fleet-session shape), `PromptService` (8 RPCs, `/v1/prompts/`), and
  `ConfigService` (5 RPCs, `/v1/config/`). Two convention refinements first appear here (documented in
  the README): (1) **composite keys** — a prompt is keyed by `(kind, id)`, so its read/delete routes
  carry *two* path params (`/v1/prompts/{kind}/{id}`); (2) **reads with a request body** — a
  repeated-scalar filter stays GET with repeated query params (`PromptService.Select` →
  `/v1/prompts/select`), but a read whose request holds a nested message / free text uses POST
  `body:"*"` (`PromptService.PreviewAssembled` → `/v1/prompts/preview`, `ConfigService.Validate` →
  `/v1/config/validate`). Added 5 class-tagged coverage rows (role-Get mirror, config-Put write,
  prompt-Get two-param corner, Select repeated-scalar GET, config-schema paramless sub-path); the
  whole-set uniqueness/versioning/streaming invariants cover the rest automatically.
