# Prompt Library — implementation status

The living tracker for the [Prompt Library](README.md) design. **Increments 01
(`647add8`), 03 (PR #145), 02 (PR #146), 04 (PR #147) and 05 (`48b0cc4`, PR #148) are
merged**; **increment 06 (example content) is coded and in review** — the final
increment, so the track is feature-complete once it lands. Where the shipped code
refines a design detail, this file becomes authoritative (the same convention as
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
| 02 | **File backend + management surface** — `PromptKind::SystemFragment`, `PromptEntry.tags`, `FilePromptStore` CRUD + directory/frontmatter tags, `mode`-context `preview_assembled`; additive wire (`PROMPT_KIND_SYSTEM_FRAGMENT=5` + `PromptEntry.tags=7`) | ✅ | — | ✅ | — | ✅ (additive) | **merged** |
| 03 | **Composition / runtime** — build the `mode:` context, inject/swap the volatile situational message at first-assembly + `record_mode_switch`; `agent_prompt_fragments_selected_total{mode,action}` counter | — | — | — | ✅ | — | **merged** |
| 04 | **Wire** — `PromptContext` message + `Select(PromptContext)` / preview-by-context (the enum value + `PromptEntry.tags` already landed additively in 02; no baseline bump) | ✅ | — | ✅ | — | ✅ | **merged** |
| 05 | **`sqlite` backend** — `[prompts] backend` + `db_path`; `SqlitePromptStore` (feature `prompt-sqlite`, first DB dep); backend `match` in `builder.rs`; the `file↔sqlite` bridge (`migrate`) | — | — | ✅ | ✅ | — | **merged** |
| 06 | **Content** — ship the example fragments (`prompts/modes.example/…` + `system.example/`) + `prompts/README.md`; component-doc pointer | — | — | — | — | — | **in review** |

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
- **As-built (04):** the wire adds a `PromptContext { repeated string tags }` message,
  a `Select(PromptContext) returns (PromptList)` RPC, and a `context` field on
  `PreviewRequest` (all additive — `buf breaking` green, no `buf.image.binpb` bump).
  The `PromptStore` seam gains `select(&PromptContext)` and `preview_assembled` now
  takes a `PromptContext` (was `TaskMode`); the server derives the context from the
  request's tags, falling back to a `mode:<mode>` tag from the pre-04 scalar `mode`
  field so old clients still work. `agent_core::PromptContext ↔ pb::PromptContext`
  conversions insert through the **bounded** constructor (an over-cap/over-long tag is
  dropped — safe, a context is a query). `select` landed keying on the `mode:` tag
  alone (matching the shipped resolver); **05 converged it to the design's full
  `tags ⊆ ctx` predicate** — see as-built 05. `Select` returns the selected fragments
  as entries (the "which of their tags matched" annotation from `04-selection.md` is
  derivable from `entry.tags ∩ ctx`, so no extra proto surface). Proven by a
  `prompt_select_and_preview_by_context_roundtrip` test on TCP + UDS.
- **As-built (05):** `SqlitePromptStore` (`crates/agent-prompt/src/sqlite.rs`, feature
  `prompt-sqlite`) is the seam's second backend, selected by `[prompts] backend` via a
  `match` in `builder.rs` (the `SessionStore` template) — `file` | `sqlite` | `grpc`,
  with an `other => bail!`. Decisions of record:
  - **First workspace DB dependency, opt-in.** `rusqlite` (`bundled` — compiles
    vendored `sqlite3.c` with stdenv's `cc`, no system lib) behind the non-default
    `prompt-sqlite` feature, so a default build / `nix build .#agent` stays
    dependency-free. crane vendors it from `Cargo.lock` (no manual hash); `cargo-audit`
    covers it automatically. Version is `Cargo.lock`-pinned (recorded in
    `nix/versions.nix`).
  - **Backends made interchangeable — two convergences.** (1) `select` is now the
    design's `fragment.tags ⊆ context` in *both* backends (file: `ctx.covers`; sqlite:
    a `NOT EXISTS (… tag NOT IN …)` SQL pushdown with tags as **bound params**),
    superseding 04's mode-only rule. (2) `list`/`select` order **globally** by
    `(order, id)` (04-selection's composition rule) in both — the file backend's
    former mode-grouped order was dropped. `fragment_tags` is now **sorted**, so both
    backends return byte-identical `tags`.
  - **Same defaults + tag derivation as file.** `System`/`ModeLens` fall back to the
    config/compiled default (`builtin=true`) when no override row exists; a
    `SystemFragment`'s tags are derived from its content (`crate::fragment_tags`, dir ∪
    frontmatter), so `put` ignores the caller's `entry.tags` — the `prompt_tags` table
    is just a denormalised cache for the pushdown.
  - **Bridge.** `agent_prompt::migrate(from, to)` copies non-builtin entries either
    direction (file↔sqlite↔grpc); the `.#prompts-export/import` CLI/nix packaging is a
    deferred convenience follow-up.
  - **Gate coverage.** The main `test` check runs default features (can't enable
    `dhat-heap` etc.), and clippy `--all-features` compiles+lints the sqlite code but
    doesn't run it — so a dedicated `prompt-sqlite` check (`nix/checks/prompt-sqlite.nix`)
    *executes* the backend's tests (CRUD, defaults, `select` pushdown, adversarial id
    traversal + SQL-metachar tag inertness, and a file→sqlite `migrate`
    interchangeability test).
  - **Still deferred:** the loop's resolver (`agent-context`) keeps the `mode:`-only
    rule + no frontmatter-strip-on-inject; for the shipped no-frontmatter content it
    coincides with the store's `tags ⊆ ctx`, converging fully when the resolver is
    upgraded (a runtime change).
- **As-built (06):** the drafted content of [`03-content.md`](03-content.md) ships as
  **inert `.example/` templates** — `prompts/modes.example/<mode>/*.md` (implement,
  debug, review, design, explain; no `other/` — the neutral base) + an illustrative
  `prompts/system.example/` — plus a non-loaded `prompts/README.md`. The resolver only
  reads `<dir>/modes/<mode>/`, so `.example/` trees are never selected; an operator
  copies a file into the live slot to activate it. Byte-identical by construction (no
  `prompts/modes/` shipped), and crane's source filter drops `prompts/**.md` from the
  build entirely, so the gate is untouched. Two honesty refinements vs the sketch: the
  multi-tag frontmatter examples stay as a README illustration (not copy-ready files)
  with the loop-resolver caveat, so a naive copy can't silently mis-activate them; and
  `system.example/` is labelled illustrative because the multi-fragment base (`system/`)
  is still the one deferral from as-built 01 (the base is `system.md`/config today).
  **This is the last increment — the track is feature-complete.** Remaining work is the
  cross-track deferral: upgrading the loop resolver to the full `tags ⊆ ctx` model
  (frontmatter read + strip-on-inject) so `language:`/`tier:` fragments go live once
  their signal sources land.
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
