# Portal GUI testing framework — implementation status

The living tracker for the [Portal GUI testing framework](README.md) design. One gated
PR per increment; each becomes review-fleet fodder. Base each PR off `main` — do not
stack (a lesson carried from the portal + code-review tracks).

## Increments

| # | Increment | Portal code | testkit | Nix check/app | ClickHouse | Status |
|---|---|:--:|:--:|:--:|:--:|:--:|
| 00 | Design docs (`docs/design/portal-gui-testing/`) | — | — | — | — | ✅ #421 |
| 01 | Widget-key scheme (inline `Key('<page>.<element>')` across all pages) | ✅ | — | — | — | ✅ #423 |
| 02 | `portal-testkit` fakes (in-proc fake gRPC + recording) + `flutter_test`/`integration_test` deps + regen `pubspec.lock` + config-diff extraction + L0 unit tests | ✅ | ✅ | — | — | ✅ #424 |
| 03 | Layer A `portal-widget` check + robots + completeness critic + Launch & Prompts tabled (template) | ✅ | ✅ | ✅ | — | **this PR** |
| 04 | Remaining pages tabled in Layer A (Graph, Agent, Router, Fleet, Settings) | ✅ | ✅ | — | — | **this PR** |
| 05 | Golden + a11y `portal-visual` check (Launch & Prompts slice; matrix extends) | ✅ | ✅ | ✅ | — | **this PR** |
| 06 | Contract drift guard (RPC set + rendered enums vs descriptor set) — `test/meta/contract_test.dart`, runs in `portal-widget` | — | ✅ | ✅ | — | **this PR** |
| 07 | Layer B `portal-e2e` app (metrics-delta + read-RPC + curated span, trace linking) | ✅ | ✅ | ✅ | — | **this PR** |
| 08 | Report renderer + aggregation (+ failure artifacts) — `nix run .#portal-test-report` (hermetic slice) | — | — | ✅ | — | ✅ 640892b |
| 09 | Performance tracking — `portal_gui_perf` table + JSONEachRow emitter + SQL + Grafana | — | ✅ | ✅ | ✅ | **this PR** |
| 10 | Envoy full instrumentation — OTLP access logs + tracing + CORS trace-context + `envoy_access_latency` | — | — | ✅ | ✅ | **this PR** |

## Dependency order

