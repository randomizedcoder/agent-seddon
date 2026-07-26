# 01 — The directory layout (the `file` backend)

> **Scope.** This doc describes the **`file` backend** — the default, git-legible
> storage for the prompt library. Storage is a seam
> ([`05-storage.md`](05-storage.md)); the same fragments+tags model
> ([`04-selection.md`](04-selection.md)) can equally live in SQLite. Read this as
> *how the file backend realises the model*: a directory is one encoding of "a
> fragment with a tag set."

The prompt library lives under the existing **`[prompts] dir`** root (default
`prompts/`, [`config/agent.toml`](../../../config/agent.toml)) — the same root the
shipped `PromptStore` seam already reads for `system.md` and `lens/<mode>.md`
([`agent-prompt/src/lib.rs`](../../../crates/agent-prompt/src/lib.rs)). This design
**generalises** that root so that every prompt slot is a *directory of ordered
fragments*, and adds a new **system fragment** slot (`modes/`) next to the existing
per-mode **lens** slot. The directory encodes a fragment's coarse tag (a
`modes/<mode>/` fragment carries the tag `mode:<mode>`); **frontmatter adds finer
tags** ([below](#tags-directory--frontmatter)).

## The tree

```
prompts/
├── README.md                     # human orientation (NOT loaded — like context.d/README.md)
│
├── system/                       # BASE system prompt — ALWAYS applied, every mode
│   ├── 0001_identity.md          #   who the agent is
│   ├── 0002_tools.md             #   tool-use guidance
│   └── 0003_conventions.md       #   working style / output shape
│
├── modes/                        # per-mode SYSTEM fragment — applied when current_mode == <mode>
│   ├── implement/
│   │   ├── 0001_focus.md
│   │   └── 0002_output.md
│   ├── debug/
│   ├── review/
│   ├── design/
│   ├── explain/
│   └── other/                    #   usually absent/empty — Other is the neutral base
│
└── lens/                         # per-mode COMPACTION lens (shipped) — now also multi-fragment
    ├── implement/                #   dir form:  lens/<mode>/NNNN_*.md
    │   └── 0001_lens.md
    ├── review.md                 #   shipped single-file form still honoured
    └── …
```

Only `*.md` files inside the slot sub-directories are read; the top-level
`README.md` is orientation and is never injected (mirroring
[`context.d/README.md`](../../../context.d/README.md)).

## The unifying rule: a **slot** is a directory of ordered fragments

Every prompt slot resolves the same way, so there is exactly one convention for a
newcomer to learn (the one they already know from `context.d`):

- A slot's text = every `NNNN_*.md` file in the slot directory, **concatenated in
  ascending numeric order**. The `NNNN_` prefix orders them; files without a numeric
  prefix sort last, by name. This reuses
  [`numeric_prefix`](../../../crates/agent-prompt/src/lib.rs) verbatim (the same
  helper `context.d` and the shipped `FilePromptStore` already use).
- Concatenation joins fragments with a blank line. Each fragment is *just its
  markdown* — no per-file heading is imposed (unlike `context.d`, whose loader wraps
  each block in `## <source>`), because these fragments compose one coherent
  instruction, not a list of labelled notes.
- An **empty or missing** slot directory ⇒ the slot's **compiled default** (base
  system → config `[agent] system_prompt`; a situational fragment → empty; a lens →
  `agent_context::lens::builtin_instruction`).

This is the "multiple prompts per concern" shape: `modes/review/` can hold
`0001_focus.md`, `0002_severity.md`, `0003_output.md` as three independently
editable files that compose into Review mode's instruction — rather than one
monolithic file.

### Backward compatibility with the shipped single-file form

Increment #128 shipped `prompts/system.md` and `prompts/lens/<mode>.md` as **single
files**. Those must keep working. Resolution for a slot is therefore, in order:

1. the **directory** form `<slot>/…/*.md` (this design's multi-fragment form), else
2. the **single-file** form `<slot>.md` (the shipped form — treated as a
   one-fragment slot), else
3. the **compiled default**.

So `lens/review/0001_lens.md` (new) wins over `lens/review.md` (shipped) wins over
the compiled `REVIEW` constant. No shipped file breaks, and an operator can migrate
a single file into a directory at their own pace. The `system/` base follows the
same ladder: `system/*.md` → `system.md` → `[agent] system_prompt`.

### Tags: directory + frontmatter

A fragment's **tag set** ([`04-selection.md`](04-selection.md)) is where situational
selection comes from. In the file backend a fragment gets its tags two ways:

- **From its directory** — a fragment under `modes/<mode>/` carries `mode:<mode>`.
  This is why the tree reads intuitively: the folder names the situation. `system/*`
  fragments have **no** directory tag → they are the always-on base.
- **From YAML-ish frontmatter** — for anything finer than the folder:

  ```markdown
  ---
  tags: [mode:review, language:rust]
  order: 20
  ---
  When reviewing Rust, watch `unsafe`, lifetimes at API boundaries, and `Result`
  swallowing. …
  ```

  Parsed by extending the existing hand parser
  ([`skills.rs::split_frontmatter`/`field`](../../../crates/agent-runtime/src/skills.rs)) —
  **no `serde_yaml` dependency** (the repo has none); it gains minimal `tags: [a, b]`
  list support and nothing else. A directory tag and frontmatter tags **union**
  (a `modes/review/…` file with `tags: [language:rust]` has both `mode:review` and
  `language:rust`). A frontmatter `order:` overrides the `NNNN` prefix if present.

A newcomer never *has* to touch frontmatter — the folder layout alone covers the
per-mode case (Round 1). Frontmatter is the escape hatch for the wider,
multi-tag catalog.

## The new slot: system fragments (`modes/`)

`modes/` is the genuinely new slot: **tagged system fragments**. A subdirectory names
the coarse tag (`modes/review/` ⇒ `mode:review`); the fragments inside are what the
agent is *told to do* when that tag is in the situation. It is the analog of the
shipped `lens/<mode>/`, differing in *when* and *how* it is used:

| | `lens/<mode>/` (shipped) | `modes/<mode>/` (this design) |
|---|---|---|
| What it is | how to **compact** when *entering* the mode | what to **do** while the situation matches |
| Selected by | the destination mode of a switch | tags ⊆ context ([`04`](04-selection.md)) |
| When applied | on a mode switch, at switch-compaction | every turn its tags are satisfied |
| Where it lands | the summariser's instruction | appended to the system prompt (see [`02`](02-composition.md)) |
| Default when absent | compiled `builtin_instruction` (non-empty) | **empty** (base-only behaviour) |
| Resolver | `LensPrompts` (live per lookup) | **`SystemFragments`** (live per lookup) — new, sibling |

The default being **empty** (not a compiled string) is deliberate: with no files,
the system prompt is exactly today's — one string, every situation — so behaviour is
byte-identical and the gate stays green. The pre-created content in
[`03-content.md`](03-content.md) is what a user *drops in* (or the repo ships as
example templates under, e.g., `prompts/modes.example/`), never active by default.

## The resolver: `SystemFragments` (mirror of `LensPrompts`)

A small resolver in `agent-context`, a direct sibling of
[`lens::LensPrompts`](../../../crates/agent-context/src/lens.rs). It reads the tagged
fragments and returns those the situation selects:

```rust
/// System fragments selected by a PromptContext. In the file backend, reads
/// `<prompts_dir>/system/*` (untagged base) + `<prompts_dir>/modes/<mode>/*`
/// (tag `mode:<mode>`) + frontmatter tags; returns the fragments whose tags ⊆ context,
/// in `order`. Empty context + no fragments ⇒ "" (base-only).
pub struct SystemFragments { root: Option<PathBuf> }

impl SystemFragments {
    pub fn defaults() -> Self;                        // no dir, no I/O
    pub fn new(prompts_dir: Option<&str>) -> Self;    // roots at <prompts_dir>

    /// The concatenated selected fragments for `ctx`. Read per lookup so a
    /// PromptStore edit is picked up next turn with no restart. Allocation-free
    /// `Cow::Borrowed("")` when no dir / nothing selected.
    pub fn select(&self, ctx: &PromptContext) -> Cow<'static, str>;
}
```

(The concrete store behind it is backend-dependent — a file walk here, a SQL query in
the SQLite backend — but the `SystemFragments` façade and its `select(ctx)` contract
are the same either way; see [`05`](05-storage.md).)

Two properties carried over from `LensPrompts` on purpose:

- **Live reads.** `select()` reads on each lookup (not at construction), so an edit
  via the `PromptService` seam takes effect on the **next turn** — matching the lens.
- **Allocation-free default.** With no `root` (or nothing selected) the lookup returns
  `Cow::Borrowed("")` — no heap, no I/O — so the per-turn cost when the feature is
  unused is nil, keeping the perf/leak budgets unmoved (the lens externalization
  argument).

In the file backend, `<mode>` in a `modes/<mode>/` path is validated against the
closed `TaskMode` set (never raw attacker text). The per-file `id` (filename) and the
frontmatter tags are the free-form parts, guarded below and in
[`04`](04-selection.md#security--tags-are-untrusted-labels).

## The management surface: `PromptKind::SystemFragment`

The shipped `PromptStore` ([`agent-core/src/lib.rs`](../../../crates/agent-core/src/lib.rs))
enumerates `System | Prepend | Append | ModeLens`. This design adds one generalized
variant — a **tagged system fragment**, of which "a per-mode system prompt" is just
the `mode:<m>`-tagged case:

```rust
pub enum PromptKind {
    System, Prepend, Append, ModeLens,
    SystemFragment,   // NEW: a tagged system fragment (file: modes/<mode>/<file>.md or a tagged fragment; sqlite: a row)
}
```

`PromptEntry` gains `tags: Vec<String>` ([`04` wire](04-selection.md#wire)). Identity
and CRUD follow the existing multi-entry precedent (`Prepend`/`Append`):

- **`id`** is the fragment's stable id — a `<mode>/<filename>` path in the file
  backend, an opaque key in SQLite. `list(SystemFragment)` returns one `PromptEntry`
  per fragment (with its `tags` and `order`).
- A mode subdir with **no files** surfaces as a synthetic `builtin=true`, empty entry —
  exactly how an un-overridden `ModeLens` surfaces its default today, so the portal's
  "grouped by kind, 🔒 on defaults" view needs no new case.
- `Put` creates/updates a fragment (writing the file, or upserting the row + its
  tags); `Delete` removes it.

`preview_assembled` ([`agent-prompt/src/lib.rs`](../../../crates/agent-prompt/src/lib.rs))
is extended to take a **`PromptContext`** (a tag set) rather than a bare `TaskMode`, so
it folds the *selected* fragments into the previewed system message — making *"see the
prompt for this situation"* literal ([`04`](04-selection.md#legibility-why-did-this-prompt-get-picked)).

## Security

Untrusted input, **fail closed** ([`CLAUDE.md`](../../../CLAUDE.md)) — no new trust
boundary, the shipped guards apply unchanged:

- **The `<mode>` segment** is `TaskMode::as_str()` from the closed set — never raw
  attacker text — so it cannot traverse. An unknown mode string on the wire is
  rejected at the `PromptRef` → core conversion (parse against `TaskMode`), not
  sanitised.
- **The fragment `id`** (filename) passes
  [`safe_prompt_file`](../../../crates/agent-prompt/src/lib.rs): single `.md`
  segment, no `..`, no separators, no leading `-`, `[A-Za-z0-9._-]`, length-capped;
  the resolved path is then `confine`d to the `prompts/` root (symlink-escape
  blocked). Adversarial cases (`../../etc/passwd`, `review/../../x`, a symlinked
  `modes/` dir, an over-long id, an over-cap body) are **mandatory** and assert
  rejection.
- **Content is size-capped** (`MAX_CONTENT_BYTES`, the shipped 64 KiB) before write.
- **Tags are bounded, opaque strings** — count + length capped, string-compared only,
  never a path or SQL fragment. Full treatment in
  [`04`](04-selection.md#security--tags-are-untrusted-labels).
- **The assembled fragment is still injection-screened at use.** It becomes part of a
  **system** message, so the loop's `scan_for_injection` continues to screen the
  assembled context at turn time — identical to how `context.d` content is guarded.
  The library edits the *source*; the loop guards the *use*.

## What this increment lands

| File | Change |
|---|---|
| `crates/agent-context/src/system_fragments.rs` (new) | `SystemFragments` resolver (sibling to `lens.rs`) — read tagged fragments, `select(ctx)`, single-file + empty-default fallback |
| `crates/agent-context/src/lens.rs` | teach `LensPrompts` the `lens/<mode>/*.md` dir form (keep single-file fallback) — one shared slot helper |
| `crates/agent-core/src/lib.rs` | `PromptKind::SystemFragment` + `as_str`/`parse` arms; `PromptEntry.tags`; `PromptContext` |
| `crates/agent-prompt/src/lib.rs` | `FilePromptStore`: `SystemFragment` CRUD; directory + frontmatter tags; `PromptContext`-aware `preview_assembled` |
| `crates/agent-runtime/src/skills.rs` | minimal `tags: [a, b]` list support in the frontmatter hand parser |
| `config/agent.toml` | `[prompts]` comment documents `system/`, `modes/`, `lens/` + `backend`/`db_path` ([`05`](05-storage.md)) |
| `prompts/README.md`, `prompts/modes.example/…` | the human orientation + pre-created example fragments (from [`03`](03-content.md)) |
| `docs/components/prompt.md` | the `SystemFragment` kind, tags, and the backends ([`05`](05-storage.md)) |

The **wire** (`prompt.proto`) needs the new enum value `PROMPT_KIND_SYSTEM_FRAGMENT =
5`, a `repeated string tags` field on `PromptEntry`, and a `PromptContext` message —
all **additive**, so `buf breaking` passes with no `buf.image.binpb` bump. The
**selection model** is [`04`](04-selection.md), the **runtime wiring** (folding the
selected fragments into the turn's system message) is [`02`](02-composition.md), and
the **storage backends** are [`05`](05-storage.md).
