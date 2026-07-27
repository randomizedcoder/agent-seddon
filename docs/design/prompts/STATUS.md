# Prompt Library — implementation status

The living tracker for the [Prompt Library](README.md) design. **Increments 01
(`647add8`) and 03 (`d99cfe9`, PR #145) are merged**; **increment 02 (file backend +
management surface) is coded and in review**; the remaining increments are
pre-implementation. Where the shipped code refines a design detail, this file becomes
authoritative (the same convention as
[`../portal/STATUS.md`](../portal/STATUS.md) and
[`../adaptive-cognition/STATUS.md`](../adaptive-cognition/STATUS.md)).

This track realises the **"Per-mode *system* prompts"** item deferred by the portal
([`../portal/STATUS.md`](../portal/STATUS.md)), building directly on that track's
shipped `PromptStore` seam and its lens-externalization precedent — then generalises
it to **tag-based situational selection** ([`04`](04-selection.md)) over a **swappable
storage backend** ([`05`](05-storage.md)).

## Increments

One focused, individually gated PR per increment — each earns the next, and each is
byte-identical to current behaviour when no fragments are present (so
`nix flake check`, whose sandbox has none, stays green throughout).

| # | Increment | Core | Context | Prompt seam | Runtime | Wire | Status |
|---|---|:--:|:--:|:--:|:--:|:--:|:--:|
| 01 | **Tag model + resolver** — `PromptContext`; `SystemFragments` resolver (`select(ctx)`, single-file + empty-default fallback); `LensPrompts` gains the dir form | ✅ | ✅ | — | — | — | **merged** |
| 02 | **File backend + management surface** — `PromptKind::SystemFragment`, `PromptEntry.tags`, `FilePromptStore` CRUD + directory/frontmatter tags, `mode`-context `preview_assembled`; additive wire (`PROMPT_KIND_SYSTEM_FRAGMENT=5` + `PromptEntry.tags=7`) | ✅ | — | ✅ | — | ✅ (additive) | **in review** |
| 03 | **Composition / runtime** — build the `mode:` context, inject/swap the volatile situational message at first-assembly + `record_mode_switch`; `agent_prompt_fragments_selected_total{mode,action}` counter | — | — | — | ✅ | — | **merged** |
| 04 | **Wire** — `PromptContext` message + `Select(PromptContext)` / preview-by-context (the enum value + `PromptEntry.tags` already landed additively in 02; no baseline bump) | — | — | ✅ | — | ✅ | **designed** |
| 05 | **`sqlite` backend** — `[prompts] backend`; `SqlitePromptStore` (feature `prompt-sqlite`, first DB dep); the `file↔sqlite` bridge | — | — | ✅ | ✅ | — | **designed** |
| 06 | **Content** — ship the example fragments (`prompts/modes.example/…`) + `prompts/README.md`; component-doc update | — | — | — | — | — | **designed** |

**Build order rationale:** 01 is the pure, testable resolver + tag model (no wire, no
loop, no backend). 02 makes fragments visible/editable through the existing portal with
no behaviour change. 03 is the only loop change — small, gated by the counter/span and
the byte-identical default. 04 completes the wire so a remote/portal client sees tags +
selection. 05 adds the database backend **as its own PR** so the first workspace DB
dependency lands reviewable in isolation. 06 is inert content that turns the capability
on for whoever opts in.

## Dependencies

- 02 depends on 01 (the store reads/writes through the tag model).
- 03 depends on 01 (it calls `SystemFragments::select`).
- 04 depends on 01/02 (it exposes their types on the wire).
- 05 depends on 01–04 (it is another backend behind the same seam + wire); **it is
  the only increment that adds a dependency**, so it is deliberately last and separate.
- 06 depends on nothing in code — example markdown + docs — but is most useful once 03
  makes a copied fragment actually take effect.

## Which axes ship, and which wait

Only **`mode`** is a free signal at assembly time, so it is the one tag **wired** (03).
The framework is axis-agnostic ([`04`](04-selection.md)); the other axes are already
expressible as tags but **inert until their signal source lands** — each a later,
independent increment:

| Axis | Prerequisite before it can fill the `PromptContext` |
|---|---|
| `mode:` | **none — wired in 03** |
| `language:` | aggregate the tantivy `lang` field (or read `[[lsp.servers]]`) into a per-run signal |
| `tier:` | invert control — decide the pool tier **before** assembly and thread it into the context + request |
| `task:` (feature / unit-test / integration-test) | extend the mode classifier taxonomy (or add a second classifier) |
| `effort:` | introduce a per-turn effort knob (no such concept exists today) |

## Notes / decisions of record

- **As-built (01):** `SystemFragments::select` returns the **situational** fragments
  only (`modes/<mode>/`) — *not* the untagged base. The base stays with
  `agent_prompt::resolve_system_prompt` so it is never duplicated in the assembled
  message; a multi-fragment base (`system/*`) is a later refinement of that startup
  hook. This narrows the `01-layout.md` sketch (which described `select` reading base +
  situational) to avoid a double-injected base.
