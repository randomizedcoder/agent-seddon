# 01 — Architecture

The framework is a **test pyramid** whose two operating modes are **Layer A** (hermetic
breadth) and **Layer B** (live depth). This doc explains the shape, the one injection
seam that makes the breadth layer possible, the patterns that keep it maintainable, and
the rules that keep it deterministic.

## The pyramid

| Tier | What | Runtime | Backend | On the gate? |
|---|---|---|---|---|
| **L0** unit | pure Dart logic (`graph_json` round-trip, `schema_form` field↔JSON, Settings config-diff, `GraphLibrary` serialization) | µs, no widget pump | none | ✅ `portal-widget` |
| **L1** widget | every element × case classes, driven via `find.byKey` | ms, sharded isolates | in-process **fake gRPC server** | ✅ `portal-widget` |
| **L1-golden** visual | one golden per page/component, light+dark, fixed sizes, responsive/text-scale | ms | fake gRPC (fixed data) | ✅ `portal-visual` |
| **L1-a11y** | contrast / tap-target / labeled-tap-target per page | ms | fake gRPC | ✅ `portal-visual` |
| **L2** e2e | real app, real stack, real wire, observability asserts | seconds, curated | **real** `--serve-all`(+sessions+fleet)+envoy | ⛔ opt-in `nix run .#portal-e2e` |

Layers A (L0/L1/golden/a11y) run under `flutter test` on the **Dart VM**, hermetic and
parallel, and join `nix flake check`. Layer B (L2) is the live integration app.

## The injection seam — how a page gets a fake backend

Every page takes its clients as a constructor argument
(`portal/lib/main.dart:90-96` → `PromptsPage(clients: _clients)` etc.), and
`PortalClients` builds each `*ServiceClient` from a `ClientChannel`
(`portal/lib/src/clients.dart:30-58`). The transport is chosen by conditional import
(`portal/lib/src/transport/channel_factory.dart:7-9`): on the **Dart VM** — where
`flutter test` runs — it resolves to `channel_io.dart`, a **real loopback
`ClientChannel`**.

That is the whole trick: a widget test starts an **in-process fake gRPC server**
(generated `*ServiceBase` subclasses that record calls and return scripted responses)
on an **ephemeral loopback port**, points a `PortalConfig` at it, and pumps the page
with `PortalClients(thatConfig)`. The page talks real gRPC over the loopback to the
fake — so the request is *actually serialized and dispatched*, and the assertion
"the correct RPC fired with these args" is proven **at the wire**, not by mocking a
method. Details + the `portal-testkit` fake in [`03-layer-a-widget.md`](03-layer-a-widget.md).

```
   flutter test (Dart VM)
   ┌───────────────────────────────────────────────┐
   │  PromptsPage(clients: PortalClients(testCfg))  │
   │        │ real gRPC over 127.0.0.1:<ephemeral>  │
   │        ▼                                        │
   │  portal-testkit FakePromptService  ──► records (method, decoded request)
   │        (scripted ok / error / boundary)  ◄── returns canned response
   └───────────────────────────────────────────────┘
```

A lighter fallback (inject client doubles via a `PortalClients.fromClients(...)` test
constructor) is available if the in-VM server ever proves heavy, but the loopback fake
is preferred — it exercises the generated stubs and the real request encoding.

## Why `integration_test`, not Playwright, for the live layer

Flutter web **renders to a canvas** — there is no DOM to select, and the semantics tree
is built only on demand with non-unique labels (the exploration found **zero**
test-facing `Key`/`Semantics` in `portal/lib` today). Driving that canvas through
Playwright means fighting the semantics tree; driving it through Flutter's own
`integration_test` + `find.byValueKey` reuses the **same widget-key scheme** Layer A
uses, in-process, with no DOM fragility. So both layers target one key scheme. The
ad-hoc Playwright MCP stays for exploratory/visual one-offs (see the
[obs runbook](../../portal-obs-mcp-runbook.md)); it is not the automation path.

## The Robot (Page-Object) pattern

Each page gets a thin **robot** that wraps its `find.byKey` interactions behind intent
methods — `promptsRobot.tapSave()`, `agentRobot.enterGoal('x')`,
`fleetRobot.expectOfflineBanner()`. Tests read as user intent, and when a `Key` is
renamed or a widget moves it is fixed in exactly one place. Robots are shared by Layer A
and Layer B (same keys), so a page's interaction vocabulary is written once. This is the
single most important thing for "keep adding tests" at scale.

## Backend preflight — "down" vs "broken"

Much of the apparent breakage is a backend that isn't running: the Agent tab needs
`--serve-sessions` (`agent_view_page.dart:276-291`), Fleet needs `--serve-fleet`
(`fleet_page.dart:721-744`), the web build needs the envoy bridge. Before Layer B runs,
a **preflight** probes each seam (a `grpc.health.v1.Health/Check` or a cheap read RPC)
and tags each page `backend: up|down`. A page whose backend is down is reported
**skipped**, never **failed** — so the report distinguishes "not wired up right now"
from "wired wrong". (Layer A has no backend to be down; its fake is always up.)

## Determinism / anti-flake rules

The hermetic layers must never flake — a flake there is a bug to fix, not a retry:

- **Fake/injected clock** for the Agent page's 3 s / 5 s pollers
  (`agent_view_page.dart:57,59`); never a real `sleep`. Advance time explicitly.
- **Ephemeral loopback port** per fake server ⇒ tests are parallel-safe across isolates.
- **Fresh state per case** — a new fake server and cleared `localStorage` (the Graph
  library lives there) between cases; no shared globals.
- **`pumpAndSettle` with bounded timeouts**; no arbitrary delays.
- **Pinned font** (the bundled Source Serif 4) for goldens so pixels are deterministic.
- **No auto-retries in Layers A** — only Layer B may bound-retry a genuine network wait.

## Where it lives in `nix/`

- `portal-widget` and `portal-visual` are new **checks** registered in
  [`nix/checks/default.nix`](../../../nix/checks/default.nix) via a direct import (like
  [`dart-analyze`](../../../nix/checks/dart-analyze.nix) at line 142, since they need
  `versions.flutter`, not craneLib). They build hermetically via
  `versions.flutter.buildFlutterApplication` + offline `autoPubspecLock`.
- `portal-e2e` is a new **app** folded into `apps` via `mkApps`
  (`nix/default.nix:614-654`), reusing the full-stack bring-up already modeled by
  [`portal-redeploy`](../../../nix/portal/default.nix) (build → serve → grpc-web
  health-check).

See [`03-layer-a-widget.md`](03-layer-a-widget.md) and
[`04-layer-b-e2e.md`](04-layer-b-e2e.md) for the concrete recipes.
