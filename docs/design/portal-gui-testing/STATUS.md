# Portal GUI testing framework — implementation status

The living tracker for the [Portal GUI testing framework](README.md) design. One gated
PR per increment; each becomes review-fleet fodder. Base each PR off `main` — do not
stack (a lesson carried from the portal + code-review tracks).

## Increments

| # | Increment | Portal code | testkit | Nix check/app | ClickHouse | Status |
|---|---|:--:|:--:|:--:|:--:|:--:|
| 00 | Design docs (`docs/design/portal-gui-testing/`) | — | — | — | — | **this PR** |
| 01 | Widget-key scheme + per-page robots + completeness critic | ✅ | ✅ | — | — | ⬜ |
| 02 | `portal-testkit` fakes + `flutter_test`/`integration_test` deps + regen `pubspec.lock` + L0 unit tests | — | ✅ | — | — | ⬜ |
| 03 | Layer A `portal-widget` check + first 1–2 pages tabled (template) | ✅ | ✅ | ✅ | — | ⬜ |
| 04 | Remaining pages tabled in Layer A | ✅ | ✅ | — | — | ⬜ |
| 05 | Golden + a11y `portal-visual` check | ✅ | ✅ | ✅ | — | ⬜ |
| 06 | Contract drift guard (RPC set + rendered enums vs descriptor set) | — | ✅ | ✅ | — | ⬜ |
| 07 | Layer B `portal-e2e` app (metrics-delta + read-RPC + curated span, trace linking) | ✅ | ✅ | ✅ | — | ⬜ |
| 08 | Report renderer + aggregation (+ failure artifacts) | — | — | ✅ | — | ⬜ |
| 09 | Performance tracking — `portal_gui_perf` table + JSONEachRow emitter + SQL + Grafana | — | ✅ | ✅ | ✅ | ⬜ |
| 10 | Envoy full instrumentation — OTLP access logs + tracing + CORS trace-context + `envoy_access_latency` | — | — | ✅ | ✅ | ⬜ |

## Dependency order

- **01** is the prerequisite for everything (keys + robots).
- **02** adds the test deps + fakes; regenerating the tracked `portal/pubspec.lock` is
  required for the hermetic `buildFlutterApplication` vendoring (the tracked
  `portal/pubspec.lock` feeds it, as [`dart-analyze`](../../../nix/checks/dart-analyze.nix)
  already relies on).
- **03** stands up the gated breadth layer with a template page; **04** fans out.
- **05, 06** are independent hermetic checks and can land in any order after 03.
- **10 (Envoy)** wires the unified `trace_id`; **07** (curated span assertion, trace
  links) and **09** (proxy-vs-backend latency split) lean on it, so 10 can land
  alongside/just before 07/09. Envoy instrumentation is also standalone-valuable
  (production bridge observability), so it may ship early.

## Notes / decisions of record

- **Delete the dead scaffold** `portal/test/widget_test.dart` in increment 02 and
  include `portal/test/` in the new checks' source filesets (today `dart-analyze`
  excludes it).
- **Perf is trend-tracking, not a hard gate** — wall-clock GUI latency is noisy;
  regressions surface via SQL/Grafana, not a red build. iai-callgrind Ir ceilings stay
  the deterministic micro-perf gate.
- **Backend preflight tags "down" vs "broken"** so an unrun seam reads as *skipped*, not
  a failure — the fix for the "lots of pages don't work" report is first to *tell* which
  is which.
- **Reuse existing infra**: hermetic Flutter (`buildFlutterApplication`), the gRPC
  metrics/spans the harness already emits, ClickStack/ClickHouse + Grafana on l2, and
  the `portal-redeploy` full-stack bring-up.

## Deferred (documented, not scoped)

- Optional non-blocking PR perf-annotation bot.
- Wiring the currently-unwired stub methods into the UI (each would then need spec rows;
  the completeness critic will flag them).
- A Dart-client `traceparent` injector so the browser is the trace root (Envoy starting
  the trace is sufficient for v1).