- **As-built (03):** landed **before 02** (allowed — both depend only on 01). The
  selected fragments are one system message held at **index 1** (right after the head),
  swapped in place on a mode change and removed when nothing matches — a *leading*
  system message, so compaction preserves it. `Session.situational_present` tracks it;
  `SystemFragments` hangs off `Agent` (`with_system_fragments`, wired from
  `[prompts] dir` in `builder.rs`). Only `mode:<current_mode>` fills the context today
  (`Session::prompt_context`). e2e-proven end to end
  (`tests/prompt_fragments_e2e.rs`): a fragment reaches the model for `Other`, a review
  goal selects the `review` fragment, no `prompts` dir injects nothing, and a
  mid-session mode switch **swaps the message in place** (`updated`) or **drops it**
  (`removed`) — each asserting the index-1 leading-position invariant. The
  loop-consumed behaviour is documented in
  [`docs/components/prompt.md`](../../components/prompt.md) ("Situational fragments").
- **As-built (02):** `FilePromptStore` gains `SystemFragment` CRUD over
  `modes/<mode>/<file>.md` (id = `<mode>/<file>`, one entry per file — like
  prepend/append, not a fixed per-mode row). Three refinements of the sketch:
  - **`tags` is a read projection, not a stored column.** The file backend derives
    `PromptEntry.tags` = `mode:<mode>` ∪ frontmatter `tags:` (bounded by
    `MAX_PROMPT_TAGS`/`_LEN`); a `put` persists tags by writing them into the content
    (frontmatter) and `get` re-derives them. The sqlite backend (05) gets a real tag
    column. So a `put(entry)` **ignores** `entry.tags` — content is the source of truth.
  - **Frontmatter parser is copied into `agent-prompt`, not shared from `skills.rs`.**
    `agent-prompt` cannot depend on `agent-runtime` (cycle), so it carries its own
    minimal `split_frontmatter`/`frontmatter_list`/`frontmatter_scalar` (inline
    `tags: [a, b]` + scalar; no block sequences) — the repo's "each crate copies its
    own small helper" convention. `skills.rs` is untouched.
  - **Wire enum value + `tags` field landed here (additive).**
    `PROMPT_KIND_SYSTEM_FRAGMENT = 5` and `PromptEntry.tags = 7` are in 02 (a
    `SystemFragment` must roundtrip faithfully through gRPC in the increment that
    introduces it), proven on TCP+UDS. 04 is now just the `PromptContext` message +
    `Select`/preview-by-context.
  - **Deferred to a later increment:** frontmatter-driven *selection* (the resolver is
    still `mode:`-tag-only, per as-built 01) and frontmatter-stripping on inject; the
    synthetic empty-per-mode list entry (not needed — the portal handles a multi-entry
    kind like prepend/append); `[prompts] backend`/`db_path` config (05).
  `FilePromptStore` unit tests + a `system_fragment_tags_roundtrip` wire test
  (TCP+UDS) cover the four case classes with mandatory `adversarial_` cases (id
  traversal, frontmatter tag-count/length overflow).
- **Base each PR off `main`** — do not stack (the lesson carried from the code-review
  and portal tracks).
- **Opt-in, empty/compiled-default fallback** is load-bearing: no fragments ⇒ today's
  behaviour exactly, so the gate stays green and no clone is silently re-prompted. The
  content ships as **example templates**, not active committed files; the DB backend is
  **feature-gated off**.
- **Freeform tags, subset match, additive, no specificity ladder** — the selection
  model ([`04`](04-selection.md)); the **requirements** match direction
  (`fragment.tags ⊆ context`) is the one open point flagged for the reviewer there.
- **Additive placement (a separate volatile situational message), not a folded/replaced
  head** — chosen for the prompt cache
  ([`02`](02-composition.md#why-a-separate-message-not-folded-into-the-base-the-cache-decision)).
- **Tags are untrusted, opaque, bounded** (count/length capped, string-compared, bound
  SQL params); the file-backend `id` still goes through `safe_prompt_file` + `confine`.
  `adversarial_` tests (tag overflow, SQL metachars, traversal id, oversize body) are
  mandatory, over the wire on TCP + UDS.
- **The SQLite dependency is the first in the workspace** — feature-gated, pinned in
  `nix/versions.nix`, `cargo-audit`ed by the gate, and confined to increment 05.
- **Reuse, don't reinvent:** `SystemFragments` mirrors `LensPrompts`; the backend
  `match` copies the `SessionStore` template; ordering reuses `numeric_prefix`; tags
  reuse the `skills.rs` frontmatter parser; the `grpc` backend reuses the already-built
  `GrpcPrompts`; the injection reuses the existing `pending_context` message path.

## Deferred (documented, not scoped here)

- **Signal sources for the non-`mode` axes** — see the table above; each is its own
  follow-up, and `tier:` additionally needs the pre-assembly tier decision.
- **A `$var` / template layer** in fragments (pi-style `$1/$@`). Kept out to preserve
  the "fixed, human-written, non-model-derived string" invariant.
- **Specificity / precedence** among selected fragments. Not needed — selection is
  additive; add only if a real conflict appears.
- **Remote SQL as a first-class local backend** (postgres/mariadb via a direct client).
  Deliberately routed through `backend = "grpc"` → a central prompt service instead, so
  the agent keeps no SQL client; the service owns the database.
- **Per-mode *tool* gating** (opencode's `plan` denies edits) — a `Policy`-seam concern.
- **Live re-resolution of the base `system/`** mid-session. Base is resolved at startup
  (next run); only the situational fragments + lens are live. Unmotivated to change.
- **Prompt versioning / history.** None in the shipped `PromptStore`; git (file) or the
  DB's tooling is the history.
