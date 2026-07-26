# Design: the Prompt Library (situational selection · storage backends)

Status: **design / pre-implementation.** This directory is the design of record for
turning the agent's prompts into a **pre-created, human-legible, editable library**
whose fragments are **selected by the current situation** (which mode, and — as the
signals arrive — which model tier, language, or task) and stored behind a **swappable
backend** (files, SQLite, or a remote catalog). A newcomer can see and edit *"what
drives Review mode vs Debug mode,"* while an operator can grow a wide, tagged catalog.
It is a draft to refine; where the shipped code later refines a detail,
[`STATUS.md`](STATUS.md) becomes authoritative — the same convention as
[`../portal/`](../portal/README.md),
[`../adaptive-cognition/`](../adaptive-cognition/README.md) and
[`../code-review/`](../code-review/README.md).

> **Design in two rounds.** Round 1 (docs 01–03) gave the prompts a home and a
> base + per-mode-fragment layout. Round 2 (docs 04–05) generalises the per-mode
> *key* into a freeform **tag** so prompts are chosen by *any* aspect of the
> situation, and makes **storage a backend seam** so the same catalog can live in
> files or a database. Round 2 **subsumes** Round 1: `mode` becomes one tag among
> many, and `prompts/modes/<mode>/` becomes sugar for "tag these `mode:<mode>`."

## The idea

`agent-seddon`'s thesis is **legibility**: every capability is a
[seam](../../architecture.md), and the [Agent Portal](../portal/README.md) already
ships a `PromptStore` seam (`PromptService`) that CRUDs every prompt. But the
system prompt it manages is **one static string** — `[agent] system_prompt`,
resolved once at startup ([`agent_prompt::resolve_system_prompt`](../../../crates/agent-prompt/src/lib.rs)),
identical in every mode. The portal's own tracker names the gap:

> **Per-mode *system* prompts** (not just per-mode compaction lenses). Today only
> the compaction lens is per-mode; a per-mode system prompt would be a separate
> design on the `ContextStrategy`/prompt seams. — [`../portal/STATUS.md`](../portal/STATUS.md)

The loop already knows what mode it is in — the classifier sets `Session.current_mode`
every turn ([`docs/components/mode.md`](../../components/mode.md)), and the six modes
are `Implement, Debug, Review, Design, Explain, Other`. What it *cannot* do is say
anything different to the model in each one. The only per-mode prompt content that
exists today is the **compaction lens** (how to summarise when *entering* a mode —
[`agent-context/src/lens.rs`](../../../crates/agent-context/src/lens.rs)), not what
the agent is *told to do* while in it.

This design closes that gap and, in doing so, gives the prompts a home:

> A **prompt library** — a base that is always applied plus **tagged fragments**
> selected when the situation supplies their tags (`mode:review`, later
> `language:rust`, `tier:heavy`, …), each fragment a small, ordered, editable unit,
> shipped with curated defaults a user can then modify. The library lives behind the
> `PromptStore` seam, so it is stored as **files** (the git-legible default) or in a
> **database** without any consumer knowing the difference.

The two independent ideas, kept separate:

