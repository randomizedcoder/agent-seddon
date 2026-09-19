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
| 04 | Remaining pages tabled in Layer A | ✅ | ✅ | — | — | ⬜ |
| 05 | Golden + a11y `portal-visual` check | ✅ | ✅ | ✅ | — | ⬜ |
| 06 | Contract drift guard (RPC set + rendered enums vs descriptor set) | — | ✅ | ✅ | — | ⬜ |
| 07 | Layer B `portal-e2e` app (metrics-delta + read-RPC + curated span, trace linking) | ✅ | ✅ | ✅ | — | ⬜ |
| 08 | Report renderer + aggregation (+ failure artifacts) | — | — | ✅ | — | ⬜ |
| 09 | Performance tracking — `portal_gui_perf` table + JSONEachRow emitter + SQL + Grafana | — | ✅ | ✅ | ✅ | ⬜ |
| 10 | Envoy full instrumentation — OTLP access logs + tracing + CORS trace-context + `envoy_access_latency` | — | — | ✅ | ✅ | ⬜ |

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
  (production bridge observability), so it may ship early.

## Notes / decisions of record

- **Dead scaffold removed** (inc 02): `portal/test/widget_test.dart` deleted and
  `portal/.gitignore` narrowed from `/test/` to just that scaffold file, so the real
  suite is tracked. Including `portal/test/` in a check's source fileset happens in
  inc 03 (the `portal-widget` check); `dart-analyze` still excludes it.
- **Perf is trend-tracking, not a hard gate** — wall-clock GUI latency is noisy;
  regressions surface via SQL/Grafana, not a red build. iai-callgrind Ir ceilings stay
  the deterministic micro-perf gate.
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
