# 04 — Layer B: live end-to-end + observability

Layer A proves the GUI *calls* the right RPC against a fake. Layer B proves the **real
wire**: the real app, driven headlessly, against a real gateway + envoy bridge, with the
assertion coming from the **observability system** — "the correct backend gRPC call
fired" **and** "the intended state changed." It is a **curated subset** (the mutating
actions), opt-in as `nix run .#portal-e2e`, not on the source-build gate (it needs a
browser + the running stack).

## Driver — `integration_test` + `find.byValueKey`

The live app is driven by Flutter's own `integration_test` (not Playwright — canvas has
no DOM; see [`01`](01-architecture.md)), reusing the **same widget keys and robots** as
Layer A. Target is the web build (through the envoy grpc-web bridge), so the path under
test is exactly the browser's: `browser → envoy → gateway → seam`. A browser +
chromedriver are vendored via `nixpkgs#playwright-driver.browsers` (the version-matching
approach the [obs runbook](../../portal-obs-mcp-runbook.md) documents), or the Linux
embedder is run headless.

## Stack bring-up (reuse `portal-redeploy`)

The app reuses the full-stack bring-up already modeled by
[`portal-redeploy`](../../../nix/portal/default.nix) (build → serve → grpc-web
health-check): start `agent --serve-all` (`:50100`, metrics `:9700`), and — so the
Agent and Fleet tabs are actually *up*, not skipped — `--serve-sessions` (`:50080`) and
`--serve-fleet` (`:50086`); bring up the envoy bridge (`grpc-web-up`,
`:8090/:8091/:8093`); serve the web bundle (`portal-web`, `:8092`). The
[preflight](01-architecture.md#backend-preflight--down-vs-broken) tags any seam that
is nonetheless down so its page is **skipped, not failed**.

## The two live assertions per mutating action

For each curated action, before/after the GUI interaction:

**1. Correct RPC fired** — HTTP `GET :9700/metrics` and assert the delta on

```
agent_grpc_server_rpc_total{rpc="/agent.v1.<Service>/<Method>",outcome="ok"} ≥ 1
```

The counter is bumped **synchronously** in the tower metrics layer
(`crates/agent-grpc/src/server/metrics_layer.rs:111-122`;
`crates/agent-metrics/src/lib.rs:956-971,2657`) before the response returns — sub-second,
no extra infra, and `outcome="ok"` simultaneously proves it succeeded. Same parsing as
`agent-testkit`'s `MetricsProbe` (`crates/agent-testkit/src/observe.rs:45-66`), but over
HTTP against the live server (the in-process probe can't see a separate process).

**2. Intended state changed** — call the matching **read RPC** on `:50100` (server
reflection is ON, `crates/agent-grpc/src/server/mod.rs:205-217`, so `grpcurl` needs no
protos) and assert the new value:

| action | read RPC | assert |
|---|---|---|
| Prompts set personality | `PromptService/GetActivePersonality` | id now the chosen one |
| Settings save | `ConfigService/GetValues` | edited key now present (pending/after restart per `Status`) |
| Graph set active | `GraphService/Get` | active graph matches the pushed doc |
| Router put / enable | `ProviderRegistry/Get` / `List` | upstream present / enabled flag flipped |
| Fleet update / approve | `ReviewFleet/GetReview` | body updated / status posted |

## Curated OTLP-span assertion (the tracing path itself)

For a **curated few** actions, also assert the server span landed in ClickHouse — the
`grpc.server` span (`crates/agent-grpc/src/server/mod.rs:131-145`), child spans like
`prompt.set_active_personality`, exported OTLP→ClickStack→ClickHouse
(`crates/agent-telemetry/src/otel.rs:74`). Because export is **batched**, this is a
short poll, so it's a curated check (not the fast gate for every action) — it proves the
end-to-end tracing pipe works, complementing the synchronous metrics delta. The harness
records the `trace_id` for each action (see [`07`](07-envoy-otel.md)) so the ClickHouse
query is a direct lookup, and the same id lands in the [perf table](06-performance.md).

## Latency captured here

Layer B is where real timings matter — client-perceived round-trip, plus the
`x-envoy-upstream-service-time` header (backend ms) so proxy+network overhead is
`client − upstream`, plus the server-side `agent_grpc_server_rpc_seconds` histogram. All
stream into `portal_gui_perf` — see [`06-performance.md`](06-performance.md).

## The `portal-e2e` app

A `pkgs.writeShellApplication` folded into `apps` via `mkApps`
(`nix/default.nix`), honoring `CONTAINER_RUNTIME` (podman on l2). It brings up
the stack, runs the `integration_test` suite over the mutating subset, performs the
metrics + read-RPC (+ curated span) assertions, emits the [report](05-report.md) +
[perf rows](06-performance.md), and tears down what it started (never the operator's
own `--serve-fleet` — track a pidfile like `portal-redeploy` does). Opt-in; not on
`nix flake check`.

### As built (inc 07)

- **Curated mutating subset shipped:** Router **Put** + **Enable**
  (`ProviderRegistryService`, fully self-contained — a unique per-run upstream id,
  cleaned up afterwards) and Prompts **SetActivePersonality**. Each action whose
  page/backend/preconditions are absent is recorded `skip`, never `fail` (the
  backend-preflight philosophy). More mutating actions (Graph `Put`, Settings
  `Put`, Fleet `UpdateReview`/`Approve`) slot into the same table.
- **The Dart suite is the driver only; the shell owns the assertions.** The Layer-A
  robots are coupled to the in-process fake gateway (its recording log), so Layer B
  reuses the **widget keys** (not the robots): it navigates by nav-rail label text,
  drives the keyed controls with deterministic values, and hands each action's
  record back through `IntegrationTestWidgetsFlutterBinding.reportData`. The shell
  reads that (the driver writes `portal/build/integration_response_data.json`; a
  browser `print` does **not** surface on `flutter drive` web stdout) and does every
  observability assertion — metrics delta, read-RPC, span — with `grpcurl`/`curl`,
  exactly as `serve-smoke` does.
- **Driver:** `flutter drive -d web-server --browser-name=chrome` serves and drives
  its **own** instance of the web build; what matters is the app's grpc-web calls go
  to the Envoy bridge (via the `--dart-define` endpoints), so the path is the
  browser's own. Headless chromium + a version-matched chromedriver are resolved at
  run time from the ambient `nixpkgs` registry (the flake-pinned `chromium` has no
  cached binary and would source-build; `portal-web`/`grpc-web-up` already fetch the
  web SDK / envoy image at runtime), overridable via `PORTAL_E2E_CHROMIUM` /
  `PORTAL_E2E_CHROMEDRIVER`.
- **Curated span check is best-effort:** it confirms a recent `agent-gateway`
  `grpc.server` span reached ClickHouse (via `clickhouse-client` in the ClickStack
  container), but a down/telemetry-disabled obs stack is a WARN, not a contract
  failure — [inc 10](07-envoy-otel.md) is the authoritative cross-hop trace proof.
- **Exit-code contract** is the shared `nix/lib/contract.sh` 0/1/2 (0 ok, 1 harness,
  2 contract). Untrusted `/metrics` text is parsed fail-closed (only a run of digits
  counts; anything else → 0, so a hostile counter can never fake a positive delta).
