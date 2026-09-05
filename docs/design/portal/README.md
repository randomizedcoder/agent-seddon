# Design: Agent Portal (Flutter · gRPC-only)

Status: **largely implemented** — the six-increment core (Launcher · Prompts ·
Agent View + the three seams) has merged, and a **configuration & router** increment
([`05-config-and-router.md`](05-config-and-router.md)) is implemented on a branch;
[`STATUS.md`](STATUS.md) is the authoritative tracker. This directory is the design
of record for a small Flutter "Agent Portal" that makes the harness's existing
legibility *reachable from one place*, and closes the gaps in it — the same
convention as [`../adaptive-cognition/`](../adaptive-cognition/README.md) and
[`../code-review/`](../code-review/README.md).

## The idea

`agent-seddon`'s thesis is **legibility**: every capability is a
[seam](../../architecture.md), separately instrumented, and dial-able over gRPC.
Today that legibility is real but *scattered* — the operator reads it through a
terminal REPL and browser tabs they open by hand: Grafana (`:3000`), HyperDX
(`:8080`), Prometheus (`:9090`). And two things are not legible at all:

> **The prompts.** What is actually folded into each turn's context — the
> pre/post-pends ([`context.d/prepend|append/*.md`](../../../context.d/README.md)),
> the operator [system prompt](../../operating.md#configuration), and the **per-mode
> compaction lens instructions** (compiled constants in
> [`agent-context/src/mode_aware.rs`](../../../crates/agent-context/src/mode_aware.rs)) —
> is invisible without reading source. You cannot see *"what prompt drives Debug mode
> vs Implement mode."*
>
> **What the running agent is doing**, as a live structured feed. Nothing streams the
> whole loop remotely — only per-token *model* output via `ProviderService.Stream`,
> and per-turn events landing in `.agent/episodic.jsonl`.

The **Agent Portal** is a small Flutter app that (a) **launches** the observability
UIs, (b) lets the operator **see and CRUD every prompt, per mode**, and (c) gives a
live **agent view** — the loop narration in a main panel plus a status bar (mode ·
context size · GPU pool · gRPC p50/p99). It talks **gRPC only**, over the
[`--serve-all` gateway](../../grpc.md), so the portal is *itself* another legible
gRPC client — consistent with "seams as services," and the same wire contract the
rest of the harness already governs with `buf`.

## Scope decisions

Four forks were settled up front; the rest of this directory elaborates them.

| Decision | Choice | Why |
|---|---|---|
| Flutter target | **Both** — abstract the transport | Native desktop dials raw gRPC to `:50100`; the web build goes through a grpc-web proxy. One UI codebase, one channel factory. |
| Live agent view | **New `AgentSessionService`** | A *structured* server-stream of loop events — not raw PTY bytes, not a coarse log-tail. Drives the main panel and the status bar's mode/context together. |
| Prompt CRUD | **Full CRUD + externalize mode prompts** | The per-mode lens instructions move from compiled `&'static str` into editable data files (compiled defaults retained), so *every* prompt is viewable and editable. |
| Telemetry access | **New `MetricsProxyService`** | A generic PromQL-over-gRPC proxy keeps the client strictly gRPC while reusing Prometheus (and any Grafana panel query verbatim). |

## Architecture

```
        ┌──────────────────────── Flutter "Agent Portal" ────────────────────────┐
        │  Left nav │  ┌─ Launcher ─┐ ┌─ Prompts (CRUD) ─┐ ┌───── Agent View ─────┐│
        │           │  │ open tabs: │ │ system / prepend │ │  main panel: live    ││
        │           │  │ Grafana    │ │ append / mode-   │ │  loop narration      ││
        │           │  │ HyperDX    │ │ lens · preview   │ │  (tokens, tool cards)││
        │           │  │ Prometheus │ └──────────────────┘ │ ─────────────────────││
        │           │  └────────────┘                      │ status bar: mode ·   ││
        │           │                                      │ ctx · GPUs · p50/p99 ││
        └───────────┴──────────────────────────────────────┴──────────────────────┘
                    │ transport abstraction (ChannelFactory)
         native ────┤ raw gRPC  ─────────────────────────────────┐
         web ───────┤ grpc-web ─▶ proxy (:8090) ─▶ gateway ───────┤
                    └───────────────────────────────────────────▶ agent --serve-all (:50100)
                                                                  ├─ PromptService        (NEW · A1)
                                                                  ├─ AgentSessionService  (NEW · A2)
                                                                  ├─ MetricsProxyService  (NEW · A3) ─▶ Prometheus HTTP :9090
                                                                  ├─ LlmPoolService.Health (exists) — GPU pool
                                                                  └─ …23 existing seams
   external browser tabs (url_launcher / window.open): Grafana :3000 · HyperDX :8080 · Prometheus :9090
```

Everything the portal *reads or writes* rides the **`--serve-all` gateway on
`:50100`** as a single endpoint — the portal dials one address and every service is
there (`--serve-all` folds every enabled seam onto one router, see
[`grpc.md`](../../grpc.md#one-process-every-seam----serve-all)). The three external
UIs are opened as ordinary browser tabs; the portal never scrapes their HTML.

**Where each piece of the status bar comes from:**

| Status cell | Source | Notes |
|---|---|---|
| Agent **mode** | `AgentSessionService` (`ModeSwitch` / `StatusSnapshot`) | Closes the "no current-mode getter" gap — live and authoritative, not derived from switch counters. |
| **Context** size | `AgentSessionService` (`ContextUpdate`) | `prompt_tokens / context_window · messages`; also available historically via A3. |
| **GPU pool** | existing `LlmPoolService.Health` | `PoolHealthReport` / `PoolMemberHealth` already carry alive/state/in_flight/latency per member — one GPU per member. |
| gRPC **p50/p99** | `MetricsProxyService.Query` | Canned `histogram_quantile(…)` PromQL over `agent_provider_request_seconds_bucket`. |

## The three new seams (Part A)

Each is a standard seam-to-wire addition (see
[`01-backend-seams.md`](01-backend-seams.md)), following the mechanical recipe in
[`grpc.md`](../../grpc.md#adding-a-seam-to-the-wire). All three are additive protos
— they pass `buf breaking` untouched (no `buf.image.binpb` bump) — and get gRPC
reflection + `grpc.health.v1` for free, so they're `grpcurl`-introspectable the
moment they're served.

- **`PromptService`** — see + CRUD every prompt, unified across its three storage
  homes, plus a per-mode "preview assembled context." Requires externalizing the
  compiled lens prompts into editable data (defaults retained).
- **`AgentSessionService`** — a structured, server-streaming feed of the live loop
  (token deltas, tool-call start/result, mode switches, context updates) plus a
  one-shot `Snapshot`. Fed by a lightweight broadcast event-sink at the runtime's
  *existing* emission sites — no new control flow in the loop.
- **`MetricsProxyService`** — a generic PromQL-over-gRPC proxy so the client stays
  gRPC-pure and any Grafana query is reusable verbatim.

## The other three parts

- [`02-dart-codegen.md`](02-dart-codegen.md) — Dart/gRPC codegen via a new
  `buf.gen.yaml` + `protoc-gen-dart` and a `nix run .#gen-dart` app. buf today only
  lints/breaking-checks; this is the first *generation* it drives (Rust codegen stays
  on `tonic-build`, untouched).
- [`03-flutter-app.md`](03-flutter-app.md) — the app itself: the transport
  abstraction (the reusable core), the left-nav's three destinations, and the
  agent-view panel layout.
- [`04-nix-tooling.md`](04-nix-tooling.md) — flutter/dart pins, the optional
  web-only grpc-web proxy, run apps, and the new port block in `nix/constants.nix`.

## Configuration & the model router (increment 05)

- [`05-config-and-router.md`](05-config-and-router.md) — two more tabs that close the
  **configuration** legibility gap and surface the one live control plane that had no
  UI. **Settings**: a schema-driven editor over the *entire* `config/agent.toml` via a
  new **`ConfigService`** seam (`GetSchema`/`GetValues`/`Validate`/`Put`/`Status`) —
  `schemars` schema + enum/secret overlay, `toml_edit` comment-preserving write-back,
  effect on **restart** (a drift banner says so). **Router**: live CRUD over the
  existing [`ProviderRegistryService`](../model-router/03-registry-proto.md) — upstream
  cards, routing policy, health, and a route tester — which **applies immediately**.
  The two opposite apply-models are the doc's organizing idea. Adds a reusable
  `SchemaForm` widget (generalises the graph page's `_ParamsEditor`).

## Security posture (inherited, not invented)

The harness's rule holds: **operator and model input is untrusted, fail closed.**
The new surfaces respect it — prompt ids that become filenames pass `safe_segment` +
`confine()` (block traversal / symlink escape); the PromQL string is length- and
result-capped and the proxy only ever *reads* Prometheus; the session stream is
observe-only in increment 1, and a future `Send` (drive a goal remotely) carries the
`--serve-mcp`-class caveat — loopback/UDS only, socket permissions are the access
control. Details live in [`01-backend-seams.md`](01-backend-seams.md).

## Non-goals

- **No new deployment story.** Like every seam today, this runs over loopback / UDS;
  container images and multi-host orchestration remain out of scope (see the README's
  "Running seams as services").
- **Not a replacement for Grafana/HyperDX.** The portal *launches* them and mirrors a
  few live numbers into its status bar; deep time-series analysis stays in the tools
  built for it.
- **No auth layer.** Transport security is unchanged — socket file permissions on
  UDS, loopback on TCP — exactly as the existing seams document.
