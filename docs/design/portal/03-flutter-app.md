# 03 — The Flutter app (`portal/`)

A new top-level `portal/` Flutter project. This part covers the reusable core (the
transport abstraction) and the UI surfaces (left-nav → Launcher, Prompts, Agent
View).

```
portal/
├── pubspec.yaml
└── lib/
    ├── main.dart                       # app shell + left nav
    └── src/
        ├── gen/                        # committed generated stubs (see 02)
        ├── transport/channel_factory.dart
        ├── config.dart                 # endpoints (gateway, proxy, external UIs)
        ├── clients/                    # thin wrappers over generated stubs
        └── pages/
            ├── launcher_page.dart
            ├── prompts_page.dart
            └── agent_view_page.dart
```

## The transport abstraction (the reusable core)

Everything else depends on one factory, so native and web share **all** UI and
client code. The only difference between the two builds is which channel this returns:

```dart
// lib/src/transport/channel_factory.dart
ClientChannelBase gatewayChannel(PortalConfig cfg) {
  if (kIsWeb) {
    // browsers can't speak raw gRPC (HTTP/2 trailers) → grpc-web via the proxy
    return GrpcWebClientChannel.xhr(Uri.parse(cfg.grpcWebProxyUrl)); // :8090
  }
  // native desktop dials the --serve-all gateway directly, no proxy
  return ClientChannel(cfg.gatewayHost,
      port: cfg.gatewayPort,                                          // :50100
      options: const ChannelOptions(credentials: ChannelCredentials.insecure()));
}
```

Every service client (`PromptServiceClient`, `AgentSessionServiceClient`,
`MetricsProxyServiceClient`, `LlmPoolServiceClient`) is constructed from this one
channel. Add a service to the gateway → it's reachable with no transport work.

`PortalConfig` holds the gateway/proxy endpoints and the three external-UI URLs
(Grafana / HyperDX / Prometheus), so the app is portable across hosts — defaults
match `nix/constants.nix`, overridable at runtime.

## Left nav → three destinations

### 1 · Launcher

Cards that open the existing UIs as ordinary browser tabs — the portal never scrapes
their HTML, it just points at them:

| Card | URL |
|---|---|
| Grafana dashboard | `http://localhost:3000` (dashboard slug `agent-seddon`) |
| HyperDX traces | `http://localhost:8080` |
| Prometheus | `http://localhost:9090` |

Native: `url_launcher`. Web: `window.open`. Plus in-app links into Prompts and Agent
View. URLs come from `PortalConfig`.

### 2 · Prompts — CRUD editor over `PromptService`

```
┌───────────────┬───────────────────────────────┬────────────────────┐
│ kind ▸ entries│ editor (selected entry)       │ Preview assembled  │
│ System        │ ┌───────────────────────────┐ │ mode: [Debug ▾]    │
│ Prepend       │ │ # 0001_persona.md         │ │ ── system ─────────│
│  0001_persona │ │ You are …                 │ │ …system+prepend…   │
│  0020_rules   │ │                           │ │ ── user ───────────│
│ Append        │ │                           │ │ <goal>             │
│ Mode-lens     │ └───────────────────────────┘ │ ── system(append) ─│
│  implement 🔒 │ [order: 0001] [Save] [Delete] │ …append…           │
│  debug 🔒     │                               │                    │
└───────────────┴───────────────────────────────┴────────────────────┘
```

- Left: entries from `PromptService.List`, grouped by `PromptKind` (System · Prepend ·
  Append · Mode-lens ×6).
- Center: a text editor for the selected entry. A **🔒 read-only badge** marks a
  built-in mode-lens default (and the system prompt) until the operator forks it;
  `order` / `id` fields show for context.d files. `Save` → `Put`, `Delete` → `Delete`
  (refused server-side for read-only kinds).
- Right: **Preview assembled** calls `PreviewAssembled(mode, goal)` and renders the
  exact `[system, user, system-append]` the model would see for the chosen `TaskMode`
  — the literal answer to *"what prompt drives Debug mode?"*

### 3 · Agent View

The panel layout from the brief — thin nav, one large live panel, a full-width status
bar:

```
┌───┬──────────────────────────────────────────────────────────┐
│ n │  main panel: live loop narration (AgentSessionService)    │
│ a │   • assistant token stream, rendered incrementally        │
│ v │   • tool-call cards (name, args, result / latency)        │
│   │   • mode-switch + compaction markers inline               │
├───┴──────────────────────────────────────────────────────────┤
│  [mode: Implement]  [ctx 12.4k / 32k · 48 msg]                │
│  [pool 3/4 · ▓▓░ mi50 busy]  [gRPC p50 210ms · p99 1.8s]      │
└───────────────────────────────────────────────────────────────┘
```

**Main panel** — one `AgentSessionService.Subscribe` stream, rendering the
`SessionEvent` oneof: `TokenDelta` appends to the current assistant bubble;
`ToolCallStart`/`ToolCallResult` render a tool card (name, args, outcome, duration);
`ModeSwitch` and compaction markers drop inline dividers; `RunStarted`/`RunFinished`
bracket a run. A late subscriber gets the `StatusSnapshot` first, so the panel is
consistent on connect.

**Status bar cells** — each an independent widget with its own source:

| Cell | Source | Cadence |
|---|---|---|
| mode | same `Subscribe` stream (`ModeSwitch` / initial `StatusSnapshot`) | push |
| context | same stream (`ContextUpdate`) — `tokens / window · messages` | push |
| GPU pool | `LlmPoolService.Health` → `PoolMemberHealth[]` (alive / state / in_flight / latency) | poll ~3s |
| p50/p99 | `MetricsProxyService.Query` with canned `histogram_quantile` PromQL | poll ~5s |

The pool cell renders one chip per member (a GPU), coloured by
`PoolMemberState` (healthy / degraded / dead) with an in-flight/saturation bar —
"GPUs in the active pool" directly off the authoritative health report, no scraping.

## State & error handling

- Each stream/poll is a small state holder (e.g. a `ChangeNotifier` / `StreamBuilder`);
  no cross-page shared mutable state beyond `PortalConfig`.
- **Fail soft, visibly.** A dropped stream shows a reconnect affordance; a failed
  `MetricsProxy`/pool poll greys its cell rather than blanking the bar — the loop and
  the other cells are unaffected (mirrors the backend's drop-not-block posture).
- Reconnect re-`Subscribe`s and re-reads `Snapshot`, so the panel resynchronises
  without a full reload.

## Why native *and* web earns its keep

The transport factory is ~15 lines; behind it, one widget tree serves a desktop app
(single binary feel, raw gRPC to `:50100`, no proxy) *and* a browser tab that sits
alongside Grafana/HyperDX (grpc-web via the [optional proxy](04-nix-tooling.md)). The
brief wants both a "portal you click to open browser tabs" and a rich live view — this
covers each without a second UI codebase.
