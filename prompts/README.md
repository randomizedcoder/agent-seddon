# prompts — the prompt library

The root the `PromptStore` seam and the loop's resolvers read (config `[prompts]
dir`, default `prompts/`). It holds operator **overrides** and **situational system
fragments**. This `README.md` is orientation only — it is **never injected** (like
[`context.d/README.md`](../context.d/README.md)); only the files under the slot
directories below are read.

Everything here is **opt-in**: with no files, the agent runs on its compiled
defaults, byte-identical to a fresh checkout. The ready-to-use content ships as
**`.example/` templates** — inert until you copy a file into its live slot.

## Activate an example

```sh
# Turn on Review-mode guidance: copy the example fragments into the live slot.
mkdir -p prompts/modes/review
cp prompts/modes.example/review/*.md prompts/modes/review/
```

A situational fragment is **live on the next context change** (the resolver reads it
per turn — no restart). You can also manage prompts through the seam
(`agent --serve-prompt`, or the portal) instead of editing files; see
[`docs/components/prompt.md`](../docs/components/prompt.md).

## Layout

```
prompts/
├── system.md              # base system prompt override (single file) — else [agent] system_prompt
├── lens/<mode>.md         # per-mode compaction-lens override — else the compiled lens
├── modes/<mode>/NNNN_*.md # situational SYSTEM fragments — appended when mode:<mode> matches
│
├── system.example/        # ← illustrative base, split into concerns (see note below)
└── modes.example/<mode>/  # ← ready-to-copy per-mode fragments (implement/debug/review/design/explain)
```

Resolution for a slot is, in order: the **directory** form `<slot>/…/*.md`, else the
**single-file** form `<slot>.md`, else the **compiled default**. Files are
concatenated in `NNNN_` numeric order (ties by name).

- **Situational fragments** (`modes/<mode>/`) are the new, working feature: a
  `modes/review/` fragment carries the tag `mode:<mode>` and is folded into the
  system prompt as a leading, volatile message whenever that mode is active. `Other`
  intentionally ships **no** fragment — the base alone is right for general work.
- **The base** today is `prompts/system.md` (single file) or the config
  `[agent] system_prompt`. The split multi-fragment form under `system.example/` is
  illustrative of a planned refinement (`system/*.md`); it is **not read yet**, so
  treat those three files as a guide to the base's structure, not a copy-and-go slot.

## Authoring

Fragments are **deltas on the base** — say only what differs for the situation;
don't restate identity, the tool list, or safety rules. Keep them short, imperative,
and testable ("Establish the changed-file set before commenting" beats "be
thorough"). No variables, no model-derived text — fixed human-written markdown.

## Finer tags (frontmatter) — the wider catalog

A fragment can key on **more than the mode** via YAML-ish frontmatter, applying only
when *every* tag is in the situation (`docs/design/prompts/04-selection.md`):

```markdown
---
tags: [mode:review, language:rust]
order: 30
---
For Rust specifically: flag `unwrap`/`expect` on fallible paths, `unsafe` without a
safety comment, lifetimes leaking across an API boundary, and a `Result` silently
dropped.
```

The `PromptStore` seam and the `Select`/`PreviewAssembled` RPCs already honour the
full `tags ⊆ context` model over such tags. **Caveat:** the *loop's* resolver
currently selects on the `mode:` tag alone — a signal source for `language:`,
`tier:`, etc. (and a small resolver upgrade) is still to land, so a frontmatter-tagged
fragment copied into `modes/<mode>/` would today be selected on its mode alone. Prefer
the plain per-mode fragments until then.

## Storage backends

This file tree is the default (`file`) backend. The same library can live in an
embedded SQLite catalog (`backend = "sqlite"`) or a central gRPC service
(`backend = "grpc"`) — selection semantics are identical, and
`agent_prompt::migrate` moves a catalog between them. See
[`docs/design/prompts/05-storage.md`](../docs/design/prompts/05-storage.md).