- **Selection** ([`04-selection.md`](04-selection.md)) — a fragment carries a freeform
  **tag set**; the loop builds a `PromptContext` (the situation's tags); a fragment is
  selected when its tags are a subset of the context, and the selected fragments
  compose additively. Backend-agnostic; the substance.
- **Storage** ([`05-storage.md`](05-storage.md)) — `[prompts] backend = file | sqlite
  | grpc`, all behind the seam's one gRPC CRUD. Because the seam abstracts it, *where*
  a prompt lives does not matter to the loop or the portal — which is exactly what
  lets us offer both a legible file tree and a wide queryable catalog.

## Principles

1. **Legible, whatever the backend.** In the file backend the layout *is* the
   documentation — numbered `.md` fragments you read, diff and edit without touching
   Rust ([`01-layout.md`](01-layout.md)). In any backend, the portal's
   `PreviewAssembled` shows the exact fragments selected for a situation and *why* —
   so even a DB catalog stays inspectable ([`04`](04-selection.md#legibility-why-did-this-prompt-get-picked)).
2. **Opt-in over compiled defaults.** No fragments present ⇒ behaviour is
   *byte-identical* to today, so `nix flake check` (whose sandbox has no operator
   files) stays green. Curated content ships as example templates; nothing is active
   until copied in. The DB backend is **feature-gated off by default** so a plain
   build pulls no new dependency. This generalises the
   [lens-externalization precedent](../portal/01-backend-seams.md) the portal shipped.
3. **Additive, not replace.** Selected fragments are *appended* to the base, never a
   wholesale replacement — so identity/tool-guidance is written once, matches compose
   without a conflict rule, and the prompt-cache prefix stays stable
   ([`02-composition.md`](02-composition.md)).
4. **Tags are freeform and opaque.** The resolver set-matches tags; it does **not**
   know what `rust` or `heavy` *mean*. That keeps the model tiny and future-proof —
   a new axis is a new tag plus one line in the context builder, not a new mechanism
   ([`04`](04-selection.md)).

## The compose model

Given a `PromptContext` (the situation's tags — today `{mode:<M>}`), the model's
system context is layered (top = most stable, bottom = most volatile — the ordering
that keeps the cache prefix intact):

```
┌─ base ───────────────  untagged fragments        → else [agent] system_prompt   ── stable, cached
├─ situational ────────  fragments whose tags ⊆ context  → else (empty)            ── volatile per-switch
├─ user context ───────  context.d/prepend/*        + recalled memory              ── existing assemble
│  user: <goal>
└─ append context ─────  context.d/append/*         (trailing system message)      ── existing assemble
```

A fragment is selected when **all its tags are present in the context**
(`{mode:review}` when review is active; `{mode:review, language:rust}` only when
both), and the selected fragments concatenate in `order` after the base
([`04`](04-selection.md)). In the **file backend** the base is `prompts/system/*.md`
and a `mode:` tag comes from the `prompts/modes/<M>/` directory; in the **SQLite
backend** both are rows with a tag set — same model, different encoding
([`05`](05-storage.md)).

Orthogonally, **on a switch into `M`** the summariser is driven by
`prompts/lens/M/*.md` (→ compiled default) — the already-shipped
[mode-aware compaction](../adaptive-cognition/02-compaction.md) path, unchanged
except that the lens slot gains the same multi-fragment form.

Everything above the goal is a legibility win the portal can render verbatim via
`PreviewAssembled`.

## The documents

| # | Doc | One line |
|---|---|---|
| — | [`README.md`](README.md) | The idea, the compose model, and the peer comparison (this file). |
| 01 | [`01-layout.md`](01-layout.md) | The **file backend's** directory layout, the slot/fragment resolver, backward-compat with the shipped single-file form, and the `PromptKind::SystemFragment` extension point. |
| 02 | [`02-composition.md`](02-composition.md) | How the selected fragments fold into `assemble_messages`, the cache-tiering decision, and the interaction with `context.d` and dimensional recall. |
| 03 | [`03-content.md`](03-content.md) | The **drafted prompt text** — the base fragments and per-mode (tagged) fragments, grounded in the repo's real conventions. |
| 04 | [`04-selection.md`](04-selection.md) | **Situational selection**: the tag model, `PromptContext`, the subset-match rule, which axes are free vs deferred, legibility, and tag security. |
| 05 | [`05-storage.md`](05-storage.md) | **Storage backends**: `[prompts] backend = file \| sqlite \| grpc`, the SQLite schema + dependency honesty, and the file↔db legibility bridge. |
| — | [`STATUS.md`](STATUS.md) | The implementation tracker and increment order. |

## Compare and contrast: how three peer agents organise prompts

The three peer codebases in the parity set each solve "base prompt + situational
variation" differently. The comparison sharpens our choices; the last column is
what this design takes.

| Dimension | **Hermes** (Python) | **pi** (TS) | **opencode** (TS) | **agent-seddon** (this design) |
|---|---|---|---|---|
| Core prompt storage | Python string constants (`agent/prompt_builder.py`) | one TS template literal (`core/system-prompt.ts`) | separate `.txt` files (`session/prompt/*.txt`) | ordered `.md` fragments under `prompts/system/` (compiled default fallback) |
| Persona / identity | `SOUL.md` under `HERMES_HOME`, else `DEFAULT_AGENT_IDENTITY` | the single `buildSystemPrompt` body | model-selected base + optional per-agent `prompt` | `prompts/system/*.md` → config `[agent] system_prompt` |
| "Modes" | **Profiles** (per message *source*) + skills + routines; no task modes | slash-command **prompt templates**; execution modes only | **first-class agents/modes** (`mode: primary\|subagent\|all`) | mode is one **tag** among many; live task-mode detection supplies `mode:<m>`, extensible to `tier:`/`language:`/… |
| Selection key | single persona per profile | one template per command | one agent's `prompt` per turn | **freeform tag set**, subset-matched against the situation ([`04`](04-selection.md)) |
| Base ↔ variant relation | tiered **stable/context/volatile**, additive | base + `appendSystemPrompt` / `customPrompt` **replace** | agent `prompt` **replaces** the model base, then env/instructions/mcp/skills appended | base **kept**, selected fragments **appended** (additive, cache-stable) |
| Override grammar | `platform_hints` **replace / append** | `customPrompt` / `appendSystemPrompt` / `promptGuidelines` | frontmatter config + body-as-prompt | operator file **overrides its slot**; fragments compose by `NNNN` order |
| User prompts on FS | `SOUL.md`, `AGENTS.md`, `.hermes.md`, `config.yaml` | `~/.pi/agent/prompts/*.md`, `.pi/prompts/*.md`, `--prompt-template` | `{agent,agents}/**/*.md`, `{mode,modes}/*.md`, `opencode.jsonc` | `prompts/{system,modes,lens}/…`, plus `context.d/{prepend,append}/` |
| File format | markdown (context/skills), code (core) | markdown + YAML frontmatter, `$1/$@` variables | `.txt` (built-in) + markdown+frontmatter (custom) | plain markdown fragments (no template engine — see below) |

**What we borrow, and why.**

- **From opencode — modes as filesystem-legible units.** Its `{mode,modes}/*.md`
  (frontmatter → config, body → prompt) and its explicit *segment array*
  (`base → env → instructions → mcp → skills`) are the closest peer to what we want.
  We take the "a mode is a place on disk" idea but keep our **additive** layering
  (opencode *replaces* the base with the agent prompt; we append), so identity and
  tool guidance are authored once, not re-stated per mode — which also protects the
  prompt-cache prefix.
- **From hermes — explicit stable/context/volatile tiering.** This maps directly
  onto our `[cache] stable-prefix` strategy: the base is the stable tier (cached for
  the whole session), the situational block is the volatile tier (invalidated only on a
  switch). See [`02-composition.md`](02-composition.md). We also mirror its
  **replace/append** override intent, expressed as *"a slot's files override its
  compiled default; multiple files within a slot compose in order."*
- **From pi — layered discovery + variable templates.** pi discovers prompt files
  from global → project → package → CLI and interpolates `$1/$@`. We adopt the
  *spirit* (a clear precedence: operator files override compiled defaults) but
  **deliberately keep fragments as plain markdown with no template engine** —
  consistent with the repo's "a lens/system prompt is a fixed, human-written,
  non-model-derived string" invariant ([`lens.rs`](../../../crates/agent-context/src/lens.rs)).
  Variable interpolation is noted as a possible follow-up in [`STATUS.md`](STATUS.md),
  not scoped here.

**Where we go beyond all three.** Each peer selects prompts on **one** axis — hermes
by message *source* (profile), pi by *command* (template), opencode by *agent*. None
selects on a **combination** of situational signals, and none puts the catalog behind
a storage seam that can be a database. Multi-tag situational selection
([`04`](04-selection.md)) and a file-or-DB backend ([`05`](05-storage.md)) are this
design's advance — enabled by the `PromptStore` seam the portal already shipped.

**Where all three converge — and where our `context.d` already sits.** Every peer
keeps *project instructions* (`AGENTS.md` / `.hermes.md` / `.cursorrules`) as a
distinct segment, separate from the base persona. agent-seddon's analog already
exists and is unchanged by this design: [`context.d/`](../../../context.d/README.md)
(`prepend/` folds into the system message; `append/` trails it). The prompt library
this design adds is the **agent-authored** half (identity + per-mode behaviour); the
`context.d` files are the **user/project-authored** half. Keeping them in separate
roots — `prompts/` vs `context.d/` — mirrors the peers' persona-vs-project split.

## Security posture (inherited, not invented)

The harness rule holds: **operator and model input is untrusted, fail closed**
([`CLAUDE.md`](../../../CLAUDE.md)). This design adds no new trust: a file-backend
`id` that becomes a filename passes `safe_prompt_file` + `confine` (traversal /
symlink-escape blocked); **tags are opaque, bounded strings** — capped in count and
length, string-compared only, never a path segment and never string-built SQL (the
SQLite backend uses bound parameters); content is size-capped before write; and the
assembled fragment still becomes a **system** message, so the loop's existing
`scan_for_injection` continues to screen it at turn time — the library edits the
*source*, the loop still guards the *use*. Details in
[`04-selection.md`](04-selection.md#security--tags-are-untrusted-labels) and the
SQLite `bound-parameters` note in [`05-storage.md`](05-storage.md#sqlite--first-class-local-catalog-opt-in-dependency).

## Non-goals

- **No template / variable engine.** Fragments are fixed markdown, not interpolated
  (the lens/system-prompt "fixed human-written string" invariant). A `$var` layer is
  a possible follow-up, not this design.
- **No axis semantics / specificity ranking.** Tags are opaque strings, matched by
  subset; there is no built-in meaning for `rust` or `heavy` and no most-specific-wins
  ladder — matches simply compose. Precedence, if ever needed, would be a later
  addition ([`04`](04-selection.md)).
- **No new situational *signals* here.** Only `mode` is wired (it already exists).
  `tier`/`effort`/`language`/`task` are expressible as tags today but inert until
  their signal source lands — each its own increment ([`STATUS.md`](STATUS.md)); the
  `tier` case additionally needs the loop to decide the tier *before* assembly.
- **No prompt versioning / history.** The shipped `PromptStore` has none; this design
  does not add it. Git (file backend) or the DB's own tooling is the history.
- **No new mode taxonomy.** The six `TaskMode`s are fixed by
  [`adaptive-cognition/01-mode.md`](../adaptive-cognition/01-mode.md); this design
  makes `mode` a tag, it does not add or rename modes.
- **No per-mode *tool* gating.** opencode's modes also restrict tools (e.g. `plan`
  denies edits); that is a `Policy`-seam concern, explicitly out of scope here.
