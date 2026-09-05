# Agent Portal — implementation status

The living tracker for the [Agent Portal](README.md) design. One gated PR per
increment. Backend seams (02–04) are independent and can land in any order; the
Flutter app (06) depends on the Dart codegen (05), which depends on the protos
existing (02–04).

## Increments

| # | Increment | Proto | Seam | Wire | Tests | Nix | Status |
|---|---|:--:|:--:|:--:|:--:|:--:|:--:|
| 01 | Design docs (`docs/design/portal/`) | — | — | — | — | — | **merged** (#127) |
| 02 | `PromptService` + mode-lens externalization | ✅ | ✅ | ✅ | ✅ | n/a | **merged** (#128) |
| 03 | `MetricsProxyService` | ✅ | ✅ | ✅ | ✅ | n/a | **merged** (#129) |
| 04 | `AgentSessionService` + broadcast event-sink | ✅ | ✅ | ✅ | ✅ | n/a | **merged** (#130) |
| 05 | Dart codegen + nix tooling (`buf.gen.yaml`, `gen-dart`, proxy) | — | — | — | — | ✅ | **merged** (#131) |
| 06 | Flutter app (transport, Launcher, Prompts, Agent View) | — | — | — | ✅ | ✅ | **in review** |
| 07 | `ConfigService` seam + `FileConfigStore` (schema · write-back · drift) — [05](05-config-and-router.md) | ✅ | ✅ | ✅ | ✅ | ✅ | **implemented** (branch `feat/portal-config-settings`) |
| 08 | Router tab (ProviderRegistry CRUD) + Settings tab (`SchemaForm`) — [05](05-config-and-router.md) | — | — | — | ✅ | — | **implemented** (branch `feat/portal-config-settings`) |

## 02 — what shipped

- **Mode-lens externalization.** The compiled `lens_instruction` strings moved into
  a feature-ungated `agent_context::lens` module (`GENERIC` + per-mode defaults +
  `builtin_instruction` + `ALL_MODES`); `summarizing::DEFAULT_INSTRUCTION` is now an
  alias to `lens::GENERIC` (one source of truth). `ModeAwareWindow` gained a
  `with_lens_dir` builder + a `LensPrompts` resolver — an operator file at
  `<prompts>/lens/<mode>.md` overrides the default, re-read **live** per
  switch-compaction; no dir ⇒ allocation-free `Cow::Borrowed` default (partition
  bench + leak budget unaffected). Behaviour with no files is byte-identical (57
  agent-context tests + leak still green).
- **`PromptStore` seam.** Trait + `PromptKind`/`PromptRef`/`PromptEntry` DTOs +
  `Error::Prompt` in `agent-core`; `FilePromptStore` in the new **`agent-prompt`**
  crate (CRUD over `context.d` + `prompts/`, `resolve_system_prompt` startup hook).
  System/prepend/append edits take effect next run; mode-lens edits are live.
- **Wire.** `prompt.proto` (`PromptService`: List/Get/Put/Delete/PreviewAssembled),
  added to `build.rs` + the descriptor-set test; conversions in `convert.rs`;
  `PromptSvc` server + `GrpcPrompts` client; `prompt` row in `constants.nix`
  (50077 / 9627) regenerated into `constants.rs`. Served via `--serve-prompt` /
  `--serve-all` (built in `builder.rs`, attached to the agent, `add_seam_service`
  arm). Additive proto — no `buf.image.binpb` bump.
- **Security.** `safe_prompt_file` + `confine` guard the untrusted `id`; content
  size-capped; adversarial traversal/oversize/unknown-mode cases asserted, including
  **over the wire on TCP + UDS** (`roundtrip.rs`).
- **Docs/config.** `[prompts] dir` in `config/agent.toml`; component doc
  `docs/components/prompt.md`.
- Bench+leak: **n/a** — no new hot path (the default lens lookup is allocation-free
  and off the compaction critical section).

## 03 — what shipped

- **`MetricsProxy` seam.** Trait + `PromQuery`/`PromRangeQuery`/`PromResult`/
  `PromSeries`/`PromSample` + `Error::Metrics` in `agent-core`. New
  **`agent-metrics-proxy`** crate: `HttpMetricsProxy` (reqwest → Prometheus
  `/api/v1/query[_range]`) + a **pure** `parse_prom_response` (vector/matrix/scalar,
  non-finite values, error/oversized/malformed) unit-tested against canned envelopes.
- **Fail-soft, read-only, capped.** Query length + series + per-series sample counts
  capped before buffering; upstream time-bounded; raw error body never forwarded
  (`errorType` class only). Every failure folds into an empty `PromResult{error}` —
  never `Err`, never a panic. Oversized-query + unreachable-upstream asserted.
- **Wire + serve.** `metrics_proxy.proto` (`MetricsProxyService`: Query/QueryRange),
  added to `build.rs` + the descriptor-set test; `From`/`From` conversions;
  `MetricsProxySvc` server + `GrpcMetricsProxy` client (fail-soft end to end);
  `metrics_proxy` row in `constants.nix` (**50079 / 9629**, with 50078/9628 reserved
  for session_stream) regenerated into `constants.rs`. Built in `builder.rs`,
  attached to the agent, served via `--serve-metrics-proxy` / `--serve-all`. Round-trip
  asserted on **TCP + UDS** via a double. Additive proto — no baseline bump.
- **Docs/config.** `[metrics_proxy] prometheus_url` in `config/agent.toml`;
  component doc `docs/components/metrics-proxy.md`.

## 04 — what shipped

- **Event types + source.** `SessionEvent` / `StatusSnapshot` / `SessionSource` (+
  `SessionEventStream`) in `agent-core`. `agent_runtime::SessionEvents` — a bounded
  `tokio::sync::broadcast` sink + a shared `StatusSnapshot`, on the `Agent` (so
  `&Agent`-only loop methods publish). Impl `SessionSource` (`snapshot` reads shared
  state; `subscribe` → `BroadcastStream`, dropping `Lagged`).
- **Publishing at existing sites — no new control flow.** Run start/finish
  (`Session::send`), iteration (`on_iteration`), context (`set_context`), tool
  start/result, mode switch (`record_mode_switch`), token deltas (streaming echo,
  **guarded on `has_subscribers()`** so an unobserved run allocates nothing extra).
  Snapshot-affecting events update the shared snapshot.
- **Wire + serve.** `agent_session.proto` (`Subscribe` server-stream + `Snapshot`;
  `SessionModeSwitch` renamed to dodge mode.proto's `ModeSwitch`), added to
  `build.rs` + descriptor-set test; core→proto conversions + `snapshot_event`.
  `AgentSessionSvc` server (snapshot-first then live tail); **no Rust client** —
  `SessionSource::snapshot` is sync so a gRPC client couldn't implement it; only the
  Dart portal consumes this. `session_stream` port claimed (**50078 / 9628**).
  Served via `--serve-session-stream` / `--serve-all`. Round-trip (stream + snapshot,
  oneof mapping) asserted on **TCP + UDS** via a double.
- **Observe a *running* agent.** `grpc_server::serve_session_observe` hosts only the
  service with no self-shutdown; a one-shot goal / the REPL runs it as a concurrent
  `tokio::select!` branch (sharing the live source) when `[grpc.session_stream]
  listen` is set — so the portal attaches to a running agent. Default off.
- **Carried fix.** inc-03's `metrics-proxy` feature now enables
  `agent-metrics-proxy/http` explicitly (it only compiled under full-workspace
  feature unification before — `-p agent-runtime` alone was broken).
- **Docs/config.** `[grpc.session_stream] listen` in `config/agent.toml`;
  `docs/components/agent-session.md`.
- **Deferred (documented):** a `Send` RPC to drive a goal remotely
  (`--serve-mcp`-class); precise per-tool `duration_ms` (currently 0 — timing is in
  `agent_tool_exec_seconds` via MetricsProxy).

## 05 — what shipped

- **Dart codegen.** `buf.gen.yaml` (repo root) with a **local** `protoc-gen-dart`
  plugin (`opt: grpc` → `*.pbgrpc.dart` service clients) — hermetic, no BSR/network.
  buf's first *generation*; Rust stays on tonic-build. `nix run .#gen-dart`
  regenerates the **committed** stubs under `portal/lib/src/gen/` (99 files; verified
  the `AgentSessionServiceClient` / `PromptServiceClient` etc. generate correctly).
- **Nix tooling.** `protoc-gen-dart` + `flutter` + `dart` pinned in `versions.nix`;
  `envoyImage` a pinned docker tag. **Deliberately NOT in `allDevPackages`** — the
  portal apps supply their toolchain via `runtimeInputs`, so the lean Rust dev shell
  is unaffected (flutter/envoy would bloat it).
- **Apps** (`nix/portal/default.nix`): `gen-dart`; `grpc-web-up`/`grpc-web-down` — the
  web-only grpc-web proxy as an **envoy docker container** (prometheus/clickstack
  pattern; CORS + HTTP/2 to gateway `:50100`), so the gate never source-builds envoy.
- **Scaffold.** `portal/README.md` + `portal/.gitignore` (build artifacts ignored;
  the generated stubs are committed). The `nix run .#portal` runner + pubspec + app
  code land with the app in **increment 06**.
- **Verification.** `nix flake check` green (nixfmt + the apps build fast — docker +
  cached protoc-gen-dart, no source builds). `nix run .#gen-dart` regenerates cleanly.

## 06 — what shipped (the finale)

- **The Flutter app** (`portal/`): `pubspec.yaml` (grpc/protobuf/fixnum/url_launcher),
  `analysis_options.yaml`, and `lib/`.
- **Transport abstraction** (`lib/src/transport/`): conditional imports pick
  `channel_io.dart` (native `ClientChannel` → gateway `:50100`) vs `channel_web.dart`
  (`GrpcWebClientChannel` → the envoy proxy) at compile time, behind one
  `createGatewayChannel`. `PortalClients` builds all four service clients
  (Prompt/MetricsProxy/AgentSession/LlmPool) from the single channel.
- **Three views** (`lib/src/pages/`) off a `NavigationRail`:
  - **Launcher** — cards that open Grafana/HyperDX/Prometheus in the system browser
    (`url_launcher`).
  - **Prompts** — CRUD over `PromptService`: list grouped by kind (🔒 on defaults),
    edit + Save/Reset, and "Preview assembled" (`PreviewAssembled`).
  - **Agent View** — live transcript from `AgentSessionService.Subscribe` (token
    deltas coalesced, tool/mode/run markers) + a status bar: mode & context from the
    stream, GPU pool from `LlmPoolService.Health` (3s poll), gRPC p50/p99 from
    `MetricsProxyService` (5s poll, canned `histogram_quantile`). Fails soft — a down
    gateway greys the cell / offers reconnect.
- **`nix run .#portal`** builds/launches it (native by default; `-- -d chrome` for
  web); it bootstraps the git-ignored platform runners (`flutter create`) on demand,
  so only `lib/` + pubspec are committed.
- **Verification:** `flutter analyze` — **No issues found** (pub get resolved
  grpc 4.2 / protobuf 4.2). The `nix flake check` gate is Rust/nix and does not
  compile Dart, so `flutter analyze` is the app's verification.

**The 6-increment Agent Portal track is complete** (design #127; PromptService #128;
MetricsProxy #129; AgentSession #130; codegen+tooling #131; app #132).

## 07–08 — configuration & the model router (what shipped)

Design of record: [`05-config-and-router.md`](05-config-and-router.md). Two more
tabs on the finished app, both **verified live** against a gateway with the Playwright
MCP; on branch `feat/portal-config-settings` (not yet merged).

- **07 · `ConfigService` seam.** New `config.proto` (`GetSchema`/`GetValues`/
  `Validate`/`Put`/`Status`, values as `JsonValue`); `ConfigStore` trait + typed
  `ConfigEdit`/`ConfigIssue`/`ConfigIssueCode`/`ConfigStatus` + `MAX_CONFIG_*` in
  `agent-core`; `FileConfigStore` + `config_schema` (schemars draft-07 + enum-choices
  and `x-secret` overlay + `validate_config`) in **`agent-runtime`** (folded in to
  avoid an `agent-config`→`agent-runtime` cycle). `toml_edit` sparse, comment-preserving,
  atomic write-back; validate-then-write; secret masking + empty-secret no-op; path
  allowlist + caps. Port **50085 / 9635 / `config.sock`**, `--serve-config` +
  `--serve-all`; additive proto (no baseline bump); 71 roundtrip cases (TCP+UDS, mock
  store). `[grpc.config]` in `config/agent.toml`.
- **08 · Router + Settings tabs.** `SchemaForm` widget (generalises `_ParamsEditor`:
  `$ref`/`allOf` resolution, enum dropdowns, `x-secret` masking, nested objects,
  raw-JSON escape hatch). `settings_page.dart` (two-pane, per-section diff→`Put`,
  Validate dialog, restart-required banner from `Status`). `router_page.dart` (typed
  CRUD over `ProviderRegistryService` — Upstreams · Health · Route tester, applies
  live). `config` + `providers` clients wired in `clients.dart`; two nav destinations.
  `flutter analyze` clean.
- **Deviations / v1 limits** (see the doc): store folded into `agent-runtime`;
  `GetValues` shows the running snapshot (form shows live value + pending banner after
  Save); array-element enums are free-text (whole-array replacement, no indexed paths);
  only inline secrets masked; no restart button; `schemars 0.8` pulls the unmaintained
  `proc-macro-error2` (gates stay green; `schemars 1.0` is the escape hatch).

## Dependency order

- **02, 03, 04 are independent** backend seams — each is a self-contained
  seam-to-wire addition gated by `nix flake check`. Land in whatever order is
  convenient; each delivers standalone value (02 = see/CRUD prompts via `grpcurl`
  today; 03 = PromQL over gRPC; 04 = live loop stream).
- **05** needs the protos from 02–04 to generate against.
- **06** consumes 05's stubs + the three seams + the existing `LlmPoolService.Health`.

## Notes / decisions of record

- **Base each PR off `main`** — do not stack (a lesson carried from the code-review
  track; see the memory index).
- **Mode-lens externalization keeps compiled defaults**, so 02 changes no behaviour
  with no operator files present and the gate stays green.
- **`AgentSessionService.Send`** (drive a goal remotely) is deliberately **out of
  increment 04** — observe-only first. It is `--serve-mcp`-class (arbitrary agent
  execution) and lands later, loopback/UDS only, with the caveat documented.
- **The grpc-web proxy is web-only and optional** — the native desktop build dials
  `:50100` directly.

## Deferred (documented, not scoped here)

- A `constants-sync`-style **drift check for the committed Dart stubs**.
- **mTLS / cross-host** transport for the portal (tracks the existing gRPC follow-up
  in [`grpc.md`](../../grpc.md#possible-follow-ups); the portal inherits whatever the
  transport layer grows).
- **Per-mode *system* prompts** (not just per-mode compaction lenses). Today only the
  compaction lens is per-mode; a per-mode system prompt would be a separate design on
  the `ContextStrategy`/prompt seams.
- Folding the status-bar reads into a single `AgentSessionService` push (pool + p50/p99
  currently poll their own services) — a later consolidation if the poll cadence
  proves noticeable.
