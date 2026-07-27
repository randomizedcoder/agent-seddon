# Prompt management — the `PromptStore` seam

See **and** edit every prompt the agent uses, from one surface. The loop does
**not** consume this seam — it is an operator/portal-facing management layer
(docs/design/portal) over the three homes a prompt lives in.

- **Trait:** `agent_core::PromptStore` ([`agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs))
- **Impl crate:** [`agent-prompt`](../../crates/agent-prompt) (`FilePromptStore`)
- **Cargo feature:** `prompt` (default on)
- **Service:** `agent --serve-prompt` (`agent.v1.PromptService`) — CRUD over gRPC;
  reflection- + `grpc.health.v1`-introspectable like every seam.

## What it manages

| [`PromptKind`] | Backing store | Default when no override |
|---|---|---|
| `System`   | `<prompts>/system.md` | config `[agent] system_prompt` |
| `Prepend`  | `<context.d>/prepend/NNNN_*.md` | — (each file *is* an entry) |
| `Append`   | `<context.d>/append/NNNN_*.md`  | — |
| `ModeLens` | `<prompts>/lens/<mode>.md` | compiled per-mode lens (`agent_context::lens`) |
| `SystemFragment` | `<prompts>/modes/<mode>/NNNN_*.md` | — (each file *is* a tagged entry) |

A `SystemFragment` is the management view of the situational fragments the loop
consumes (see "Situational fragments (consumed by the loop)" below): `list`/`get`
return one entry per `modes/<mode>/*.md` file, id `<mode>/<file>.md`. Its `tags` is a
**read projection** — the directory tag `mode:<mode>` unioned with any frontmatter
`tags: [..]`, bounded by `MAX_PROMPT_TAGS`/`MAX_PROMPT_TAG_LEN`. The file backend has
no separate tag column, so a `Put` persists tags by writing them into the fragment's
content (frontmatter); `Get` re-derives them. The `<mode>` in an id is validated
against the closed `TaskMode` set and the `<file>` via `safe_prompt_file`; a bad id is
`InvalidArgument`, never a traversal.

**Selecting by context.** `Select(PromptContext)` returns the situational fragments
selected for a tag set — the seam form of the loop's resolver — each as a full entry
(with its tags), so an operator sees exactly the set that composes and why.
`PreviewAssembled` takes a `PromptContext` (the pre-04 scalar `mode` field is honoured
as a fallback) and folds that selection into the previewed system message at index 1,
matching the loop's placement — so the preview answers *"show me the prompt for this
situation."* Both apply the same rule the loop uses today: a `modes/<mode>/` fragment
is selected when `mode:<mode>` is in the context. A fragment's finer frontmatter tags
ride on the entry for the catalog view but are not yet part of selection — so `Select`
and `PreviewAssembled` reflect exactly what the loop injects. Tags travel the wire as a
bounded, opaque `PromptContext { repeated string tags }`; a hostile tag is
string-compared only and simply matches nothing.

`builtin` on a returned entry means the served content is the compiled/config
default (no override file yet). `Put` writes an override file; `Delete` removes it
(reverting system/mode-lens to their default, or deleting a context.d file).

## The trait

```rust
#[async_trait]
pub trait PromptStore: Send + Sync {
    async fn list(&self, kind: Option<PromptKind>) -> Result<Vec<PromptEntry>>;
    async fn get(&self, r: &PromptRef) -> Result<PromptEntry>;
    async fn put(&self, entry: PromptEntry) -> Result<PromptEntry>;
    async fn delete(&self, r: &PromptRef) -> Result<bool>;
    async fn preview_assembled(&self, mode: TaskMode, goal: &str) -> Result<Vec<Message>>;
}
```

`preview_assembled` returns the `[system, user, (system-append)]` the model would
see for a goal, assembled from the *current* prompts. `mode` is informational:
initial assembly is mode-independent — the lens applies only at switch-compaction.

## When an edit takes effect

- **Mode lens — live.** `ModeAwareWindow` re-reads `<prompts>/lens/<mode>.md` on the
  next switch-compaction, so an edit applies with no restart (the resolver is
  [`agent_context::lens::LensPrompts`](context.md)).
- **System / prepend / append — next run.** These are read into an immutable
  `Settings` at startup (`resolve_system_prompt`, `context_files::load`), so an edit
  applies to the next run / session.

## Security

Every `id` is untrusted (it may become a filename), so the store **fails closed**: a
`context.d` id is validated (`safe_prompt_file`: reject empty, `..`, separators,
leading `-`, non-`.md`, over-long) and the resolved path is `confine`d to its root
(symlink-escape blocked); a mode-lens id must be a known `TaskMode`; content is
size-capped before any write. A rejected request is `InvalidArgument`, never a
traversal. Adversarial cases (traversal, oversized body, unknown mode) are covered
and asserted, including across the wire ([`roundtrip.rs`](../../crates/agent-grpc/tests/roundtrip.rs)).

## Situational fragments (consumed by the loop)

Distinct from the management surface above, the loop **does** consume one class of
prompt directly: *situational system fragments* — small per-mode markdown files that
are appended to the system prompt when the situation matches them
([`docs/design/prompts/`](../design/prompts/README.md)). The resolver is
[`agent_context::system_fragments::SystemFragments`](../../crates/agent-context/src/system_fragments.rs),
a sibling of the `LensPrompts` resolver in [`context.md`](context.md), rooted at the
same `[prompts] dir`.

- **What / where.** A fragment lives at `<prompts>/modes/<mode>/NNNN_*.md` (directory
  form; multiple fragments, `NNNN_`-ordered) or `<prompts>/modes/<mode>.md`
  (single-file, back-compat). It carries the tag `mode:<mode>`, and is selected when
  that tag is a subset of the turn's `PromptContext`. Only the **current mode** is a
  live context signal today; more tags (a wider catalog) land in later increments.
- **Placement.** Selected fragments are concatenated into **one volatile system
  message held at index 1** — right after the stable base head — so compaction
  preserves it as a *leading* system message. The base prompt stays with
  `agent_prompt::resolve_system_prompt` and is never duplicated into it.
- **Lifecycle.** Injected on first assembly, **swapped in place** on a mode switch,
  and **removed** when the new mode matches nothing. Reads are **live per turn**, so an
  edit through `PromptService` applies on the next turn with no restart — unlike
  system/prepend/append, which are next-*run*.
- **Observability.** Each change bumps `agent_prompt_fragments_selected_total{mode,
  action}`, `action ∈ {inserted, updated, removed}` — **counts and labels only, never
  the fragment text or tags**.
- **Byte-identical default.** With no `[prompts] dir` (or nothing selected) the
  resolver returns `Cow::Borrowed("")`, no message is added, and behaviour is exactly
  today's — the per-turn cost when unused is nil. See
  [`docs/design/prompts/02-composition.md`](../design/prompts/02-composition.md).
- **Example content.** Ready-to-use per-mode fragments (implement/debug/review/
  design/explain) ship as **inert templates** under `prompts/modes.example/`; copy a
  file into `prompts/modes/<mode>/` to activate it (live next context change). See
  [`prompts/README.md`](../../prompts/README.md) for the layout, the resolution
  ladder, and authoring guidance.

## Storage backends

The medium a prompt lives in is a **backend choice behind the seam** — every consumer
reaches prompts through the same CRUD, so files or a database is invisible to them
([`docs/design/prompts/05-storage.md`](../design/prompts/05-storage.md)). Selected by
`[prompts] backend`, using the same `match cfg.<x>.backend` template as the
`SessionStore`:

| `backend` | Impl | Notes |
|---|---|---|
| `file` (default) | `FilePromptStore` | git-legible markdown under `<prompts>`/`<context.d>`; zero dependencies |
| `sqlite` | `SqlitePromptStore` (feature `prompt-sqlite`) | embedded catalog; tags normalised into a `prompt_tags` table so `select` pushes down to SQL. The workspace's **only** DB dependency — off by default (`rusqlite`, `bundled`) |
| `grpc` | `GrpcPrompts` client | dials a central `PromptService` (a shared catalog, itself backed by any store) |

Selection semantics are identical across backends (`fragment.tags ⊆ context`); only
*where the filter runs* differs (in-memory vs SQL vs remote). The `sqlite` store returns
the same shape as the file store — `System`/`ModeLens` fall back to their
compiled/config default when no override row exists, and a `SystemFragment`'s `tags` are
derived from its content the same way — so the two are interchangeable. Move a catalog
between them with `agent_prompt::migrate(&from, &to)` (skips builtins; works in either
direction).

## Config

```toml
[prompts]
backend = "file"    # file (default) | sqlite (feature prompt-sqlite) | grpc
dir = "prompts"     # file backend: holds system.md + lens/<mode>.md overrides
                    # + modes/<mode>/ situational fragments; missing ⇒ defaults
db_path = ""        # sqlite backend: empty ⇒ <working_dir>/.agent-seddon/prompts.db
```

## Adding your own

Implement `PromptStore` (e.g. a database-backed store) in a new crate, register it,
and select it. The `FilePromptStore` is the reference filesystem impl. See
[`extending.md`](../extending.md) and the design track
[`design/portal/`](../design/portal/README.md).
