# Personalities (example seed) — run agent-seddon *as* a peer harness

**Round 3, Phase 1.** A **personality** is a named base system prompt agent-seddon can
run as, selected with `[agent] personality` (one of the closed set `agent-seddon | pi |
hermes | opencode | codex`). See
[`../../docs/design/prompts/07-personalities.md`](../../docs/design/prompts/07-personalities.md).

This directory holds **inert example seeds** — one per personality. They are **not
active**: the resolver reads the *live* slot `prompts/personalities/<p>/` (never this
`.example/` tree), and crane filters `prompts/**.md` out of the build, so a default
build is byte-identical to having no personalities at all.

## How to activate one

Copy a personality's fragment(s) into the live slot, then select it in config:

```sh
mkdir -p prompts/personalities/codex
cp prompts/personalities.example/codex/0001_codex.md prompts/personalities/codex/
# then set, in config/agent.toml:
#   [agent]
#   personality = "codex"
```

Copy only the `NNNN_*.md` prompt fragment(s) — **not** `ATTRIBUTION.md` (it is a notice
file, not prompt text). Multiple `NNNN_*.md` files in a live personality dir concatenate
in numeric order.

## The base-resolution ladder

For an active `personality = "<p>"`, the base is the first of:

1. `prompts/personalities/<p>/*.md` (numeric-ordered concat)
2. `prompts/personalities/<p>.md` (single file)
3. `prompts/system.md` (the shared base — unchanged)
4. `[agent] system_prompt` (the config default)

An **empty or unknown** personality skips rungs 1–2 entirely → today's base
(rungs 3–4), byte-identical. Until you copy a seed into the live slot, selecting even
`personality = "agent-seddon"` yields today's base — the seeds are opt-in.

## What's here

- `agent-seddon/` — our **best-of-breed** base, synthesized (Kimi-assisted) from the
  four peers per [`../../docs/design/prompts/06-personality-comparison.md`](../../docs/design/prompts/06-personality-comparison.md).
  Ours; no third-party license.
- `pi/`, `hermes/`, `opencode/`, `codex/` — the peer prompts, redistributed verbatim
  under their (permissive) upstream licenses. Each carries an `ATTRIBUTION.md`; the
  notices are collected in [`LICENSES.md`](LICENSES.md).

> These are model-tuned prompts from other harnesses; one that lifts a given model can
> depress another. Treat a peer personality as a starting point to evaluate, not a
> guaranteed improvement — see doc 06's caveats.
