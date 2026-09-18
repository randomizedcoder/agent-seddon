# Reference: the peer coding-harness clones (and nixpkgs) we consult

An orientation map of the read-only source trees we keep on hand for parity work,
prompt archaeology, and design comparison. It answers two questions a newcomer (or a
future agent) keeps re-discovering: *what is each of these clones, and where do its
prompts live* — and *how do we reach the same source reproducibly through nixpkgs*.

This is deliberately a **reference/orientation** doc, not a spec. It complements the
test-focused per-feature specs in [`../parity/README.md`](../parity/README.md) and the
prompt-adoption design in
[`../design/prompts/06-personality-comparison.md`](../design/prompts/06-personality-comparison.md).

> **Snapshot, not a pin.** The `~/Downloads/*` paths below are working clones on the
> dev box and drift. For a *reproducible* copy of a peer's source — the one the
> [prompt-refresh](../design/prompts/09-nixpkgs-sourcing.md) pipeline reads — use the
> nixpkgs `<pkg>.src` route in the last section, not these clones.

## The four peers

The parity set. Each solves "base prompt + situational variation" differently; the
[personality-comparison](../design/prompts/06-personality-comparison.md) doc mines the
actual prompt *text*, while this table is the "where is everything" map.

### pi — TypeScript, disciplined minimalism
- **Local clone:** `~/Downloads/pi` · upstream `github:earendil-works/pi` (monorepo `badlogic/pi-mono`).
- **What it is:** a lean TS coding agent; prompt is assembled programmatically and
  shaped by which tools are enabled, rather than authored as a static document.
- **Prompts:** code, not data files. The default system prompt is an inline template
  literal built by `buildSystemPrompt()` in
  `packages/coding-agent/src/core/system-prompt.ts` (~line 121); the skills XML block
  is `packages/agent/src/harness/system-prompt.ts`. A separate user-facing
  "prompt templates" feature lives in `packages/*/src/**/prompt-templates.ts`
  (docs: `packages/coding-agent/docs/prompt-templates.md`). Repo instructions in
  `AGENTS.md`. No persona/mode-variant prompt files.
- **nixpkgs:** `pi-coding-agent` (`pkgs/by-name/pi/pi-coding-agent/package.nix`,
  `buildNpmPackage` from source). Prompt text is in `.src` but as TS literals (no
  clean extractable file); `$out` is transpiled JS.

### hermes-agent — Python, batteries-included
- **Local clone:** `~/Downloads/hermes-agent` · upstream `github:NousResearch/hermes-agent`.
- **What it is:** a large, feature-rich Python agent (Nous Research) with profiles,
  skills, routines, and multi-model guidance baked into a tiered prompt assembly.
- **Prompts:** Python string constants. Three-tier assembly (stable / context /
  volatile) in `agent/system_prompt.py`; the text constants (`DEFAULT_AGENT_IDENTITY`
  + `*_GUIDANCE` blocks) are in `agent/prompt_builder.py`. A user-supplied `SOUL.md`
  (loaded from `HERMES_HOME`) overrides the identity. Assembly docs:
  `website/docs/developer-guide/prompt-assembly.md`. Repo instructions in `AGENTS.md`.
- **nixpkgs:** **not packaged.** No `hermes`/`NousResearch` attr anywhere under
  `nixpkgs/pkgs`. Reproducible sourcing needs its own locked input (see below).

### opencode — TypeScript/bun, polished fundamentals
- **Local clone:** `~/Downloads/opencode` · upstream `github:anomalyco/opencode`.
- **What it is:** a fundamentals-first daily-driver agent with first-class
  agents/modes and per-model prompt variants — the cleanest prompts to source.
- **Prompts:** standalone `.txt` files. `packages/opencode/src/session/prompt/` holds
  ~15 per-model/mode variants (`anthropic.txt`, `beast.txt`, `codex.txt`,
  `gpt.txt`, `gemini.txt`, `kimi.txt`, `default.txt`, plus `plan.txt`/`plan-mode.txt`/
  reminders), selected by model id in `packages/opencode/src/session/system.ts`.
  Agent-specific prompts under `packages/opencode/src/agent/prompt/`; project
  instructions in per-package `AGENTS.md`.