- **01** is the prerequisite for everything (the widget keys). It ships **keys only** —
  inline `Key('<page>.<element>')` literals (the design's "source scan" registry option),
  so every page could be keyed in parallel with zero shared-file contention, and the
  change is fully verifiable by the existing `dart-analyze` gate.
- **02** adds the `flutter_test`/`integration_test` SDK deps, regenerates the tracked
  `portal/pubspec.lock` (needed for the hermetic `buildFlutterApplication` vendoring, as
  [`dart-analyze`](../../../nix/checks/dart-analyze.nix) already relies on), and lands the
  `portal-testkit` fakes — an **in-process fake gRPC gateway** on an ephemeral loopback
  port that records every call + scripts responses, dialed through a real `PortalClients`
  (the key-feasibility claim, proven by `test/testkit/fake_gateway_test.dart`). It also
  extracts the Settings config-diff into a pure `lib/src/config_diff.dart` so it is
  L0-testable, and ships the L0 unit suite (`graph_json`, `graph_library`, `config_diff`).
  **The per-page robots + completeness critic move to 03**, where the `portal-widget` nix
  check first runs `test/` in the gate and the template pages give the robots a consumer —
  writing them earlier would land test-only Dart with nothing exercising it.
  (`portal/.gitignore` narrowed from `/test/` to just the `flutter create`
  `test/widget_test.dart` scaffold so the real suite is tracked.)
- **03** stands up the gated breadth layer (`portal-widget`), adds the robots + completeness
  critic, and tables a template page; **04** fans out.
- **05, 06** are independent hermetic checks and can land in any order after 03.
- **10 (Envoy)** wires the unified `trace_id`; **07** (curated span assertion, trace
  links) and **09** (proxy-vs-backend latency split) lean on it, so 10 can land
  alongside/just before 07/09. Envoy instrumentation is also standalone-valuable
  (production bridge observability), so it may ship early. **Shipped first in the
  live-l2 wave** — config-only, gate stays green; the `envoyproxy/envoy:v1.31` image
  accepts the OTLP access-logger + tracer + collector cluster (`envoy --mode validate`
  → `configuration OK`). One design-listed access-log field, `%UPSTREAM_HANDSHAKE_DURATION%`
  (07 §1), is **not a supported command operator in v1.31** (`Not supported field in
  StreamInfo`) and was dropped; the proxy-vs-backend split still holds on
  `%DURATION% − %RESPONSE_DURATION%` + the upstream duration. `envoy_access_latency`
  materialized view deferred to inc 09 (where the ClickHouse schema work lands).
- **07 (`portal-e2e`)** shipped second in the live-l2 wave — opt-in
  `nix run .#portal-e2e`, NOT on the gate. Live-verified on l2: the curated mutating
  subset (Router `Put`+`Enable`, Prompts `SetActivePersonality`) drove headless
  chromium through the Envoy bridge and each action passed **metrics delta + read-RPC**
  (`3 passed · 0 failed`). See [`04`](04-layer-b-e2e.md) "As built". Notes of record:
  the Dart suite is the driver only (the shell owns assertions via `grpcurl`/`curl`,
  the `serve-smoke` pattern); Layer-A robots are fake-gateway-coupled so Layer B reuses
  the **keys**, not the robots; `flutter drive`'s web `print` does not surface, so the
  suite hands records back via `reportData` → `build/integration_response_data.json`;
  the browser + matched chromedriver are resolved at runtime from the `nixpkgs`
  registry (the flake-pinned `chromium` has no cached binary → would source-build);
  and the curated OTLP span check is best-effort (inc 10 is authoritative). The
  personality select must target a personality **other than** the active one (the page
  no-ops an unchanged selection, firing no RPC).
- **09 (perf → `portal_gui_perf`)** shipped third (last) in the live-l2 wave — folded
  into the same opt-in `nix run .#portal-e2e`, NOT on the gate. See [`06`](06-performance.md)
  "As built". Notes of record: the table lives in the **agent** ClickHouse
  (`agent.portal_gui_perf`, added to `nix/clickhouse/schema.sql` — the doctor drift check
  auto-extends), inserted via HTTP `JSONEachRow` to `:8123`. **Since obs-single-ch-01
  decomposed the obs stack, there is ONE ClickHouse:** `default.otel_traces` now lives on
  the same server as `agent.portal_gui_perf`, so `trace_id` is a real cross-DB **JOIN** —
  Q3 of the canned SQL is a single `INNER JOIN`. *(Originally two servers — the agent's and
  ClickStack's bundled one, isolated by rootless podman — making trace_id a two-step key
  link.)* Per action we record `interaction_ms` (client, from the driver) and
  `grpc_server_ms` (server truth from the `:9700` histogram delta — its `rpc` label is the
  **full** path with **no** `outcome` label, unlike the `_total` counter); `trace_id` is
  looked up in `otel_traces` by the gateway span's **short** op name
  (`SpanAttributes['rpc']`, e.g. `registry.put`), so a captured id is guaranteed to
  resolve there. All parsed values are fail-closed: histogram fields accept only a clean
  non-negative decimal, `value_ms` is validated before the row, and a server-supplied
  `trace_id` is accepted only if hex (else `''`). The insert is **best-effort** (a down CH
  or un-migrated table is a warn, never a contract fail). Grafana ships a
  `grafana-clickhouse-datasource` datasource + `portal-gui-perf` dashboard (plugin
  installed via `GF_INSTALL_PLUGINS`; a pre-existing grafana container must be recreated).
  `iteration=1` for the single live drive (N-iteration warm-up is future work). Also fixed
  an inc-07 carryover: the curated span check grepped for a container named `clickstack`
  but it is `agent-seddon-clickstack` (`versions.clickstackContainerName`).

## Notes / decisions of record

- **Dead scaffold removed** (inc 02): `portal/test/widget_test.dart` deleted and
  `portal/.gitignore` narrowed from `/test/` to just that scaffold file, so the real
  suite is tracked. Including `portal/test/` in a check's source fileset happens in
  inc 03 (the `portal-widget` check); `dart-analyze` still excludes it.
- **Report renderer (inc 08)** is `nix run .#portal-test-report -- <jsonl>...` (Python behind a
  shell shim, `test/portal-report/render.py`, gated by `portal-report-tests`). The **hermetic
  slice** consumes the checks' `flutter test --machine` streams (page/case/outcome/duration keyed
  by test name) and is forward-compatible with the design's richer per-case records (element_id,
  backend, rpc_fired, trace_id, artifacts) — Layer B's `portal-e2e` will append those (inc 07), at
  which point the backend-down legend and per-element rows populate. Renderer stays green (it is a
  report, not a gate) unless `--fail-on-fail`.
