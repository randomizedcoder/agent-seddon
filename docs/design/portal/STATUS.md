# Agent Portal — implementation status

The living tracker for the [Agent Portal](README.md) design. One gated PR per
increment. Backend seams (02–04) are independent and can land in any order; the
Flutter app (06) depends on the Dart codegen (05), which depends on the protos
existing (02–04).

## Increments

| # | Increment | Proto | Seam | Wire | Tests | Nix | Status |
|---|---|:--:|:--:|:--:|:--:|:--:|:--:|
| 01 | Design docs (`docs/design/portal/`) | — | — | — | — | — | **this PR** |
| 02 | `PromptService` + mode-lens externalization | ☐ | ☐ | ☐ | ☐ | ☐ | pending |
| 03 | `MetricsProxyService` | ☐ | ☐ | ☐ | ☐ | ☐ | pending |
| 04 | `AgentSessionService` + broadcast event-sink | ☐ | ☐ | ☐ | ☐ | ☐ | pending |
| 05 | Dart codegen + nix tooling (`buf.gen.yaml`, `gen-dart`, flutter/dart pins, proxy) | — | — | — | ☐ | ☐ | pending |
| 06 | Flutter app (transport, Launcher, Prompts, Agent View) | — | — | — | ☐ | ☐ | pending |

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
