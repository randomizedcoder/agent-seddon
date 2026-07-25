# Agent Portal — implementation status

The living tracker for the [Agent Portal](README.md) design. One gated PR per
increment. Backend seams (02–04) are independent and can land in any order; the
Flutter app (06) depends on the Dart codegen (05), which depends on the protos
existing (02–04).

## Increments

| # | Increment | Proto | Seam | Wire | Tests | Nix | Status |
|---|---|:--:|:--:|:--:|:--:|:--:|:--:|
| 01 | Design docs (`docs/design/portal/`) | — | — | — | — | — | **merged** (#127) |
| 02 | `PromptService` + mode-lens externalization | ✅ | ✅ | ✅ | ✅ | n/a | **in review** |
| 03 | `MetricsProxyService` | ☐ | ☐ | ☐ | ☐ | ☐ | pending |
| 04 | `AgentSessionService` + broadcast event-sink | ☐ | ☐ | ☐ | ☐ | ☐ | pending |
| 05 | Dart codegen + nix tooling (`buf.gen.yaml`, `gen-dart`, flutter/dart pins, proxy) | — | — | — | ☐ | ☐ | pending |
| 06 | Flutter app (transport, Launcher, Prompts, Agent View) | — | — | — | ☐ | ☐ | pending |

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