- **nixpkgs:** `opencode` (`pkgs/by-name/op/opencode/package.nix`, bun build from
  source). The raw `.txt` prompts ship verbatim in `.src` — the ideal case.

### codex — Rust (`codex-rs`), the deepest peer
- **Local clone:** `~/Downloads/codex` · upstream `github:openai/codex`.
  ⚠️ This clone's *root* also contains stray `hermes_*.py` files; the real codex
  sources are under `codex-rs/`.
- **What it is:** OpenAI's large (~130-crate) Rust agent, added as a fourth peer for
  its depth (apply-patch discipline, sandbox/approval policy, per-model prompts).
- **Prompts:** `.md` files embedded via `include_str!`. Live base:
  `codex-rs/models-manager/prompt.md` (embedded in `models-manager/src/model_info.rs`).
  Per-model variants `codex-rs/core/*_prompt.md` (gpt-5.1/5.2/codex/apply-patch).
  Template prompts (review/compact/permissions) in `codex-rs/prompts/templates/**`.
  Guardian policy `codex-rs/core/src/guardian/policy.md`.
- **nixpkgs:** `codex` (`pkgs/by-name/co/codex/package.nix`, `buildRustPackage`,
  `sourceRoot=codex-rs`). The `.md` prompts are in `.src`; `$out` embeds them into the
  binary. nixpkgs pins a specific version whose prompt file set can differ from the
  local clone.

## nixpkgs — always available as a source of record

- **Local clone:** `~/Downloads/nixpkgs` (large; `master` tracked).
- **agent-seddon pins nixpkgs** as the flake input `nixpkgs.url =
  "github:NixOS/nixpkgs/nixos-unstable"` ([`flake.nix`](../../flake.nix)); everything
  is built against that locked revision (`flake.lock`). The same lock is what
  [`NixRunToolProvider`](../../crates/agent-runtime/src/tool_provider.rs) bakes onto
  the agent wrapper as `AGENT_NIXPKGS_FLAKEREF` — the reproducibility anchor.
- **Consulting a package's source** (the reproducible alternative to the `~/Downloads`
  clones): `nix build nixpkgs#<pkg>.src` gives the content-addressed source tarball a
  package builds from — e.g. `nix build nixpkgs#opencode.src` yields the tree
  containing `packages/opencode/src/session/prompt/*.txt`. This is how the
  [prompt-refresh pipeline](../design/prompts/09-nixpkgs-sourcing.md) reads peer
  prompts: **from `.src`, never `$out`** (codex `include_str!`-embeds and opencode/pi
  bundle their prompts into build output, so only `.src` has the raw files).
- **hermes is the exception** — not in nixpkgs — so it is sourced via a dedicated
  locked `flake=false` input (`github:nousresearch/hermes-agent`, pinned by commit +
  nar hash), the same pattern `flake.nix` already uses for `advisory-db` and the
  `xtcp2-*` eval corpus.

## Summary table

| Peer | Local clone | Language | Prompt location | Prompt format | nixpkgs attr | Source via `.src`? |
|---|---|---|---|---|---|---|
| pi | `~/Downloads/pi` | TS | `packages/coding-agent/src/core/system-prompt.ts` | TS template literal | `pi-coding-agent` | yes, but as `.ts` (needs extraction) |
| hermes-agent | `~/Downloads/hermes-agent` | Python | `agent/prompt_builder.py` | Python string constants | — (not packaged) | no — locked `flake=false` input |
| opencode | `~/Downloads/opencode` | TS/bun | `packages/opencode/src/session/prompt/*.txt` | plain `.txt` (per model/mode) | `opencode` | **yes, clean** |
| codex | `~/Downloads/codex` (`codex-rs/`) | Rust | `codex-rs/models-manager/prompt.md` (+ `core/*_prompt.md`) | `.md` via `include_str!` | `codex` | yes (`.src`; version may differ) |
| nixpkgs | `~/Downloads/nixpkgs` | — | (source of record for the above) | — | (the pin itself) | `nix build nixpkgs#<pkg>.src` |
