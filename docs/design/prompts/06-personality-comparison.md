# 06 — Personality comparison: the peer system prompts, and the agent-seddon blend

> **Round 3.** Rounds 1–2 gave prompts a home and situational selection
> ([`README.md`](README.md)). Round 3 adds **personalities** — the ability to run
> agent-seddon *as* one of the peer harnesses by adopting its system-prompt text,
> plus a native **agent-seddon best-of-breed** blend. This doc is the analysis; the
> architecture is [`07-personalities.md`](07-personalities.md).

The README's ["how three peer agents organise prompts"](README.md#compare-and-contrast-how-three-peer-agents-organise-prompts)
table compares *structure* (files vs constants vs literals) across hermes/pi/opencode.
This doc goes one level deeper and one peer wider: it reads the **actual prompt text**
of all **four** peers (adding **codex**), names each one's strengths and weaknesses,
and synthesises the elements worth adopting into our own default.

Source paths for every prompt cited here are in
[`../../reference/peer-harnesses.md`](../../reference/peer-harnesses.md).

## Per-harness reading

### pi — programmatic, tool-shaped, minimal
- **Shape:** one `buildSystemPrompt()` body (`core/system-prompt.ts`) assembled from
  a short identity + a guidelines section + a *generated* tool/docs section; overridable
  via `customPrompt` / `appendSystemPrompt` / `promptGuidelines`.
- **Strengths:** tight and low-ceremony; the tool section is derived from the enabled
  toolset so the prompt never describes a tool that isn't present. Easy to reason about.
- **Weaknesses:** thin on safety/planning/persistence guidance; the identity is generic;
  little model-specific tuning. It leans on the model's own competence.
- **Distinctive technique:** *prompt = f(tools)* — the instruction set tracks the
  capability set automatically.

### hermes — tiered, identity-forward, model-aware
- **Shape:** three tiers — **stable / context / volatile** (`system_prompt.py`) — built
  from many `*_GUIDANCE` constants (`prompt_builder.py`): identity, per-provider
  (OpenAI/Google) guidance, memory, skills, tool-use enforcement, platform hints. A
  user `SOUL.md` replaces `DEFAULT_AGENT_IDENTITY`.
- **Strengths:** the tiering maps cleanly onto prompt-cache stability (stable prefix
  cached; volatile tail cheap to invalidate); explicit tool-use *enforcement* language;
  first-class persona override.
- **Weaknesses:** large and sprawling; a lot of it is Nous/platform-specific; the
  breadth risks diluting focus for a small model.
- **Distinctive technique:** **explicit stable/context/volatile tiering** and a
  swappable identity file.

### opencode — per-model variants, replace-the-base
- **Shape:** a *different* base prompt per model family (`session/prompt/anthropic.txt`,
  `gpt.txt`, `gemini.txt`, `kimi.txt`, `beast.txt`, …), selected by model id, then
  env/instructions/mcp/skills appended. Mode prompts (`plan.txt`) are their own files.
- **Strengths:** the cleanest to read and to *source* (plain `.txt`); genuinely tuned
  per model — the Anthropic and GPT prompts differ in tone and structure, not just
  wording; explicit `plan` vs `build` mode text.
- **Weaknesses:** the agent prompt *replaces* the model base, so shared guidance is
  re-stated per variant (maintenance cost, cache-prefix churn on switch).
- **Distinctive technique:** **per-model prompt variants** as first-class files.

### codex — apply-patch discipline, sandbox/approval, the deepest
- **Shape:** a large base (`models-manager/prompt.md`, ~20 KB) with per-model variants
  (`core/*_prompt.md`) and separate template prompts for review/compact/permissions
  (`prompts/templates/**`); a guardian policy prompt (`core/src/guardian/policy.md`).
- **Strengths:** by far the most rigorous on **editing discipline** (apply-patch/V4A
  format rules), **sandbox & approval** semantics, and **persistence** ("keep going
  until the task is truly done"); strong output-formatting and planning sections.
- **Weaknesses:** heavily GPT-5-tuned (its persistence/verbosity calibration may not
  transfer to Kimi/Qwen); long enough to crowd a small context window.
- **Distinctive technique:** **explicit apply-patch + sandbox/approval + persistence**
  sections, and a dedicated review rubric.

## Comparison table (text/persona axis — extends the README's org table with codex)

| Dimension | pi | hermes | opencode | codex | agent-seddon (blend) |
|---|---|---|---|---|---|
| Identity / persona | generic, inline | `DEFAULT_AGENT_IDENTITY` / `SOUL.md` | per-model tone | task-focused, terse | concise, seam-honest identity |
| Tool-use guidance | generated from toolset | explicit enforcement | per-variant | rigorous (apply-patch/V4A) | **adopt codex** editing rigor + **pi** tool-tracks-toolset |
| Safety / sandbox | thin | platform hints | some | **strong** (sandbox/approval) | **adopt codex**, mapped to our `Policy` seam |
| Planning | thin | present | mode-gated (`plan`) | strong | **adopt codex/opencode**, mode-tagged |
| Persistence ("finish the job") | thin | present | present | **strong** | **adopt codex** |
| Output formatting | light | present | per-variant | strong | **adopt codex**, trimmed |
| Cache/tiering | flat | **stable/context/volatile** | replace-per-model | flat base + templates | **adopt hermes** tiering (maps to our stable-prefix) |
| Model-specific tuning | none | provider guidance | **per-model files** | per-model files | **adopt opencode** — a variant per generator (Kimi/Qwen) later |
| Base↔variant relation | append | additive | **replace** | base + templates | base **selected** by personality; mode fragments **append** (our additive rule) |

## Synthesis → the agent-seddon best-of-breed base

The native `agent-seddon` personality is authored (in Phase 1,
[`07-personalities.md`](07-personalities.md)) as a **blend**, taking:

- **codex** — the editing-discipline, sandbox/approval, persistence, and
  output-formatting sections (the areas where it is clearly the strongest), rewritten to
  reference **our** tools and the [`Policy`](../../../CLAUDE.md) seam rather than codex's
  sandbox model.
- **hermes** — the **stable / context / volatile** tiering, which already matches our
  prompt-cache stable-prefix strategy ([`02-composition.md`](02-composition.md)) and the
  base-vs-situational split.
- **pi** — *prompt-tracks-toolset*: keep the tool guidance honest to the enabled tools,
  and keep the whole thing lean.
- **opencode** — the **per-model-variant** idea, deferred: the blend is written once now,
  with per-generator variants (Kimi/Qwen/GLM) as a later refinement once we can measure
  which wins.

**Honest caveats.** These prompts are *model-tuned*: codex's persistence/verbosity
calibration is for GPT-5, and a prompt that lifts one model can depress another. This
doc is **descriptive analysis**, not a benchmark. The blend is therefore shipped as a
*selectable* personality first; promoting it to the default happens only after a live
A/B on our own generators (Kimi/Qwen) — the same "measure before you commit" discipline
the [review-parallelism](../review-parallelism/STATUS.md) track used. Until then the
default personality is **byte-identical to today's** prompt
([`07-personalities.md`](07-personalities.md)).

## What this doc is not

It does not adopt any peer's *code* or license-bound assets — only a reading of their
publicly-visible prompt text, and a synthesis authored in our own words. The mechanism
for importing peer prompts verbatim as *selectable* personalities (with provenance) is
[`08-versioning-and-provenance.md`](08-versioning-and-provenance.md) +
[`09-nixpkgs-sourcing.md`](09-nixpkgs-sourcing.md); the licensing note lives there.
