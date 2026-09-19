# Design: Portal GUI testing automation framework

Status: **design of record** — this directory specifies the framework; nothing is
built yet. [`STATUS.md`](STATUS.md) is the authoritative increment tracker (one gated
PR per increment). Same convention as [`../portal/`](../portal/README.md),
[`../review-fleet/`](../review-fleet/README.md) and the other design tracks.

## Why

Clicking around the [Agent Portal](../portal/README.md), a lot of pages *appear*
broken. Most of that is **not** a UI bug — it is a backend that isn't running (the
Agent tab needs `--serve-sessions`, the Fleet tab needs `--serve-fleet`, the web build
needs the envoy bridge) and errors that fold silently into SnackBars. But we cannot
*tell* the difference today, because the portal has **no automated tests at all**: the
only `*_test.dart` is the dead `flutter create` scaffold, deliberately excluded from
the [`dart-analyze`](../../../nix/checks/dart-analyze.nix) gate. Every regression — the
"all nav icons went white" class, a mis-wired button, a chatty extra RPC, a page that
got slower — ships unnoticed.

This framework makes the portal **exhaustively, quickly, and repeatably testable**:

1. **Every page, every element/option** is exercised.
2. It runs **in parallel, in seconds** (the breadth layer is hermetic and shards
   across isolates).
3. It is **table-driven** — one row per case, with `positive_ / negative_ / boundary_ /
   corner_` (and `adversarial_` for untrusted inputs) classes, each row carrying a
   **description** and an **expected outcome** — mirroring the repo's Rust test
   convention (`rstest` `#[case::<prefix>_…]`, [`crates/agent-tools/src/edit.rs`]).
4. It produces a **per-page, per-element pass/fail report**.
5. It **verifies through the observability system** that a GUI action fired the
   **correct backend gRPC call** and that the **intended state changed** — and records
   **latency** for every step so slowdowns are caught, not just breakage.
6. It is **easy to grow**: adding a test is adding a table row; adding an element is
   adding its `Key` and its rows, and a completeness critic fails the build if any
   element has no test.

It leans entirely on the existing pieces — `flake.nix` + the modular `nix/` design, the
hermetic Flutter toolchain already used by `dart-analyze`, the gRPC seams + metrics +
OTLP tracing the harness already emits, and the ClickStack/ClickHouse + Grafana stack
already running on l2.

## The test pyramid at a glance

```
        slow / few                                   ┌───────────────────────────────┐
            ▲                                         │ L2  live e2e + observability  │  ← Layer B
            │   integration_test → real stack         │  (curated mutating subset)    │    (opt-in app
            │   metrics-delta + read-RPC + OTLP span   └───────────────────────────────┘     nix run .#portal-e2e)
            │                                         ┌───────────────────────────────┐
            │   golden (light/dark, sizes, a11y)      │ L1-golden · L1-a11y           │  ← gated check
            │                                         └───────────────────────────────┘    (portal-visual)
            │   widget + in-process fake gRPC          ┌───────────────────────────────┐
            │   every element × case classes           │ L1  widget (breadth)          │  ← gated check  ★ the bulk
            │                                          └───────────────────────────────┘    (portal-widget)
            │   pure Dart logic                        ┌───────────────────────────────┐
            ▼   graph_json · schema_form · config-diff │ L0  unit                      │  ← gated check
        fast / many                                    └───────────────────────────────┘
```

- **Layer A** = L0 + L1 + golden + a11y — **hermetic, milliseconds, massively
  parallel, on `nix flake check`.** This is where "every element in a few seconds"
  lives. Backend is an in-process **fake gRPC server** ([`portal-testkit`](03-layer-a-widget.md))
  that records every call and returns scripted responses.
- **Layer B** = the live end-to-end layer — the real app driven by
  `integration_test` against a real `--serve-all` (+`--serve-sessions`
  +`--serve-fleet`) + envoy bridge, asserting the **real wire** through the
  observability system. Opt-in (`nix run .#portal-e2e`), a **curated** subset, not on
  the source-build gate.

Both share **one source of truth** — the per-page [test-spec table](02-test-spec.md) —
and both emit into **one report** ([`05-report.md`](05-report.md)) and **one perf
table** ([`06-performance.md`](06-performance.md)).

## The documents

| Doc | Covers |
|---|---|
| [`01-architecture.md`](01-architecture.md) | The pyramid, the fake-gRPC injection seam, the Robot pattern, `integration_test`-over-Playwright, the backend preflight, determinism rules |
| [`02-test-spec.md`](02-test-spec.md) | The table schema + case taxonomy + the exact-RPC-set rule + worked rows for all 7 pages |
| [`03-layer-a-widget.md`](03-layer-a-widget.md) | `portal-testkit` fakes, the widget-key scheme, per-page robots, the `portal-widget` nix check, L0 unit tests, the completeness critic + contract drift guard |
| [`03b-visual-and-a11y.md`](03b-visual-and-a11y.md) | Golden/visual regression + accessibility guidelines |
| [`04-layer-b-e2e.md`](04-layer-b-e2e.md) | `integration_test` live e2e, metrics-delta + read-RPC + OTLP-span assertions, `nix run .#portal-e2e` |
| [`05-report.md`](05-report.md) | The report JSON schema, renderer, failure artifacts |
| [`06-performance.md`](06-performance.md) | Latency capture + the `portal_gui_perf` ClickHouse table + cross-PR SQL |
| [`07-envoy-otel.md`](07-envoy-otel.md) | Fully instrumenting the Envoy bridge (OTLP logs + tracing) so any slow call is traceable via SQL |
| [`STATUS.md`](STATUS.md) | Increment roadmap + per-increment status |

## Security posture (inherited, not invented)

The harness rule holds: **operator and model input is untrusted, fail closed**
([CLAUDE.md](../../../CLAUDE.md)). The framework *tests that posture at the UI* — the
resilience matrix ([`02`](02-test-spec.md)) drives hostile payloads (huge/invalid JSON,
unicode/RTL/injection strings, oversized markdown) and asserts the truncation banners
and the **inert-image / no-remote-fetch** behavior hold; it never relaxes them. The
test harness itself only *reads* observability (metrics text, read-RPCs, ClickHouse
queries) and drives loopback endpoints.

## Non-goals

- **Not a hard wall-clock perf gate.** Latency is recorded and trend-tracked in
  ClickHouse; regressions surface as a query/dashboard signal, not a red build. The
  deterministic instruction-count gate stays [iai-callgrind](../../components/benchmarking.md).
- **Not a replacement for the ad-hoc Playwright MCP** used for exploratory/visual
  checks — the live layer uses `integration_test` instead (canvas has no DOM), but
  Playwright stays available for one-off manual driving.
- **No new deployment story.** Everything runs over loopback/UDS on l2, exactly as the
  seams do today.