- **Visual/a11y (inc 05)** is `portal-visual` — `matchesGoldenFile` + `meetsGuideline` over
  `test/visual/`. Goldens are generated and checked on the **same pinned `versions.flutter`**, so
  the nix sandbox reproduces them byte-for-byte (verified: a corrupted golden fails the check).
  Landed as a **representative slice** (Launch + Prompts, light) — the template; extending to the
  other five pages and the {dark}×{narrow,wide}×{large-text} matrix is mechanical follow-on
  (regenerate with `flutter test --update-goldens test/visual`).
- **Perf is trend-tracking, not a hard gate** — wall-clock GUI latency is noisy;
  regressions surface via SQL/Grafana, not a red build. iai-callgrind Ir ceilings stay
  the deterministic micro-perf gate.
- **Contract drift guard (inc 06)** is `test/meta/contract_test.dart` (runs inside `portal-widget`,
  no new check): every spec `expectedRpc` must resolve to a real method on its generated
  `*ServiceBase` (`$lookupMethod`), and the **hardcoded** `graphEdgeKinds` list must cover
  `GraphEdge_Kind` (minus unspecified). Enums the UI renders straight from `Enum.values`
  (TaskMode/RouteRole/PoolTier) can't drift, so they need no guard. It also records the invoked
  RPC set per service (the "unwired stubs" reference).
- **Backend preflight tags "down" vs "broken"** so an unrun seam reads as *skipped*, not
  a failure — the fix for the "lots of pages don't work" report is first to *tell* which
  is which.
- **Real-loopback pump recipe (inc 03, load-bearing for every page test)**: pages dial the
  fake gRPC over a real ephemeral loopback socket, but `flutter_test`'s fake clock does not
  advance real socket I/O — so `pumpWidget` and teardown must run inside `tester.runAsync`,
  and `Robot.pumpUntil` alternates a real-async step with a `pump()` to settle deterministically
  (no fixed sleeps). Pump on a desktop-sized surface (the default 800×600 overflows). Select a
  `DropdownButton` item by its **text** with `.hitTestable()` (the keyed `DropdownMenuItem` has an
  offstage IndexedStack twin). Drain trailing timers before test end — the SnackBar auto-dismiss
  and grpc-dart's HTTP/2 **connection idle timeout** (5 min) both leak as `!timersPending`
  otherwise. All centralised in `test/testkit/robots/robot.dart`.
- **Two more real-loopback gotchas (inc 04, every page robot must handle):**
  1. **grpc-dart's `channel.shutdown()` wedges indefinitely on an in-flight RPC.** A page that
     issues an RPC whose response is still arriving at teardown (e.g. Settings' trailing
     `Status`, a `Validate`/`Put` reply) hangs the test. Fix: a `quiesce()` that advances a few
     **real-time** windows (`runAsync` + `pump`) so every fired call's response lands and the
     channel is idle before shutdown — end RPC-firing tests with it.
  2. **initState `Timer.periodic` pollers are REAL timers** (initState runs inside `runAsync`),
     so `tester.pump(Duration)` does NOT fire them — advance with real `Future.delayed`. And the
     teardown must **unmount the page** (`pumpWidget(SizedBox())`) so `dispose` cancels the
     pollers + stream subscription before shutting the wire down, or a leaked real timer races
     finalization (the Agent page).
- **Completeness critic** source-scans `portal/lib` for `Key('…')` stems and fails a tabled page
  that has a stem without a `positive_` row; not-yet-tabled pages are logged as *pending*
  (inc 04 burns the pending count to 0), never silently skipped.
- **Reuse existing infra**: hermetic Flutter (`buildFlutterApplication`), the gRPC
  metrics/spans the harness already emits, ClickStack/ClickHouse + Grafana on l2, and
  the `portal-redeploy` full-stack bring-up.

## Deferred (documented, not scoped)

- Optional non-blocking PR perf-annotation bot.
- Wiring the currently-unwired stub methods into the UI (each would then need spec rows;
  the completeness critic will flag them).
- A Dart-client `traceparent` injector so the browser is the trace root (Envoy starting
  the trace is sufficient for v1).
