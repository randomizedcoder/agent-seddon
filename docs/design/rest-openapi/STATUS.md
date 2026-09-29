# REST + OpenAPI surface — implementation status

The living tracker for the [rest-openapi](README.md) design. One gated PR per increment, based off
`main` — do not stack. Each must pass `nix develop -c nix flake check`. Update the matching row (and
the as-built log) in the PR that lands the increment.

## Increments

| # | Increment | Proto | Nix | Envoy | Tests | Bench | Status |
|---|---|:--:|:--:|:--:|:--:|:--:|:--:|
| 01 | [Design directory](README.md) (design-of-record + STATUS + index) | — | — | — | — | — | ✅ merged (#510) |
| 02 | Groundwork: vendor `google/api/{annotations,http}.proto`, wire `tonic-build` + buf-lint exemption, annotate one RPC, coverage-test skeleton | ✅ | — | — | ✅ | — | ✅ merged (#512) |
| 03 | Annotate the full surface, batched per proto group (reads→GET, deletes→DELETE, else POST body:*); server-streaming annotated too — **nothing excluded** | ✅ | — | — | ✅ | — | ✅ complete (03a–03g4) |
| 04 | OpenAPI doc: pin `protoc-gen-openapiv2`, generate + commit, `gen-openapi`/`openapi-sync` drift gate, OpenAPI-parity test | — | ✅ | — | ✅ | — | ✅ merged (#534) |
| 05 | Envoy `grpc_json_transcoder`: descriptor derivation, loopback REST listener (`:8094`, in the `portal_envoy.py` renderer), filter after `cors`/before `router`, descriptor mount, `authorization` in CORS | — | ✅ | ✅ | ✅ | — | ✅ complete |
| 06 | `nix run .#rest-integration` (boot → Envoy → curl → assert → teardown; adversarial cases); folded into `nix/integration.nix` | — | ✅ | ✅ | ✅ | — | ✅ complete |
| 07 | `nix run .#rest-bench` (REST-vs-gRPC via `ghz` + HTTP load; descriptor/config-size note) | — | ✅ | ✅ | — | ✅ | ✅ complete |

**★ Track complete — all 7 increments merged (#510, #512, #515–#530, #534, #538, #539, PR-07).**

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
| 03b | `prompt.proto`, `role.proto`, `config.proto` (control plane) | ✅ merged (#517) |
| 03c | `forge_registry.proto`, `transport_registry.proto`, `upstream.proto` (registries) | ✅ merged (#519) |
| 03d | `repo.proto`, `search.proto`, `ast.proto` (code intelligence) | ✅ merged (#520) |
| 03e | `session.proto`, `session_registry.proto`, `agent_session.proto`, `scheduler.proto` | ✅ merged (#522) |
| 03f | `tool.proto`, `exec.proto`, `web.proto`, `forge.proto` (TaskService) | ✅ merged (#523) |
| 03g1 | `provider.proto`, `llm_pool.proto`, `embed.proto`, `tokenizer.proto` (LLM plane) | ✅ merged (#526) |
| 03g2 | `memory.proto`, `context.proto`, `dimension.proto`, `mode.proto`, `digest.proto`, `reference.proto` (cognition/memory) | ✅ merged (#527) |
| 03g3 | `graph.proto`, `scanner.proto`, `lsp.proto`, `metrics_proxy.proto`, `policy.proto`, `review.proto` (analysis + control) | ✅ merged (#529) |
| 03g4 | `auth.proto` (AuthService, 15 RPCs — OIDC/JWT/RBAC/sessions/bindings + S13 browser sign-in `Issuers`/`Begin`) — **final batch; whole surface now annotated** | ✅ merged (#530) |

(Batch boundaries may shift as the sweep proceeds; the tracker is updated per PR. The original
16-proto `03g` was split into reviewable sub-batches — smaller PRs; `auth.proto` surfaced as a
distinct control-plane group during the sweep and became its own final batch, 03g4.)

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
- **03c (registries: forge / transport / provider-router).** Annotated
  `ForgeRegistryService` (`/v1/forges/`) and `TransportRegistryService` (`/v1/transports/`) — both
  straight CRUD mirrors of the fleet-session shape — and `ProviderRegistryService`, the model-router
  registry (the portal's "Router" tab), under `/v1/router/`: upstream CRUD + toggle under
  `/v1/router/upstreams` (`Enable` → `/v1/router/upstreams/{id}/enable`), the router-wide policy at
  `/v1/router/policy` (GET/POST), `Health` → GET `/v1/router/health`, and `Route` introspection →
  POST `/v1/router/route` (read-only but its request nests a `RouteHint`, so the nested-body-read
  convention maps it to POST, not GET). Added 3 class-tagged coverage rows (forge-Get third mirror,
  upstream-Enable nested toggle, Route nested-body-read → POST).
- **03d (code intelligence: repo / search / ast).** Annotated `RepoService` (`/v1/repo/`),
  `SearchService` (`/v1/search/`), and `AstService` (`/v1/ast/`). Two shapes appear here for the first
  time: (1) **revision-addressed reads ride as query params, never path captures** — a revision/path is
  a rev-spec that may contain `/` (`refs/heads/main`, `dir/file`), so unlike a `safe_segment` id it can't
  be a `{param}` segment; every object read (`Resolve`/`ReadFile`/`ListTree`/`Diff`/`Grep`/`Log`) is a
  bare GET with the fields auto-mapped to the query string. (2) **the first server-streaming RPCs**
  (`SearchService.Reindex`, `AstService.Reindex`) — the transcoder supports server-streaming (chunked
  JSON), and a reindex is a side-effect, so both are POST `body:"*"` (nothing left gRPC-only). AST's
  structural queries split by the nested-body-read convention: those naming a target by a nested
  `SymbolRef` (`Implementations`/`InterfaceOf`/`Callers`/`Callees`/`Callchain`) → POST `body:"*"`, while
  scalar/repeated-scalar reads (`FindSymbol`/`BlastRadius`/`DependencyPath`) stay GET. `RepoService`
  lifecycle side-effects → POST (`Fetch`/`WorktreeAdd`/`CreateCheckpoint`/`Push`), `WorktreeRemove` →
  DELETE `/{id}`; list + create share the `/v1/repo/worktrees` collection path, disambiguated by verb
  (GET lists, POST creates). Added 6 class-tagged coverage rows (revision-query-param read, first
  server-streaming annotation, nested-body Callers vs repeated-scalar BlastRadius, and the GET/POST
  shared-collection-path pair); the whole-set invariants cover the rest.
- **03e (sessions: session / session-registry / agent-session / scheduler).** Annotated the four
  session-family services under distinct areas so their routes never overlap: `SessionService`
  (`/v1/sessions/`, the content-addressed checkpoint STORE), `SessionRegistryService`
  (`/v1/session-registry/`, lifecycle), `AgentSessionService` (`/v1/agent-sessions/`, live view + drive),
  and `SchedulerService` (`/v1/scheduler/`). New shapes: (1) **a nested composite-key DELETE** — a session
  is keyed by `(user, session_id)`, so `Close` → DELETE `users/{user}/sessions/{session_id}` (two path
  params, deeper than the prompt read); `Open`/`Heartbeat` nest the same way. (2) **server-streaming
  splits by read-vs-action** — `AgentSessionService.Subscribe` is a server-streaming *read* → GET (the
  first GET-streaming route), while `Send` (drives the agent, `--serve-mcp`-class) and the reindex RPCs
  are server-streaming *actions* → POST. This closes the streaming set: all five server-streaming RPCs
  (`Search.Reindex`, `Ast.Reindex`, `AgentSession.Subscribe`, `AgentSession.Send`, and the still-pending
  `Provider.Stream` in 03g) are accounted for; none is gRPC-only. `SessionService` follows the canonical
  collection triple (`checkpoints`: POST create / GET list / GET `/{id}` item), head-mutations → POST;
  `SchedulerService` maps `Cancel` → DELETE `/jobs/{id}` and `History` → GET `/jobs/{id}/runs`. Added 5
  class-tagged coverage rows (composite-key nested DELETE, server-streaming read → GET, drive → POST,
  Cancel-as-DELETE, sub-resource read); the whole-set invariants cover the rest.
- **03f (tools / exec / web / forge).** Annotated the seven capability seams: `ToolService`
  (`/v1/tools/`), `SandboxService` + `PtyService` (`/v1/sandbox/`, `/v1/pty/`), `WebService` +
  `WebSearchService` (`/v1/web/`, `/v1/web-search/`), and `ForgeService` + `TaskService` (`/v1/forge/`,
  `/v1/tasks/`). New shapes: (1) **a name/key captured in the path with a nested body** —
  `ToolService.Execute` → POST `/v1/tools/{name}/execute` (arguments/context in the body). (2) **the
  first numeric path param** — a PR is addressed by `uint64 number`, bound to `{number}` exactly like a
  string id (`ForgeService.GetPr` → GET `/v1/forge/prs/{number}`; Comment/ReviewPr write under it). (3) **a
  collection-level DELETE with no path param** — `TaskService.Clear` → DELETE `/v1/tasks`, on the same
  `/v1/tasks` path that also carries POST Write + GET List (three verbs, one path). Reads that only
  *retrieve* (WebService.Fetch, WebSearch.*, Pty.Read cursor read) stay GET even when they touch the
  network or advance a cursor; the large-grant actions (`Sandbox.Exec`, `Pty.Open`/`Write`/`Resize`,
  every `Forge` write) → POST body:*. `Update` (patch a todo matched by free-text `content`, not a
  path-safe id) stays POST `/v1/tasks/update` per the no-PATCH convention. The two dangerous protos carry
  a note that transcoding does NOT widen the (unauthenticated-by-design) grant — a REST call hits the
  same seam, same loopback/UDS confinement. Added 6 class-tagged coverage rows (name-in-path + body,
  exec action, pty cursor read → GET, numeric path param, collection-level DELETE, write under a numeric
  parent); the whole-set invariants cover the rest.
- **03g1 (LLM plane: provider / llm-pool / embed / tokenizer).** Annotated `Provider`
  (`/v1/provider/`), `LlmPoolService` (`/v1/llm-pool/`), `EmbedService` (`/v1/embed/`), and
  `TokenizerService` (`/v1/tokenizer/`). `Provider.Stream` is the **last of the five server-streaming
  RPCs** → POST body:*, so the streaming set is now complete (none gRPC-only). New refinement
  (documented in the README convention): **content-payload vs filter** — the repeated-scalar-stays-GET
  rule is for *filters* (tags/globs/ids), but a repeated-scalar **content payload** (document bodies)
  uses POST body:* since a query string carries selectors, not payloads: `EmbedService.EmbedDocs` →
  POST `/v1/embed/docs`, while the single-query `EmbedQuery` stays GET `/v1/embed/query`. Nested-request
  completions/counts → POST (`Provider.Complete`, `LlmPool.Complete`, `Tokenizer.CountMessages`); reads
  → GET (`*.Capabilities`/`Health`, `Tokenizer.Count`). Added 5 class-tagged coverage rows (last
  server-streaming → POST, nested complete → POST, content-payload EmbedDocs → POST vs single-query
  EmbedQuery → GET, repeated-nested CountMessages → POST); the whole-set invariants cover the rest.
- **03g2 (cognition / memory: memory / context / dimension / mode / digest / reference).** Annotated the
  three memory services (`Memory` `/v1/memory/`, `Episodic` `/v1/episodic/`, `Semantic` `/v1/semantic/`),
  `ContextService` (`/v1/context/`), `DimensionService` (`/v1/dimensions/`), `ModeService` (`/v1/mode/`),
  `DigestService` (`/v1/digests/`), and `ReferenceService` (`/v1/references/`). Nested-request reads and
  content payloads → POST body:* (`Memory.Recall`/`Semantic.Recall` carry a `RecallQuery`;
  `Context.Assemble`/`Compact`, `Dimension.Summarize`, `Mode.Classify` carry nested/history payloads;
  `Reference.Resolve` carries a whole prompt — the **scalar** form of the content-payload refinement).
  Scalar-only reads → GET (`Episodic.Recent`, `limit` as query). Two new addressing shapes: a read keyed
  by a `safe_segment` **slug** captures it as a path param (`Dimension.Recall` → GET
  `/v1/dimensions/{dimension}`), and a ledger read keyed by `session_id` keeps its repeated-scalar
  `keywords_any` **filter** as query params (`Digest.Query` → GET `/v1/digests/{session_id}` — the filter
  half of the content-payload-vs-filter rule). Writes/triggers → POST (`*.Append`, `Digest.Put`,
  `Memory.Distill` even with an empty request). Added 6 class-tagged coverage rows (scalar-limit read →
  GET, nested-body Recall → POST, slug path param, repeated-scalar filter stays GET, content-payload
  Resolve → POST, empty-request action → POST); the whole-set invariants cover the rest.
- **03g3 (analysis + control: graph / scanner / lsp / metrics_proxy / policy / review).** Annotated
  `GraphService` (`/v1/graph/`), `ScannerService` (`/v1/scanner/`), `LspService` (`/v1/lsp/`),
  `MetricsProxyService` (`/v1/metrics/`), `Policy` (`/v1/policy/`), and `FactCollectorService`
  (`/v1/review/`). `GraphService.Get`/`Put` share `/v1/graph` (verb-disambiguated, like
  WorktreeList/Add); `Validate` carries the whole document → POST body:* (nested-body-read). Content
  payloads → POST (`Scanner.Scan`, `Lsp.Open`/`Request`); `Policy.Authorize` carries a nested `ToolCall`
  → POST (this **flipped the former `Unmapped` sentinel** to a positive row). Two read shapes worth
  noting: `MetricsProxy.Query`/`QueryRange` are pure reads whose PromQL `query` is a **selector**
  expression over stored series (not a content payload), so they stay GET with the query as a query
  param — mirroring Prometheus's own `/api/v1/query`; and `FactCollector.Collect`'s `target` selector can
  contain `:`/`/` (`branch:feature/x`), so — like `RepoService.ReadFile`'s revision — it rides as a query
  param, never a `{param}` capture (GET `/v1/review/facts`). Added 6 class-tagged coverage rows
  (shared-path GET, nested-body Validate → POST, content-payload Scan → POST, PromQL selector stays GET,
  slashy selector as query param, the Policy.Authorize flip). **Completeness:** a sweep of `proto/agent/v1/`
  found `auth.proto` (`AuthService`, 13 RPCs) as the sole remaining unannotated service — carved out as
  the final batch **03g4**; the `Unmapped` sentinel now points at `AuthService.WhoAmI` until then.
- **03g4 (auth: AuthService) — FINAL batch; increment 03 complete.** Annotated all 15 RPCs under
  `/v1/auth/`: token-mint/rotate/mutations → POST body:* (`Exchange`, `Begin`, `Refresh`, `Logout`,
  `PutBinding`), reads → GET (`Issuers`, `Jwks`, `WhoAmI`, `ListMySessions` `/my/sessions`, `ListSessions`
  + `ListBindings` with `tenant` as a query param, `GetBinding` `/bindings/{id}`), and session/binding
  removals → DELETE by id (`RevokeMySession` `/my/sessions/{sid}`, `RevokeSession` `/sessions/{sid}`,
  `DeleteBinding` `/bindings/{id}` — the `tenant`/`keep_sessions` scalars ride as query params, no body on
  a DELETE). `bindings` (GET list / POST create) and `bindings/{id}` (GET get / DELETE remove) each pair
  two verbs on one path. The S13 browser sign-in RPCs (`Issuers` → GET `/v1/auth/issuers`, `Begin` → POST
  `/v1/auth/begin` — it mints single-use server-side `state`, so its PKCE challenge/redirect ride in the
  body, not the URL) landed on `main` (#528) after this batch was cut and were folded in on rebase.
  **Flipped** the `AuthService.WhoAmI` `Unmapped` sentinel to a positive GET row and added a new end-state
  invariant — `adversarial_every_method_has_a_route` — asserting EVERY RPC in the descriptor now carries
  ≥1 route (a routeless method = a new RPC added without an annotation; this is what caught `Issuers`/`Begin`
  on rebase). Added 6 class-tagged rows (paramless WhoAmI → GET, Exchange token-mint → POST, RevokeMySession
  nested DELETE, DeleteBinding DELETE-by-id, Issuers read → GET, Begin start-flow → POST); now 55 rows + 5
  invariants = 60 tests. With this, **the whole surface is annotated** and increment 03 (03a–03g4) is
  complete — REST bypasses no authz (transcoded calls hit the same gRPC handler behind the same
  `AuthLayer`). Next: increment 04 (OpenAPI doc + drift gate).
- **04 (OpenAPI doc + drift gate).** Generated a committed **Swagger 2.0** contract at
  `crates/agent-proto/openapi/agent.swagger.json` from the `.proto` `(google.api.http)` annotations, and
  gated it against drift with the **`constants-sync` trio**: a shared derivation
  (`nix/gen-openapi.nix`) → the `gen-openapi` app (copies the derivation into the repo) → the
  `openapi-sync` check (`diff -u` committed vs derivation). Generator = **`protoc-gen-openapiv2`**,
  bundled in nixpkgs `grpc-gateway` (pinned as `versions.grpc-gateway`; the top-level
  `protoc-gen-openapiv2`/gnostic attrs are absent in the pin) and run via `buf generate` with its own
  template (`nix/openapi/buf.gen.openapi.yaml`). openapiv2 needs a Go import path per proto (only for
  output grouping); supplied via buf v2 **managed mode** (`go_package_prefix` override + `disable
  go_package` for the `buf.build/googleapis/googleapis` module so the vendored google/api protos keep
  their own) rather than 36 hand-written `M…=` mappings — output is byte-identical and **deterministic**
  across runs (`preserve_rpc_order=true`), which the drift gate requires. opts: `allow_merge=true` +
  `merge_file_name=agent` (one merged doc), `json_names_for_fields=true` (camelCase JSON + `{reviewId}`
  path params), `openapi_naming_strategy=fqn`. A `.info` rewrite (`nix/openapi/info.jq`) stamps the
  title/version/"recommend gRPC" description + do-not-edit note. **Crane caveat:** the committed doc is
  `.json`, which crane's Rust source filter drops from `commonArgs.src` — so, like `buf.nix`,
  `openapi-sync.nix` references it by a direct nix path, not through the filtered source. **Parity test**
  (`crates/agent-proto/tests/openapi_parity.rs`): asserts the committed doc's `(verb, path)` set equals
  the route set decoded straight from `FILE_DESCRIPTOR_SET` — so a generator that dropped or invented a
  route fails in `cargo test`, not just at Envoy load time. Both it and `http_annotations.rs` now derive
  routes from one shared decoder (`tests/proto_http/mod.rs`, the minimal descriptor mirror extracted from
  02) so they can never disagree. Doc = 146 unique paths / 172 `(verb,path)` routes; 4 parity tests + the
  refactored 60 annotation tests all green. Verified: `nix run .#gen-openapi` no-diffs a clean tree, and a
  hand-edit makes `openapi-sync` fail (the gate bites both ways). Next: increment 05 (Envoy transcoder).
- **05 (Envoy `grpc_json_transcoder` — the live REST path).** Added a REST/JSON transcoder listener on
  **`127.0.0.1:8094`** that fronts the SAME `agent_gateway` cluster the grpc-web bridge uses, so a REST
  call is projected to gRPC per the `.proto` `(google.api.http)` routes and hits the SAME handler behind
  the SAME `AuthLayer` — REST bypasses no authz. Filter chain `cors → grpc_json_transcoder → router`
  (the transcoder must precede `router`); `typed_config`: `auto_mapping: false` (every RPC is annotated),
  `match_incoming_request_route: true` (an unmapped path 404s rather than being force-mapped),
  `convert_grpc_status: true`, `request_validation_options.{reject_unknown_method,reject_unknown_query_parameters}:
  true` (the REST body is attacker-controlled — fail closed), `print_options.{add_whitespace,always_print_primitive_fields}`.
  CORS `allow_headers` already carries `authorization` (a bearer forwards to the agent's AuthLayer).
  **Descriptor + service list from one derivation** (`nix/rest-descriptor.nix`): `buf build
  --as-file-descriptor-set` emits the `agent_descriptor.pb` Envoy loads (imports included → the http
  options resolve; Envoy's C++ protobuf reads the custom option natively), and the same build derives the
  service list (all 40 `agent.v1.*` FQNs, via `buf … #format=json | jq`) — so the transcoder's `services:`
  can never disagree with its descriptor, and a newly-added service is picked up with no hand-maintained
  list. The descriptor is **not committed**; it is mounted read-only into the Envoy container at bring-up.
  **Two deviations from the plan** (both driven by #536, which landed between plan and build and replaced
  the Envoy YAML heredoc with a data-driven Python renderer, `test/portal-envoy/portal_envoy.py`, on the
  gate):
  (1) **The REST port lives in `nix/portal/envoy-spec.nix` (`ports.rest = 8094`), not `nix/constants.nix`**
  — matching the grpc-web/Envoy listener ports, which S14 keeps there as UI plumbing rather than in the
  seam table, so no `constants.rs`/`gen-constants` churn.
  (2) **The listener is expressed as a `rest` block in the spec and rendered by `portal_envoy.py`, not a
  hand-written heredoc.** A new `Rest` dataclass + `load_services()` parse it, failing closed on any
  non-`agent.v1` / empty / oversized / control-char service file; `rest_listener()` renders the chain; the
  descriptor is mounted via the same host→container path indirection the TLS files use. The REST listener
  is **pinned to `127.0.0.1`** and, unlike the grpc-web listeners, **ignores `PORTAL_GRPC_WEB_HOST`** — a
  LAN bind of the browser bridge never silently exposes an (edge-)unauthenticated REST surface (the agent
  AuthLayer still applies; edge `jwt_authn` for a publicly-fronted REST listener, which needs REST-path
  unauthenticated prefixes rather than the grpc-web gRPC-path ones, is deferred with external exposure).
  **Gate coverage is now stronger than the plan anticipated:** the `portal-envoy` check runs **real `envoy
  --mode validate`** over the whole spec — which now carries the transcoder listener with the real
  40-service descriptor — across all five modes (auth off / local JWKS / remote JWKS / LAN bind /
  TLS+mTLS), so Envoy itself accepts the transcoder config on the hermetic gate (the descriptor store path
  flows into the check as a build dependency via the spec JSON's string context — IFD-free). Added a
  `rest-descriptor` check (descriptor non-empty; ≥30 all-`agent.v1.*` services) and 13 new four-class +
  adversarial `portal_envoy` unit tests (`LoadServices`, `SpecLoadRest`, `RestTranscoder`: filter order,
  fail-closed transcoder knobs, loopback pin under LAN host, no edge-jwt, cluster reuse, descriptor
  remap, OTLP-key coverage). A live REST curl round-trip is increment 06 (`nix run .#rest-integration`).
  Next: increment 06.

- **Increment 06 — `nix run .#rest-integration` (live REST↔gRPC round-trip).** New `nix/rest-integration.nix`:
  a `writeShellApplication` that boots `agent --serve-all` on `127.0.0.1:50100` (hermetic, model-free,
  no-`[auth]` ⇒ loopback `mode="none"`), brings up the `grpc_json_transcoder` listener via `grpc-web-up`
  (which renders the `rest` block of `envoy-spec.nix`, mounts `agent_descriptor.pb`, `--network host`),
  polls `GET /v1/config/status` ready, then drives nine four-class/adversarial `curl` cases and tears down
  (EXIT trap: `grpc-web-down` + kill the pids it started + `rm -rf`). It **self-skips with exit 0** when no
  container runtime is reachable (same `$CONTAINER_RUNTIME` probe as `pg-integration`), so it folds into
  the model-free tier of `nix/integration.nix` and stays green on a bare box. Representative RPC =
  `ConfigService.GetValues` (`GET /v1/config/values`), served on the gateway because `--config` gives the
  CLI a `source_path` (the exact condition, in `builder.rs`, that wires the config seam). Cases: (1) the
  read is 200 and its body carries `.values`; (2) **parity** — `grpcurl … ConfigService/GetValues` agrees
  (also carries `.values`); (3) `GET /v1/config/schema` 200; (4) an unmapped path is 404
  (`match_incoming_request_route`); (5) empty `validate` body handled (200), not 500; (6) malformed JSON →
  400 at the transcoder, never 500; (7) unknown body field ignored (non-5xx); (8) a percent-encoded
  traversal under a real served prefix → 4xx (never 5xx / file read); (9) an ~8MB body in a real field
  fails closed (non-5xx) **and** the gateway survives (re-probed healthy — the OOM assertion). Registered
  in `nix/default.nix` (`let` def with `inherit (portal) grpc-web-up grpc-web-down`, added to `mkApps` +
  threaded into the `integration` derivation) and `nix/integration.nix` (arg + `runtimeInputs` + a
  `run_step` in the model-free tier beside `pg-integration`). No proto/Envoy-spec change (05's `rest` block
  + descriptor are reused as-is). Gate green. **Deviation from the plan's traversal case:** the plan curled
  the fleet capture route (`/v1/fleet/sessions/{id}`), but the fleet seam is unwired under the hermetic
  config → that route would 501 (UNIMPLEMENTED, a 5xx) and muddy the "never 5xx" invariant; retargeted the
  traversal at the always-served config prefix, which 404s deterministically. Next: increment 07
  (`nix run .#rest-bench`).

- **Increment 07 — `nix run .#rest-bench` (REST-vs-gRPC latency; the number behind "recommend gRPC").**
  New `nix/rest-bench.nix`: a `writeShellApplication` that reuses `harness.serveWire` (`dial_for tcp` +
  `start_serve_all tcp` boot `agent --serve-all` on `127.0.0.1:50100` + its health-wait + auto-clean
  `$work`/EXIT trap), brings up the transcoder (`grpc-web-up`, REST at `:8094`), then measures the SAME
  read two ways against the SAME gateway: the **gRPC leg** = `ghz --insecure --call
  agent.v1.ConfigService.GetValues -d '{}' -c $CONC -n $REQS` (reflection); the **REST leg** = a
  concurrent `seq $REQS | xargs -P $CONC … curl -w '%{time_total}'` loop at `GET /v1/config/values`.
  Prints a side-by-side p50/p95 (+ gRPC rps/avg) table, the transcoding-overhead delta (REST − gRPC),
  and an operational note (`agent_descriptor.pb` byte size + transcoded service count, from
  `rest-descriptor.nix`). Env-overridable `REQS` (default 2000) / `CONC` (default 50). ghz percentiles
  are ns→ms via `jq`; curl percentiles are nearest-rank over the sorted per-request `time_total` (s→ms)
  via `awk`. Self-skips (exit 0) with no container runtime; asserts each leg produced data (`note_fail 1`
  otherwise) — a bench measures, it never `note_fail 2`s. **App only** — registered in `nix/default.nix`
  (`let` def + `mkApps`), deliberately **not** a `check` and **not** in `nix/integration.nix` (throughput
  is machine-dependent and needs a live server + container). Gate green (flake-eval + shellcheck-build).
  **Track complete.** A live l2 run (podman) will fill in the concrete delta number.

- **Post-verification finding (2026-09-28) — the "+12ms overhead" was a bench artifact; it exposed a real
  ~40ms Nagle stall.** The first l2/podman run reported gRPC p95 ~30ms vs REST p95 ~42ms → "+12ms
  transcoding overhead". That number is **not** transcoding cost: the bench compared a **pooled** gRPC leg
  (`ghz --connections 8`) against a **fresh-process/fresh-TCP-per-request** REST leg (`xargs curl`), and the
  two opposite confounds canceled at p50 while the p95 tail was a connection-reuse difference, not the Envoy
  hop. JSON↔protobuf transcoding actually measures ~1–2ms. The diagnostic instead surfaced a genuine bug: a
  classic **Nagle + delayed-ACK stall** (~40ms) on large unary replies over keep-alive connections. Root
  cause is the **agent side** — tonic's `serve_with_incoming_shutdown` does not apply `TCP_NODELAY` to a
  caller-provided incoming stream (only its own `serve(addr)` path does), so accepted gRPC sockets held the
  final small TRAILERS frame until the peer's delayed-ACK timer fired. A raw gRPC client that ACKs promptly
  masks it; the transcoder (which must buffer the whole unary reply before emitting a byte) surfaces it.
  Envoy already sets `TCP_NODELAY` by default (strace-confirmed).
  - **Fix (PR #555):** `enable_nodelay` maps the accepted `TcpListenerStream` in
    `crates/agent-grpc/src/transport.rs` before serving — measured **42ms → 1.20ms** on the 34 KB keep-alive
    path. (An earlier no-op that set `TCP_NODELAY` in the Envoy config, PR #552, was closed as redundant.)
  - **Bench methodology fix (PR #556):** both legs pooled (REST leg → `hey`, pinned `versions.hey`);
    benchmarks `ConfigService.Status` (~116 B) as the headline small-read overhead and keeps `GetValues`
    (~34 KB) as a large-read keep-alive witness. Corrected takeaway: transcoding ≈ 1–2ms/call — still
    "prefer gRPC for hot paths", but the honest number is ~1–2ms, not +12ms.

- **Follow-up: compact transcoder JSON (`add_whitespace: false`).** Re-measuring on l2/podman with the
  now-fair bench + a payload-size probe found pretty-printing nearly **doubles** a config read on the wire:
  `GET /v1/config/values` = 35,078 B pretty → 17,878 B compact (**49% saved**). Flipped
  `grpc_json_transcoder` `print_options.add_whitespace` → `false` in `test/portal-envoy/portal_envoy.py`
  (`always_print_primitive_fields` kept `true` — an API-shape contract, not formatting), guarded by a new
  `RestTranscoder.test_positive_transcoder_emits_compact_json`. Envoy-config file is S14-owned
  (coordinated with the security-hardening session; no conflict). Latency was already healthy post-#555
  (REST large-read p50 ~15ms, no ~40ms stall; small-read overhead ~1.15ms) — this is a payload/CPU win,
  not a latency one. Considered-and-deferred (marginal on this loopback bridge): tonic/Envoy HTTP/2 window
  tuning (wrong flow-control side + payloads under one 64 KB window), STATIC vs LOGICAL_DNS, circuit
  breakers, connection-balance, trace sampling.
